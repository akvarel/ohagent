//! Opt-in actual Jcode/provider acceptance. Identity remains an explicit private
//! fixture: this does not certify deployed OIDC or a real customer browser login.
use super::*;
use ohagent_core::jcode_bridge::{JcodeBridge, JcodeBridgeConfig};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit live cancellation acceptance requires existing provider environment"]
async fn live_cancel_actual_jcode_turn_after_instance_start() {
    assert_eq!(std::env::var("OHAGENT_LIVE_AAAS").as_deref(), Ok("1"));
    assert!(std::env::var_os("DEEPSEEK_API_KEY").is_some());
    let binary =
        std::env::var_os("OHAGENT_JCODE_BINARY").expect("select installed Jcode explicitly");
    let dir = tempfile::Builder::new().prefix("ac-").tempdir().unwrap();
    let bridge = Arc::new(
        JcodeBridge::new(Arc::new(jcode_base::provider::MultiProvider::new()))
            .with_config(JcodeBridgeConfig {
                runtime_root: Some(dir.path().join("r")),
                jcode_binary: Some(binary.into()),
            })
            .with_text_only_policy(),
    );
    let mut s = fixture(&dir.path().join("journal.db"));
    s.runtime = Some(Arc::new(
        crate::live_adapter::JcodeAnalyticalRuntime::new(bridge.clone(), dir.path().join("w"))
            .unwrap(),
    ));
    let (_, process) = request(&s, "POST", "/api/aaas/processes", "alice", json!({
        "title":"Synthetic cancellation acceptance", "goal":"Prepare a detailed analytical brief using only the supplied project materials.",
        "expected_result":"Detailed findings, missing facts and exact source quotations. No external actions.",
        "materials":"Project CANCEL-ACCEPTANCE-31. Integration tests passed on November 24, 2026. Deployment approval is missing. Budget is unknown. Synthetic acceptance data."
    }), None).await;
    let (created, run) = request(
        &s,
        "POST",
        "/api/aaas/runs",
        "alice",
        json!({"process_id":process["id"],"process_version":process["version"]}),
        Some("live-cancel-key"),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);
    let id = run["id"].as_str().unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let run = s.store.run("alice", id).unwrap();
        assert!(
            !run.terminal(),
            "turn finished before cancellation could be exercised"
        );
        if run.tasks.iter().any(|task| task.status == "running") {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let current = s.store.run("alice", id).unwrap();
    let (status, requested) = request(
        &s,
        "POST",
        &format!("/api/aaas/runs/{id}/cancel"),
        "alice",
        json!({"version":current.version}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(requested["status"], "cancellation_requested");
    loop {
        let run = s.store.run("alice", id).unwrap();
        if run.terminal() {
            assert_eq!(
                run.status, "cancelled",
                "logical SDK turn cancellation was not confirmed"
            );
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (events, _) = s.store.events("alice", id, 0).unwrap();
    assert!(events
        .iter()
        .any(|event| event.kind == "run.cancellation_requested"));
    assert!(events.iter().any(|event| event.kind == "run.cancelled"));
    assert!(!events.iter().any(|event| event.kind == "run.completed"));
    assert_eq!(s.store.artifact("alice", id), Err(BackendError::NotFound));
    if let Some(path) = std::env::var_os("OHAGENT_CANCEL_EVIDENCE") {
        std::fs::write(path,serde_json::to_vec_pretty(&json!({"kind":"actual-jcode-logical-cancellation-with-test-identity","executed_at":chrono::Utc::now().to_rfc3339(),"run":s.store.run("alice",id).unwrap(),"events":events,"limitation":"SDK logical TurnDone is not a physical provider termination guarantee"})).unwrap()).unwrap();
    }
    let permits = tokio::time::timeout(
        Duration::from_secs(15),
        s.runtime_slots.clone().acquire_many_owned(8),
    )
    .await
    .unwrap()
    .unwrap();
    for _ in 0..40 {
        if bridge.list_sessions("aaas:alice").await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(bridge.list_sessions("aaas:alice").await.is_empty());
    drop(permits);
    drop(s);
    drop(bridge);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit live acceptance requires installed Jcode and existing provider environment; performs real inference"]
async fn live_two_principals_actual_jcode_parallel_brief() {
    assert_eq!(
        std::env::var("OHAGENT_LIVE_AAAS").as_deref(),
        Ok("1"),
        "explicit live opt-in required"
    );
    assert!(
        std::env::var_os("DEEPSEEK_API_KEY").is_some(),
        "existing provider environment required"
    );
    let binary =
        std::env::var_os("OHAGENT_JCODE_BINARY").expect("select installed Jcode explicitly");
    let dir = tempfile::Builder::new().prefix("al-").tempdir().unwrap();
    let bridge = Arc::new(
        JcodeBridge::new(Arc::new(jcode_base::provider::MultiProvider::new()))
            .with_config(JcodeBridgeConfig {
                runtime_root: Some(dir.path().join("r")),
                jcode_binary: Some(binary.into()),
            })
            .with_text_only_policy(),
    );
    let mut s = fixture(&dir.path().join("journal.db"));
    s.runtime = Some(Arc::new(
        crate::live_adapter::JcodeAnalyticalRuntime::new(bridge.clone(), dir.path().join("w"))
            .unwrap(),
    ));
    let _router_factory = crate::live_adapter::router;
    let mut ids = Vec::new();
    for (tenant, marker, date, goal) in [
        ("alice", "ALPHA-ACCEPTANCE-71", "2026-12-03", "Assess release readiness and deployment approval gaps in English using only my supplied materials."),
        ("bob", "BETA-ACCEPTANCE-92", "2026-12-17", "Assess budget evidence gaps and information needed for a delivery decision in English using only my supplied materials."),
    ] {
        let input = json!({"title": format!("Live acceptance {tenant}"), "goal": goal,
            "expected_result": "Summarize status, delivery risk and missing information. Include an exact source quotation containing the project identifier. Never execute tools or external actions.",
            "materials": format!("Project {marker}. The integration test suite passed on November 24, 2026. Delivery target: {date}. The release owner has not approved the deployment. The budget has not been provided. This is synthetic acceptance material, not customer data.")});
        let (status, p) = request(&s, "POST", "/api/aaas/processes", tenant, input, None).await;
        assert_eq!(status, StatusCode::CREATED);
        let start = json!({"process_id": p["id"], "process_version": p["version"]});
        let (status, run) = request(
            &s,
            "POST",
            "/api/aaas/runs",
            tenant,
            start.clone(),
            Some("live-stable-key"),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (repeat, repeated) = request(
            &s,
            "POST",
            "/api/aaas/runs",
            tenant,
            start,
            Some("live-stable-key"),
        )
        .await;
        assert_eq!(repeat, StatusCode::OK);
        assert_eq!(run["id"], repeated["id"]);
        ids.push((tenant, marker, run["id"].as_str().unwrap().to_owned()));
    }
    let deadline = Instant::now() + Duration::from_secs(290);
    loop {
        let runs: Vec<_> = ids
            .iter()
            .map(|(tenant, _, id)| s.store.run(tenant, id).unwrap())
            .collect();
        if runs.iter().all(Run::terminal) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "live acceptance exceeded its deadline"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_ne!(
        s.store.run("alice", &ids[0].2).unwrap().goal,
        s.store.run("bob", &ids[1].2).unwrap().goal
    );
    let mut evidence = Vec::new();
    let mut instances = HashSet::new();
    for (tenant, marker, id) in &ids {
        let run = s.store.run(tenant, id).unwrap();
        assert_eq!(
            run.status, "completed",
            "actual runtime did not complete: {run:?}"
        );
        assert_eq!(run.tasks.len(), 4);
        assert_eq!(run.edges.len(), 4);
        assert!(run.tasks.iter().all(|task| task.status == "completed"));
        for task in &run.tasks {
            assert!(
                instances.insert(task.agent_instance_id.clone().unwrap()),
                "duplicate actual session"
            );
        }
        let branches: Vec<_> = run
            .tasks
            .iter()
            .filter(|t| ["analyst", "evidence_checker"].contains(&t.agent_definition_id.as_str()))
            .collect();
        let latest_start = branches
            .iter()
            .map(|t| t.started_at.as_ref().unwrap())
            .max()
            .unwrap();
        let first_finish = branches
            .iter()
            .map(|t| t.completed_at.as_ref().unwrap())
            .min()
            .unwrap();
        assert!(
            latest_start < first_finish,
            "actual branch intervals did not overlap"
        );
        let artifact = s.store.artifact(tenant, id).unwrap();
        assert!(
            artifact.contains(&marker.replace('-', "\\-")),
            "own source identifier missing from verified artifact"
        );
        let other = if *tenant == "alice" { "bob" } else { "alice" };
        assert_eq!(s.store.artifact(other, id), Err(BackendError::NotFound));
        let (status, _) = request(
            &s,
            "GET",
            &format!("/api/aaas/runs/{id}"),
            other,
            Value::Null,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (events, _) = s.store.events(tenant, id, 0).unwrap();
        assert!(events.iter().any(|e| e.kind == "artifact.verified"));
        evidence.push(json!({"identity": "private test principal, not deployed OIDC", "tenant": tenant, "run": run, "events": events, "artifact_markdown": artifact}));
    }
    if let Some(path) = std::env::var_os("OHAGENT_LIVE_EVIDENCE") {
        std::fs::write(path, serde_json::to_vec_pretty(&json!({"kind": "actual-jcode-provider-with-test-identities", "executed_at": chrono::Utc::now().to_rfc3339(), "runs": evidence})).unwrap()).unwrap();
    }
    // Runtime permits own all executing futures. Wait for them and cleanup guards
    // before dropping child-runtime owners and the test-only scratch directory.
    let permits = tokio::time::timeout(
        Duration::from_secs(15),
        s.runtime_slots.clone().acquire_many_owned(8),
    )
    .await
    .unwrap()
    .unwrap();
    for _ in 0..40 {
        if bridge.list_sessions("aaas:alice").await.is_empty()
            && bridge.list_sessions("aaas:bob").await.is_empty()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(bridge.list_sessions("aaas:alice").await.is_empty());
    assert!(bridge.list_sessions("aaas:bob").await.is_empty());
    drop(permits);
    drop(s);
    drop(bridge);
}
