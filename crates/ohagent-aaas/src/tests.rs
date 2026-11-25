use super::*;
use crate::auth::{Authorizer, Principal};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tower::ServiceExt;

#[path = "live_tests.rs"]
mod live_tests;

// Explicit test-only principal injection. There is no production constructor or env bypass.
struct TestAuth;
#[async_trait::async_trait]
impl Authorizer for TestAuth {
    async fn authorize(&self, token: &str) -> Result<Principal, BackendError> {
        match token {
            "alice" | "bob" => Ok(Principal {
                tenant: token.into(),
            }),
            _ => Err(BackendError::Unauthorized),
        }
    }
}
struct HoldingRuntime(AtomicUsize);
#[async_trait::async_trait]
impl Runtime for HoldingRuntime {
    async fn execute(&self, _execution: RunExecution, sink: EventSink) -> Result<(), String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        while !sink.is_cancelled() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        sink.confirm_cancelled().map_err(|e| e.to_string())
    }
}
fn fixture(path: &std::path::Path) -> State {
    State {
        store: Arc::new(store::Store::open(path).unwrap()),
        authorizer: Some(Arc::new(TestAuth)),
        public_auth: None,
        runtime: Some(Arc::new(HoldingRuntime(AtomicUsize::new(0)))),
        runtime_slots: Arc::new(tokio::sync::Semaphore::new(8)),
        stream_slots: Arc::new(tokio::sync::Semaphore::new(64)),
    }
}
fn input() -> ProcessInput {
    ProcessInput {
        title: "Fixture brief".into(),
        goal: "Fixture goal".into(),
        expected_result: "A bounded brief".into(),
        materials: "Labelled fixture material".into(),
    }
}
fn seeded(s: &State) -> (Run, EventSink) {
    let p = s.store.process("alice", input(), None).unwrap();
    let (run, execution) = s
        .store
        .start(
            "alice",
            "key",
            &StartRequest {
                process_id: p.id,
                process_version: p.version,
            },
        )
        .unwrap();
    let execution = execution.unwrap();
    let sink = EventSink {
        store: s.store.clone(),
        tenant: "alice".into(),
        run: run.id.clone(),
        epoch: execution.producer_epoch,
    };
    (run, sink)
}
async fn request(
    s: &State,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("x-tenant-id", "alice");
    if let Some(key) = key {
        req = req.header("idempotency-key", key);
    }
    let response = router(s.clone())
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}
#[tokio::test]
async fn router_two_tenants_duplicate_start_cancel_and_safe_rejections() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = fixture(&dir.path().join("journal.db"));
    let runtime = Arc::new(HoldingRuntime(AtomicUsize::new(0)));
    s.runtime = Some(runtime.clone());
    let (status, p) = request(
        &s,
        "POST",
        "/api/aaas/processes",
        "alice",
        serde_json::to_value(input()).unwrap(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let start = json!({"process_id":p["id"],"process_version":p["version"]});
    let (first, second) = tokio::join!(
        request(
            &s,
            "POST",
            "/api/aaas/runs",
            "alice",
            start.clone(),
            Some("same")
        ),
        request(
            &s,
            "POST",
            "/api/aaas/runs",
            "alice",
            start.clone(),
            Some("same")
        )
    );
    assert!([first.0, second.0].contains(&StatusCode::CREATED));
    assert!([first.0, second.0].contains(&StatusCode::OK));
    assert_eq!(first.1["id"], second.1["id"]);
    let rid = first.1["id"].as_str().unwrap();
    let path = format!("/api/aaas/runs/{rid}");
    for suffix in ["", "/artifact", "/events?after=0"] {
        assert_eq!(
            request(
                &s,
                "GET",
                &format!("{path}{suffix}"),
                "bob",
                json!(null),
                None
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(
            &s,
            "POST",
            "/api/aaas/runs",
            "bob",
            start.clone(),
            Some("same")
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut conflict = start.clone();
    conflict["process_version"] = json!(99);
    assert_eq!(
        request(
            &s,
            "POST",
            "/api/aaas/runs",
            "alice",
            conflict,
            Some("same")
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, current) = request(&s, "GET", &path, "alice", json!(null), None).await;
    let (status, cancel) = request(
        &s,
        "POST",
        &format!("{path}/cancel"),
        "alice",
        json!({"version":current["version"]}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancel["status"], "cancellation_requested");
    assert_eq!(
        request(
            &s,
            "POST",
            &format!("{path}/cancel"),
            "alice",
            json!({"version":current["version"]}),
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &s,
            "POST",
            "/api/aaas/processes",
            "alice",
            json!({"title":"x","tenant_id":"bob"}),
            None
        )
        .await
        .1["error"]["code"],
        "invalid_request"
    );
    assert_eq!(
        request(&s, "GET", "/api/aaas/processes", "bad", json!(null), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if s.store.run("alice", rid).unwrap().status == "cancelled" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(runtime.0.load(Ordering::SeqCst), 1);
}
#[test]
fn sqlite_atomic_replay_fencing_and_artifact_authorization() {
    let dir = tempfile::tempdir().unwrap();
    let s = fixture(&dir.path().join("journal.db"));
    let (run, sink) = seeded(&s);
    assert_eq!(sink.complete("premature"), Err(BackendError::Conflict));
    assert_eq!(s.store.run("alice", &run.id).unwrap().watermark, 1);
    let mut stale = sink.clone();
    stale.epoch = "old-producer".into();
    assert!(matches!(
        stale.start_instance("coordinator", "fixture-session"),
        Err(BackendError::Fenced)
    ));
    assert_eq!(s.store.run("alice", &run.id).unwrap().watermark, 1);
    for role in ["coordinator", "analyst", "evidence_checker", "reviewer"] {
        sink.dispatch(role).unwrap();
        let task = sink
            .start_instance(role, &format!("fixture-{role}"))
            .unwrap();
        sink.complete_task(&task.id, "Fixture completed").unwrap();
    }
    assert_eq!(sink.complete("  "), Err(BackendError::Invalid));
    sink.complete("# Labelled fixture artifact").unwrap();
    assert_eq!(
        s.store.artifact("alice", &run.id).unwrap(),
        "# Labelled fixture artifact"
    );
    assert_eq!(
        s.store.artifact("bob", &run.id),
        Err(BackendError::NotFound)
    );
    assert_eq!(sink.fail("late failure"), Err(BackendError::Fenced));
    let final_run = s.store.run("alice", &run.id).unwrap();
    let (events, done) = s.store.events("alice", &run.id, 0).unwrap();
    assert!(done);
    assert_eq!(events.len() as u64, final_run.watermark);
    for (i, event) in events.iter().enumerate() {
        assert_eq!(event.run_seq, i as u64 + 1);
        assert_eq!(event.payload["snapshot"]["watermark"], event.run_seq);
    }
    let replay = s.store.events("alice", &run.id, 2).unwrap().0;
    assert_eq!(replay[0].event_id, events[2].event_id);
    assert_eq!(
        s.store
            .events("alice", &run.id, final_run.watermark + 1)
            .unwrap_err(),
        BackendError::Cursor
    );
    s.store
        .0
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM aaas_events WHERE tenant=? AND run_id=? AND seq<=3",
            rusqlite::params!["alice", run.id],
        )
        .unwrap();
    assert_eq!(
        s.store.events("alice", &run.id, 0).unwrap_err(),
        BackendError::Cursor
    );
}
#[test]
fn sqlite_restart_interrupts_without_reexecution_and_pins_process_version() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("journal.db");
    let s = fixture(&db);
    let (run, sink) = seeded(&s);
    let p = s
        .store
        .process(
            "alice",
            ProcessInput {
                title: "Changed".into(),
                ..input()
            },
            Some(&run.process_id),
        )
        .unwrap();
    assert_eq!(p.version, 2);
    assert_eq!(s.store.run("alice", &run.id).unwrap().process_version, 1);
    sink.dispatch("coordinator").unwrap();
    sink.start_instance("coordinator", "fixture-first").unwrap();
    let reopened = store::Store::open(&db).unwrap();
    let interrupted = reopened.run("alice", &run.id).unwrap();
    assert_eq!(interrupted.status, "interrupted");
    assert_eq!(
        sink.complete_task(&run.tasks[0].id, "late"),
        Err(BackendError::Fenced)
    );
    let (same, execution) = reopened
        .start(
            "alice",
            "key",
            &StartRequest {
                process_id: run.process_id,
                process_version: 1,
            },
        )
        .unwrap();
    assert!(execution.is_none());
    assert_eq!(same.status, "interrupted");
    let (events, done) = reopened.events("alice", &run.id, 0).unwrap();
    assert!(done);
    assert_eq!(events.last().unwrap().kind, "run.interrupted");
}
#[tokio::test]
async fn missing_auth_is_closed_but_public_config_is_readable() {
    let dir = tempfile::tempdir().unwrap();
    let s = State::open(
        Config {
            database_path: dir.path().join("journal.db"),
            oidc: None,
        },
        None,
    )
    .await
    .unwrap();
    let (status, config) = request(&s, "GET", "/api/aaas/config", "", json!(null), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(config["auth"].is_null());
    assert_eq!(config["pilot_available"], false);
    assert_eq!(
        request(&s, "GET", "/api/aaas/runs", "alice", json!(null), None)
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}
#[tokio::test]
async fn router_durable_sse_replay_and_invalid_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let s = fixture(&dir.path().join("journal.db"));
    let (run, sink) = seeded(&s);
    sink.fail("fixture").unwrap();
    let response = router(s.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/aaas/runs/{}/events?after=0", run.id))
                .header("authorization", "Bearer alice")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("id: 1"));
    assert!(text.contains("id: 2"));
    assert!(text.contains("run.failed"));
    assert_eq!(
        request(
            &s,
            "GET",
            &format!("/api/aaas/runs/{}/events?after=999", run.id),
            "alice",
            json!(null),
            None
        )
        .await
        .1["error"]["code"],
        "reset_required"
    );
}

#[test]
fn sqlite_journal_failure_rolls_back_snapshot_artifact_and_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let s = fixture(&dir.path().join("journal.db"));
    let (run, sink) = seeded(&s);
    assert!(matches!(
        sink.start_instance("coordinator", "fixture-undispatched"),
        Err(BackendError::Conflict)
    ));
    s.store.0.lock().unwrap().execute_batch("CREATE TRIGGER fixture_reject_event BEFORE INSERT ON aaas_events BEGIN SELECT RAISE(ABORT, 'labelled fixture fault'); END;").unwrap();
    assert_eq!(sink.dispatch("coordinator"), Err(BackendError::Storage));
    assert_eq!(s.store.run("alice", &run.id).unwrap().watermark, 1);
    s.store
        .0
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fixture_reject_event")
        .unwrap();
    for role in ["coordinator", "analyst", "evidence_checker", "reviewer"] {
        sink.dispatch(role).unwrap();
        assert_eq!(sink.dispatch(role), Err(BackendError::Conflict));
        let task = sink
            .start_instance(role, &format!("fixture-{role}"))
            .unwrap();
        sink.complete_task(&task.id, "Fixture done").unwrap();
    }
    let watermark = s.store.run("alice", &run.id).unwrap().watermark;
    s.store.0.lock().unwrap().execute_batch("CREATE TRIGGER fixture_reject_event BEFORE INSERT ON aaas_events BEGIN SELECT RAISE(ABORT, 'labelled fixture fault'); END;").unwrap();
    assert_eq!(
        sink.complete("Fixture artifact"),
        Err(BackendError::Storage)
    );
    let unchanged = s.store.run("alice", &run.id).unwrap();
    assert_eq!(unchanged.watermark, watermark);
    assert!(unchanged.artifact.is_none());
    let stored: Option<String> = s
        .store
        .0
        .lock()
        .unwrap()
        .query_row(
            "SELECT artifact FROM aaas_runs WHERE tenant=? AND id=?",
            rusqlite::params!["alice", run.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(stored.is_none());
}

struct ExpiringAuth(AtomicUsize);
#[async_trait::async_trait]
impl Authorizer for ExpiringAuth {
    async fn authorize(&self, _token: &str) -> Result<Principal, BackendError> {
        if self.0.fetch_add(1, Ordering::SeqCst) < 2 {
            Ok(Principal {
                tenant: "alice".into(),
            })
        } else {
            Err(BackendError::Unauthorized)
        }
    }
}
#[tokio::test]
async fn sse_reauthorizes_each_bounded_batch_and_releases_capacity_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = fixture(&dir.path().join("journal.db"));
    let (run, _) = seeded(&s);
    for _ in 0..70 {
        s.store
            .update("alice", &run.id, None, "fixture.progress", |_| Ok(None))
            .unwrap();
    }
    s.authorizer = Some(Arc::new(ExpiringAuth(AtomicUsize::new(0))));
    s.stream_slots = Arc::new(tokio::sync::Semaphore::new(1));
    let response = router(s.clone())
        .oneshot(
            Request::builder()
                .uri(format!("/api/aaas/runs/{}/events?after=0", run.id))
                .header("authorization", "Bearer fixture")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(s.stream_slots.available_permits(), 0);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("id: 64\n"));
    assert!(!text.contains("id: 65\n"));
    assert!(text.contains("auth_required"));
    assert_eq!(s.stream_slots.available_permits(), 1);
}

#[tokio::test]
async fn capacity_does_not_break_idempotency_or_create_orphaned_runs() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = fixture(&dir.path().join("journal.db"));
    let (run, _) = seeded(&s);
    s.runtime_slots = Arc::new(tokio::sync::Semaphore::new(0));
    let start = json!({"process_id":run.process_id,"process_version":run.process_version});
    assert_eq!(
        request(
            &s,
            "POST",
            "/api/aaas/runs",
            "alice",
            start.clone(),
            Some("key")
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &s,
            "POST",
            "/api/aaas/runs",
            "alice",
            start,
            Some("new-key")
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(s.store.runs("alice").unwrap().len(), 1);
    s.stream_slots = Arc::new(tokio::sync::Semaphore::new(0));
    assert_eq!(
        request(
            &s,
            "GET",
            &format!("/api/aaas/runs/{}/events?after=0", run.id),
            "alice",
            json!(null),
            None
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[test]
fn independent_sqlite_connections_atomically_claim_one_idempotency_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.db");
    let first = Arc::new(store::Store::open(&path).unwrap());
    let second = Arc::new(store::Store::open(&path).unwrap());
    let process = first.process("alice", input(), None).unwrap();
    let req = StartRequest {
        process_id: process.id,
        process_version: process.version,
    };
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let mut handles = Vec::new();
    for store in [first.clone(), second] {
        let barrier = barrier.clone();
        let req = req.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            store.start("alice", "racing-key", &req).unwrap()
        }));
    }
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results[0].0.id, results[1].0.id);
    assert_eq!(
        results
            .iter()
            .filter(|(_, execution)| execution.is_some())
            .count(),
        1
    );
    assert_eq!(first.runs("alice").unwrap().len(), 1);
    assert_eq!(
        first.events("alice", &results[0].0.id, 0).unwrap().0.len(),
        1
    );
    let bob = first.process("bob", input(), None).unwrap();
    let (bob_run, execution) = first
        .start(
            "bob",
            "racing-key",
            &StartRequest {
                process_id: bob.id,
                process_version: bob.version,
            },
        )
        .unwrap();
    assert!(execution.is_some());
    assert_ne!(bob_run.id, results[0].0.id);
    let integrity: String = first
        .0
        .lock()
        .unwrap()
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

#[test]
fn cancellation_request_fences_all_producer_writes_until_engine_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let s = fixture(&dir.path().join("journal.db"));
    let (run, sink) = seeded(&s);
    sink.dispatch("coordinator").unwrap();
    let task = sink
        .start_instance("coordinator", "fixture-session")
        .unwrap();
    s.store
        .update(
            "alice",
            &run.id,
            None,
            "run.cancellation_requested",
            |run| {
                run.status = "cancellation_requested".into();
                Ok(None)
            },
        )
        .unwrap();
    let watermark = s.store.run("alice", &run.id).unwrap().watermark;
    assert!(sink.is_cancelled());
    assert_eq!(
        sink.complete_task(&task.id, "late output"),
        Err(BackendError::Fenced)
    );
    assert_eq!(sink.dispatch("analyst"), Err(BackendError::Fenced));
    assert_eq!(sink.complete("late artifact"), Err(BackendError::Fenced));
    assert_eq!(sink.fail("late failure"), Err(BackendError::Fenced));
    assert_eq!(s.store.run("alice", &run.id).unwrap().watermark, watermark);
    assert_eq!(
        s.store.run("alice", &run.id).unwrap().status,
        "cancellation_requested"
    );
    sink.confirm_cancelled().unwrap();
    assert_eq!(s.store.run("alice", &run.id).unwrap().status, "cancelled");
    assert_eq!(sink.confirm_cancelled(), Err(BackendError::Fenced));
}

// Fixture sessions only: this exercises durable orchestration, not real Jcode execution.
fn fixture_parallel_branches(s: &State, sink: &EventSink) -> Vec<Task> {
    sink.dispatch("coordinator").unwrap();
    let coordinator = sink
        .start_instance("coordinator", "fixture-coordinator")
        .unwrap();
    for role in ["analyst", "evidence_checker", "reviewer"] {
        assert_eq!(sink.dispatch(role), Err(BackendError::Conflict));
        assert!(matches!(
            sink.start_instance(role, "fixture-premature"),
            Err(BackendError::Conflict)
        ));
    }
    sink.complete_task(&coordinator.id, "Fixture plan").unwrap();
    // Both threads start their own fixture session before either task is completed.
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = ["analyst", "evidence_checker"]
        .into_iter()
        .map(|role| {
            let sink = sink.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                sink.dispatch(role).unwrap();
                sink.start_instance(role, &format!("fixture-{role}"))
                    .unwrap()
            })
        })
        .collect();
    let branches: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let snapshot = s.store.run("alice", &sink.run).unwrap();
    assert_eq!(
        snapshot
            .tasks
            .iter()
            .filter(|task| task.status == "running")
            .count(),
        2
    );
    assert_ne!(branches[0].agent_instance_id, branches[1].agent_instance_id);
    branches
}

#[test]
fn fixture_dag_parallel_branches_merge_only_after_both_complete() {
    for first_completed in [0, 1] {
        let dir = tempfile::tempdir().unwrap();
        let s = fixture(&dir.path().join("journal.db"));
        let (run, sink) = seeded(&s);
        assert_eq!(run.tasks.len(), 4);
        assert_eq!(run.edges.len(), 4);
        let topology: Vec<_> = run
            .edges
            .iter()
            .map(|edge| {
                let role = |id: &str| {
                    run.tasks
                        .iter()
                        .find(|task| task.id == id)
                        .unwrap()
                        .agent_definition_id
                        .clone()
                };
                (role(&edge.from), role(&edge.to))
            })
            .collect();
        assert_eq!(
            topology,
            vec![
                ("coordinator".into(), "analyst".into()),
                ("coordinator".into(), "evidence_checker".into()),
                ("analyst".into(), "reviewer".into()),
                ("evidence_checker".into(), "reviewer".into()),
            ]
        );
        // Reorder the stored task array to prove readiness is graph-based.
        s.store
            .update("alice", &run.id, None, "fixture.reorder", |run| {
                run.tasks.reverse();
                Ok(None)
            })
            .unwrap();
        let branches = fixture_parallel_branches(&s, &sink);
        for completed in [false, true] {
            if completed {
                sink.complete_task(&branches[first_completed].id, "Fixture branch result")
                    .unwrap();
            }
            let watermark = s.store.run("alice", &run.id).unwrap().watermark;
            assert_eq!(sink.dispatch("reviewer"), Err(BackendError::Conflict));
            assert!(matches!(
                sink.start_instance("reviewer", "fixture-reviewer"),
                Err(BackendError::Conflict)
            ));
            assert_eq!(
                sink.complete("Premature fixture artifact"),
                Err(BackendError::Conflict)
            );
            assert_eq!(s.store.run("alice", &run.id).unwrap().watermark, watermark);
        }
        sink.complete_task(&branches[1 - first_completed].id, "Fixture second result")
            .unwrap();
        assert!(matches!(
            sink.start_instance("reviewer", "fixture-undispatched"),
            Err(BackendError::Conflict)
        ));
        sink.dispatch("reviewer").unwrap();
        assert_eq!(sink.dispatch("reviewer"), Err(BackendError::Conflict));
        let reviewer = sink.start_instance("reviewer", "fixture-reviewer").unwrap();
        let reviewing = s.store.run("alice", &run.id).unwrap();
        assert!(reviewing
            .edges
            .iter()
            .filter(|edge| edge.to == reviewer.id)
            .all(|edge| edge.status == "dispatched"));
        sink.complete_task(&reviewer.id, "Fixture merged review")
            .unwrap();
        sink.complete("# Labelled fork-join fixture artifact")
            .unwrap();
        let finished = s.store.run("alice", &run.id).unwrap();
        assert_eq!(finished.status, "completed");
        assert!(finished.tasks.iter().all(|task| task.status == "completed"));
        assert!(finished.edges.iter().all(|edge| edge.status == "completed"));
        let (events, done) = s.store.events("alice", &run.id, 0).unwrap();
        assert!(done);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == "task.dispatched")
                .count(),
            4
        );
        assert!(events.iter().any(|event| event.payload["snapshot"]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|task| task["status"] == "running")
            .count()
            == 2));
        assert_eq!(
            s.store.artifact("bob", &run.id),
            Err(BackendError::NotFound)
        );
    }
}

#[test]
fn fixture_dag_restart_fences_both_running_branches_without_reexecution() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("journal.db");
    let s = fixture(&db);
    let (run, sink) = seeded(&s);
    let branches = fixture_parallel_branches(&s, &sink);
    let reopened = store::Store::open(&db).unwrap();
    let interrupted = reopened.run("alice", &run.id).unwrap();
    assert_eq!(interrupted.status, "interrupted");
    for branch in branches {
        let stored = interrupted
            .tasks
            .iter()
            .find(|task| task.id == branch.id)
            .unwrap();
        assert_eq!(stored.status, "failed");
        assert_eq!(stored.error_code.as_deref(), Some("runtime_interrupted"));
        assert_eq!(
            sink.complete_task(&branch.id, "Late fixture output"),
            Err(BackendError::Fenced)
        );
    }
    assert_eq!(sink.dispatch("reviewer"), Err(BackendError::Fenced));
    assert!(matches!(
        sink.start_instance("reviewer", "fixture-late"),
        Err(BackendError::Fenced)
    ));
    let (same, execution) = reopened
        .start(
            "alice",
            "key",
            &StartRequest {
                process_id: run.process_id,
                process_version: run.process_version,
            },
        )
        .unwrap();
    assert_eq!(same.id, run.id);
    assert!(execution.is_none());
    assert_eq!(same.watermark, interrupted.watermark);
    let again = store::Store::open(&db).unwrap();
    assert_eq!(
        again.run("alice", &run.id).unwrap().watermark,
        interrupted.watermark
    );
}

#[tokio::test]
async fn public_template_lists_all_four_dag_roles() {
    let dir = tempfile::tempdir().unwrap();
    let s = fixture(&dir.path().join("journal.db"));
    let (status, body) =
        request(&s, "GET", "/api/aaas/processes", "alice", json!(null), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["template"]["roles"],
        json!(["coordinator", "analyst", "evidence_checker", "reviewer"])
    );
}

#[test]
fn fixture_dag_cancellation_fences_both_running_branches() {
    let dir = tempfile::tempdir().unwrap();
    let s = fixture(&dir.path().join("journal.db"));
    let (run, sink) = seeded(&s);
    let branches = fixture_parallel_branches(&s, &sink);
    s.store
        .update(
            "alice",
            &run.id,
            None,
            "run.cancellation_requested",
            |run| {
                run.status = "cancellation_requested".into();
                Ok(None)
            },
        )
        .unwrap();
    let watermark = s.store.run("alice", &run.id).unwrap().watermark;
    for branch in &branches {
        assert_eq!(
            sink.complete_task(&branch.id, "Late branch output"),
            Err(BackendError::Fenced)
        );
    }
    assert_eq!(sink.dispatch("reviewer"), Err(BackendError::Fenced));
    assert!(matches!(
        sink.start_instance("reviewer", "fixture-late-reviewer"),
        Err(BackendError::Fenced)
    ));
    assert_eq!(s.store.run("alice", &run.id).unwrap().watermark, watermark);
    sink.confirm_cancelled().unwrap();
    let cancelled = s.store.run("alice", &run.id).unwrap();
    assert_eq!(cancelled.status, "cancelled");
    for branch in branches {
        assert_eq!(
            cancelled
                .tasks
                .iter()
                .find(|task| task.id == branch.id)
                .unwrap()
                .status,
            "cancelled"
        );
    }
}

#[test]
fn fixture_dag_start_rechecks_each_predecessor_even_with_durable_dispatch() {
    for blocked_role in ["analyst", "evidence_checker"] {
        let dir = tempfile::tempdir().unwrap();
        let s = fixture(&dir.path().join("journal.db"));
        let (run, sink) = seeded(&s);
        let branches = fixture_parallel_branches(&s, &sink);
        for branch in &branches {
            sink.complete_task(&branch.id, "Fixture completed branch")
                .unwrap();
        }
        sink.dispatch("reviewer").unwrap();
        // Deliberate test-only persisted-state fault, isolating start's own readiness guard
        // from both the dispatch journal guard and incoming-edge dispatch status guard.
        s.store
            .update("alice", &run.id, None, "fixture.predecessor_fault", |run| {
                run.tasks
                    .iter_mut()
                    .find(|task| task.agent_definition_id == blocked_role)
                    .unwrap()
                    .status = "running".into();
                Ok(None)
            })
            .unwrap();
        let watermark = s.store.run("alice", &run.id).unwrap().watermark;
        assert!(matches!(
            sink.start_instance("reviewer", "fixture-reviewer"),
            Err(BackendError::Conflict)
        ));
        assert_eq!(s.store.run("alice", &run.id).unwrap().watermark, watermark);
        let branch = branches
            .iter()
            .find(|task| task.agent_definition_id == blocked_role)
            .unwrap();
        sink.complete_task(&branch.id, "Fixture restored predecessor")
            .unwrap();
        sink.start_instance("reviewer", "fixture-reviewer").unwrap();
    }
}

#[test]
fn pilot_quotas_bound_retained_versions_and_runs_without_breaking_retries() {
    let store = store::Store::open(std::path::Path::new(":memory:")).unwrap();
    let first = store.process("alice", input(), None).unwrap();
    for _ in 1..256 {
        store.process("alice", input(), Some(&first.id)).unwrap();
    }
    assert!(matches!(
        store.process("alice", input(), Some(&first.id)),
        Err(BackendError::Capacity)
    ));
    assert!(store.process("bob", input(), None).is_ok());
    let request = StartRequest {
        process_id: first.id,
        process_version: 1,
    };
    let original = store.start("alice", "key-0", &request).unwrap().0;
    for i in 1..100 {
        store.start("alice", &format!("key-{i}"), &request).unwrap();
    }
    assert!(matches!(
        store.start("alice", "overflow", &request),
        Err(BackendError::Capacity)
    ));
    assert_eq!(
        store.start("alice", "key-0", &request).unwrap().0.id,
        original.id
    );
}
