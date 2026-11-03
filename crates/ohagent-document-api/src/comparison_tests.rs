use super::*;
use ohagent_documents::{
    DocumentFields, DocumentType, Extraction, ModelProviderFactory, ModelSlot,
};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicUsize, Ordering},
};

const SERVICE: &str = "comparison-test-service-key-000";
const ADMIN: &str = "comparison-test-administrator-key-000";
struct Fixture {
    fail: bool,
    calls: Arc<AtomicUsize>,
}
#[async_trait::async_trait]
impl VisionProvider for Fixture {
    async fn extract(&self, _: ExtractRequest) -> Result<Extraction, DocumentError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(DocumentError::Transport);
        }
        Ok(Extraction {
            document_type: DocumentType::Invoice,
            fields: DocumentFields {
                total_amount: Some("12.30".into()),
                ..Default::default()
            },
            evidence: vec![],
            warnings: vec![],
            model: "fixture-untrusted".into(),
        })
    }
}
fn state(fail: [bool; 3]) -> (DocumentApiState, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let models = ["a", "b", "c"]
        .into_iter()
        .map(|id| ModelSlot {
            id: id.into(),
            label: format!("Operator {id}"),
            model: format!("operator-{id}-v1"),
            enabled: true,
        })
        .collect();
    let factories: HashMap<String, Arc<dyn ModelProviderFactory>> = ["a", "b", "c"]
        .into_iter()
        .zip(fail)
        .map(|(id, fail)| {
            let provider: Arc<dyn VisionProvider> = Arc::new(Fixture {
                fail,
                calls: calls.clone(),
            });
            let factory: Arc<dyn ModelProviderFactory> =
                Arc::new(move |_: &str| Ok(provider.clone()));
            (id.into(), factory)
        })
        .collect();
    let service = ComparisonService::in_memory(
        ModelRegistry {
            revision: 1,
            models,
        },
        factories,
    )
    .unwrap();
    (
        DocumentApiState::unavailable(SERVICE.into())
            .with_comparison(Arc::new(service), Some(ADMIN.into())),
        calls,
    )
}
fn document() -> serde_json::Value {
    serde_json::json!({"file_name":"photo.png","media_type":"image/png","content_base64":"iVBORw0KGgpmaXh0dXJl"})
}
struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start(state: DocumentApiState) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/api/document-processing",
            listener.local_addr().unwrap()
        );
        let task = tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
        Self { url, task }
    }
    fn call(&self, method: reqwest::Method, path: &str, key: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(key)
    }
    async fn compare(&self) -> reqwest::Response {
        self.call(reqwest::Method::POST, "/compare", SERVICE)
            .header("X-Tenant-Id", "tenant-a")
            .json(&document())
            .send()
            .await
            .unwrap()
    }
    async fn get(&self) -> serde_json::Value {
        let response = self
            .call(reqwest::Method::GET, "/models", ADMIN)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        response.json().await.unwrap()
    }
    async fn put(&self, value: &serde_json::Value) -> reqwest::Response {
        self.call(reqwest::Method::PUT, "/models", ADMIN)
            .json(value)
            .send()
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn compare_http_contract_preserves_partial_failures_and_disabled_slots() {
    let (state, calls) = state([true, false, false]);
    let server = Server::start(state).await;
    let mut policy = server.get().await;
    policy["models"][2]["enabled"] = false.into();
    assert_eq!(server.put(&policy).await.status(), 200);
    let response = server.compare().await;
    assert_eq!(response.status(), 200);
    let result: serde_json::Value = response.json().await.unwrap();
    assert_eq!(result["extraction"]["model"], "operator-b-v1");
    assert_eq!(result["comparison"]["policy_revision"], 2);
    assert_eq!(result["comparison"]["complete"], false);
    assert_eq!(result["comparison"]["results"][0]["status"], "failed");
    assert_eq!(
        result["comparison"]["results"][0]["error_code"],
        "provider_failed"
    );
    assert!(result["comparison"]["results"][0]["extraction"].is_null());
    assert_eq!(result["comparison"]["results"][1]["status"], "succeeded");
    assert_eq!(result["comparison"]["results"][2]["status"], "disabled");
    assert!(result["comparison"]["results"][2]["error_code"].is_null());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(result.as_object().unwrap().len(), 2);
}

#[tokio::test]
async fn service_key_cannot_administer_models_and_admin_key_cannot_extract() {
    let (state, _) = state([false; 3]);
    let server = Server::start(state).await;
    for method in [reqwest::Method::GET, reqwest::Method::PUT] {
        assert_eq!(
            server
                .call(method, "/models", SERVICE)
                .header("X-Tenant-Id", "tenant-a")
                .json(&serde_json::json!({}))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    for path in ["/compare", "/extract"] {
        assert_eq!(
            server
                .call(reqwest::Method::POST, path, ADMIN)
                .header("X-Tenant-Id", "tenant-a")
                .json(&document())
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
    }
    let public = server.get().await.to_string();
    for forbidden in [
        SERVICE,
        ADMIN,
        "base_url",
        "api_key",
        "api_key_env",
        "render_image",
    ] {
        assert!(!public.contains(forbidden));
    }
}

#[tokio::test]
async fn missing_or_identical_administrator_credentials_fail_closed() {
    for key in [None, Some(SERVICE.into()), Some("too-short".into())] {
        let (mut state, _) = state([false; 3]);
        let service = state.comparison.take().unwrap();
        let server = Server::start(state.with_comparison(service, key)).await;
        assert_eq!(
            server
                .call(reqwest::Method::GET, "/models", SERVICE)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        assert_eq!(
            server
                .call(reqwest::Method::GET, "/models", ADMIN)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        assert_eq!(
            server.compare().await.status(),
            200,
            "admin auth absence does not disable configured comparison"
        );
    }
}

#[tokio::test]
async fn model_versions_change_without_restart_and_stale_put_conflicts() {
    let (state, _) = state([false; 3]);
    let server = Server::start(state).await;
    let original = server.get().await;
    let mut updated = original.clone();
    updated["models"][0]["model"] = "operator-a-new-version".into();
    updated["models"][1]["enabled"] = false.into();
    let response = server.put(&updated).await;
    assert_eq!(response.status(), 200);
    let response: serde_json::Value = response.json().await.unwrap();
    assert_eq!(response["revision"], 2);
    assert_eq!(server.put(&original).await.status(), 409);
    assert_eq!(server.get().await, response);
    let comparison: serde_json::Value = server.compare().await.json().await.unwrap();
    assert_eq!(comparison["comparison"]["policy_revision"], 2);
    assert_eq!(comparison["extraction"]["model"], "operator-a-new-version");
    assert_eq!(comparison["comparison"]["results"][1]["status"], "disabled");
}

#[tokio::test]
async fn settings_reject_unknown_endpoint_and_secret_properties() {
    let (state, _) = state([false; 3]);
    let server = Server::start(state).await;
    let original = server.get().await;
    for field in [
        "base_url",
        "api_key_env",
        "api_key",
        "render_image",
        "tenant_id",
    ] {
        let mut changed = original.clone();
        changed["models"][0][field] = "not-allowed".into();
        assert_eq!(server.put(&changed).await.status(), 400, "field {field}");
    }
    let mut changed = original.clone();
    changed["models"][0]["id"] = "new-profile".into();
    assert_eq!(server.put(&changed).await.status(), 400);
    assert_eq!(server.get().await, original);
    let oversized = " ".repeat(16 * 1024 + 1);
    let response = server
        .call(reqwest::Method::PUT, "/models", ADMIN)
        .header("Content-Type", "application/json")
        .body(oversized)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 413);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["error"]["message_key"],
        "documents.model_registry_too_large"
    );
}

#[tokio::test]
async fn all_disabled_settings_succeed_but_comparison_makes_no_calls() {
    let (state, calls) = state([false; 3]);
    let server = Server::start(state).await;
    let mut policy = server.get().await;
    for model in policy["models"].as_array_mut().unwrap() {
        model["enabled"] = false.into();
    }
    assert_eq!(server.put(&policy).await.status(), 200);
    let response = server.compare().await;
    assert_eq!(response.status(), 503);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        "all_models_disabled"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn all_failed_comparison_returns_200_with_null_candidate() {
    let (state, _) = state([true; 3]);
    let server = Server::start(state).await;
    let response = server.compare().await;
    assert_eq!(response.status(), 200);
    let result: serde_json::Value = response.json().await.unwrap();
    assert!(result["extraction"].is_null());
    assert_eq!(result["comparison"]["complete"], false);
    assert_eq!(result["comparison"]["results"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn missing_registry_never_falls_back_to_the_legacy_provider() {
    let calls = Arc::new(AtomicUsize::new(0));
    let state = DocumentApiState::with_provider(
        SERVICE.into(),
        Arc::new(Fixture {
            fail: false,
            calls: calls.clone(),
        }),
    );
    let server = Server::start(state).await;
    assert_eq!(server.compare().await.status(), 503);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        server
            .call(reqwest::Method::POST, "/extract", SERVICE)
            .header("X-Tenant-Id", "tenant-a")
            .json(&document())
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

async fn incomplete(
    state: DocumentApiState,
    method: &str,
    path: &str,
    auth: &str,
    tenant: &str,
) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let server = Server::start(state).await;
    let host = server
        .url
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let mut stream = tokio::net::TcpStream::connect(host).await.unwrap();
    stream.write_all(format!("{method} /api/document-processing/{path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {auth}\r\nX-Tenant-Id: {tenant}\r\nContent-Type: application/json\r\nContent-Length: 10000000\r\n\r\n{{").as_bytes()).await.unwrap();
    let mut buffer = [0u8; 512];
    let count = tokio::time::timeout(std::time::Duration::from_secs(1), stream.read(&mut buffer))
        .await
        .expect("must reject before buffering body")
        .unwrap();
    std::str::from_utf8(&buffer[..count])
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn comparison_and_admin_admission_reject_before_reading_incomplete_bodies() {
    assert_eq!(
        incomplete(state([false; 3]).0, "POST", "compare", "wrong", "tenant-a").await,
        401
    );
    assert_eq!(
        incomplete(
            state([false; 3]).0,
            "POST",
            "compare",
            SERVICE,
            "../tenant-b"
        )
        .await,
        400
    );
    assert_eq!(
        incomplete(state([false; 3]).0, "PUT", "models", SERVICE, "tenant-a").await,
        401
    );
    let (state, _) = state([false; 3]);
    let permits = state.permits.clone();
    let _held = permits.acquire_many(4).await.unwrap();
    assert_eq!(
        incomplete(state.clone(), "POST", "compare", SERVICE, "tenant-a").await,
        429
    );
    assert_eq!(incomplete(state, "PUT", "models", ADMIN, "").await, 429);
}
