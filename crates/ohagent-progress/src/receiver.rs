//! Standalone receiver: one explicitly configured tenant, separate read and publish credentials.
use crate::{now_ms, valid_id, Snapshot, MAX_EVENTS, MAX_SESSIONS};
use axum::{
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Serialize)]
pub struct ReceivedSnapshot {
    pub received_at: i64,
    pub stale: bool,
    pub snapshot: Snapshot,
}
#[derive(Clone)]
pub struct Receiver {
    tenant: String,
    view_token: String,
    publish_token: String,
    snapshots: Arc<Mutex<BTreeMap<String, ReceivedSnapshot>>>,
}
impl Receiver {
    pub fn new(
        tenant: String,
        view_token: String,
        publish_token: String,
    ) -> Result<Self, &'static str> {
        if !valid_id(&tenant)
            || view_token.len() < 24
            || publish_token.len() < 24
            || view_token == publish_token
        {
            return Err("receiver requires a tenant and different existing read/publish tokens of at least 24 bytes");
        }
        Ok(Self {
            tenant,
            view_token,
            publish_token,
            snapshots: Default::default(),
        })
    }
    pub fn router(self) -> Router {
        Router::new()
            .route("/api/progress", get(read))
            .route("/api/progress/snapshots", post(ingest))
            .layer(DefaultBodyLimit::max(1024 * 1024))
            .with_state(self)
    }
}
fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
    else {
        return false;
    };
    token.len() == expected.len()
        && token
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |n, (a, b)| n | (a ^ b))
            == 0
}
#[derive(Deserialize)]
struct Scope {
    tenant_id: String,
}
async fn read(
    State(state): State<Receiver>,
    headers: HeaderMap,
    Query(scope): Query<Scope>,
) -> Result<Json<Vec<ReceivedSnapshot>>, StatusCode> {
    if !authorized(&headers, &state.view_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if scope.tenant_id != state.tenant {
        return Err(StatusCode::FORBIDDEN);
    }
    let snapshots = state.snapshots.lock().unwrap_or_else(|e| e.into_inner());
    let now = now_ms();
    Ok(Json(
        snapshots
            .values()
            .map(|v| {
                let mut v = v.clone();
                v.stale = now - v.received_at > 90_000;
                v
            })
            .collect(),
    ))
}
fn valid_snapshot(snapshot: &Snapshot) -> bool {
    let now = now_ms();
    snapshot.version == 1
        && valid_id(&snapshot.source_id)
        && valid_id(&snapshot.boot_id)
        && snapshot.sequence > 0
        && now.abs_diff(snapshot.generated_at) <= 120_000
        && snapshot.sessions.len() <= MAX_SESSIONS
        && snapshot.sessions.iter().all(|s| {
            s.id.len() == 24
                && s.id.bytes().all(|c| c.is_ascii_hexdigit())
                && s.started_at >= 0
                && s.started_at <= s.updated_at
                && s.updated_at <= now + 120_000
                && s.tools.len() <= 64
                && s.tools.keys().all(|t| valid_id(t) && t.len() <= 64)
                && s.current_tool
                    .as_ref()
                    .is_none_or(|t| valid_id(t) && t.len() <= 64)
                && s.events.len() <= MAX_EVENTS
                && s.events.iter().all(|e| {
                    e.at >= 0
                        && e.at <= now + 120_000
                        && e.tool.as_ref().is_none_or(|t| valid_id(t) && t.len() <= 64)
                })
        })
}
async fn ingest(
    State(state): State<Receiver>,
    headers: HeaderMap,
    Json(snapshot): Json<Snapshot>,
) -> Result<StatusCode, StatusCode> {
    if !authorized(&headers, &state.publish_token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if snapshot.tenant_id != state.tenant {
        return Err(StatusCode::FORBIDDEN);
    }
    if !valid_snapshot(&snapshot) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let mut all = state.snapshots.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(previous) = all.get(&snapshot.source_id) {
        // Retries are harmless. Never replace current state with an older heartbeat,
        // including an old process arriving after the publisher restarted.
        if snapshot.generated_at < previous.snapshot.generated_at
            || (snapshot.boot_id == previous.snapshot.boot_id
                && snapshot.sequence < previous.snapshot.sequence)
        {
            return Err(StatusCode::CONFLICT);
        }
        if snapshot.boot_id == previous.snapshot.boot_id
            && snapshot.sequence == previous.snapshot.sequence
        {
            return if snapshot == previous.snapshot {
                Ok(StatusCode::NO_CONTENT)
            } else {
                Err(StatusCode::CONFLICT)
            };
        }
    } else if all.len() >= 32 {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    all.insert(
        snapshot.source_id.clone(),
        ReceivedSnapshot {
            received_at: now_ms(),
            stale: false,
            snapshot,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        publisher::{publish_once, PublisherConfig},
        Activity, ProgressStore,
    };
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    fn receiver() -> Receiver {
        Receiver::new("tenant".into(), "r".repeat(32), "w".repeat(32)).unwrap()
    }
    fn snapshot() -> Snapshot {
        let store = ProgressStore::default();
        store.update(
            "tenant",
            "private",
            Activity::Working,
            Some("read"),
            Some((40, 3)),
        );
        store.snapshot("tenant", "desktop", 2)
    }
    async fn post(app: Router, value: serde_json::Value, token: &str) -> StatusCode {
        app.oneshot(
            Request::post("/api/progress/snapshots")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(value.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
    }
    #[tokio::test]
    async fn read_and_write_scopes_are_separate_and_tenant_bound() {
        let app = receiver().router();
        let value = serde_json::to_value(snapshot()).unwrap();
        assert_eq!(
            post(app.clone(), value.clone(), &"r".repeat(32)).await,
            StatusCode::UNAUTHORIZED
        );
        let mut foreign = value.clone();
        foreign["tenant_id"] = "other".into();
        assert_eq!(
            post(app.clone(), foreign, &"w".repeat(32)).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            post(app.clone(), value, &"w".repeat(32)).await,
            StatusCode::NO_CONTENT
        );
        for (token, tenant, code) in [
            ("r", "tenant", StatusCode::OK),
            ("w", "tenant", StatusCode::UNAUTHORIZED),
            ("r", "other", StatusCode::FORBIDDEN),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("/api/progress?tenant_id={tenant}"))
                        .header("authorization", format!("Bearer {}", token.repeat(32)))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), code);
        }
    }
    #[tokio::test]
    async fn stale_replay_unknown_fields_and_oversize_are_rejected() {
        let app = receiver().router();
        let value = serde_json::to_value(snapshot()).unwrap();
        assert_eq!(
            post(app.clone(), value.clone(), &"w".repeat(32)).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            post(app.clone(), value.clone(), &"w".repeat(32)).await,
            StatusCode::NO_CONTENT
        );
        let mut old = value.clone();
        old["sequence"] = 1.into();
        assert_eq!(
            post(app.clone(), old, &"w".repeat(32)).await,
            StatusCode::CONFLICT
        );
        let mut stale = value.clone();
        stale["generated_at"] = 0.into();
        assert_eq!(
            post(app.clone(), stale, &"w".repeat(32)).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let mut extra = value.clone();
        extra["prompt"] = "private".into();
        assert_eq!(
            post(app.clone(), extra, &"w".repeat(32)).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let mut huge = value;
        huge["sessions"] = serde_json::json!(vec![snapshot().sessions[0].clone(); 129]);
        assert_eq!(
            post(app, huge, &"w".repeat(32)).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    #[tokio::test]
    async fn real_http_publisher_receiver_round_trip() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, receiver().router()).await.unwrap();
        });
        let store = ProgressStore::default();
        store.update("tenant", "s", Activity::Waiting, Some("read"), None);
        store.update("other", "hidden", Activity::Failed, None, None);
        let config = PublisherConfig::new(
            &format!("http://{addr}/api/progress/snapshots"),
            "w".repeat(32),
            "tenant".into(),
            "desktop".into(),
        )
        .unwrap();
        let client = reqwest::Client::new();
        publish_once(&client, &config, &store, 1).await.unwrap();
        let response = client
            .get(format!("http://{addr}/api/progress?tenant_id=tenant"))
            .bearer_auth("r".repeat(32))
            .send()
            .await
            .unwrap();
        let data: serde_json::Value = response.json().await.unwrap();
        assert_eq!(data[0]["snapshot"]["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(data[0]["snapshot"]["sessions"][0]["status"], "waiting");
        assert_eq!(data[0]["stale"], false);
        task.abort();
    }

    #[tokio::test]
    async fn extreme_timestamps_and_changed_retries_are_rejected() {
        let app = receiver().router();
        let value = serde_json::to_value(snapshot()).unwrap();
        assert_eq!(
            post(app.clone(), value.clone(), &"w".repeat(32)).await,
            StatusCode::NO_CONTENT
        );
        let mut changed = value.clone();
        changed["sessions"][0]["input_tokens"] = 999.into();
        assert_eq!(
            post(app.clone(), changed, &"w".repeat(32)).await,
            StatusCode::CONFLICT
        );
        for timestamp in [i64::MIN, i64::MAX] {
            let mut extreme = value.clone();
            extreme["generated_at"] = timestamp.into();
            assert_eq!(
                post(app.clone(), extreme, &"w".repeat(32)).await,
                StatusCode::UNPROCESSABLE_ENTITY
            );
        }
    }

    #[tokio::test]
    async fn old_receipts_stay_stale_and_http_payload_is_bounded() {
        let state = receiver();
        state.snapshots.lock().unwrap().insert(
            "desktop".into(),
            ReceivedSnapshot {
                received_at: now_ms() - 100_000,
                stale: false,
                snapshot: snapshot(),
            },
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", "r".repeat(32)).parse().unwrap(),
        );
        let response = read(
            State(state.clone()),
            headers,
            Query(Scope {
                tenant_id: "tenant".into(),
            }),
        )
        .await
        .unwrap();
        assert!(response.0[0].stale);
        let response = state
            .router()
            .oneshot(
                Request::post("/api/progress/snapshots")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", "w".repeat(32)))
                    .body(Body::from(" ".repeat(1024 * 1024 + 1)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
