//! Actual binary, boot environment, file registry and three loopback HTTP providers.
//! Synthetic documents and synthetic keys only, never external services or live quality.
use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use ohagent_documents::{DocumentFields, DocumentType, Extraction};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

const SERVICE: &str = "isolated-binary-service-key-000000";
const ADMIN: &str = "isolated-binary-admin-key-00000000";
struct Provider {
    url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}
#[derive(Clone)]
struct ProviderState {
    key: String,
    amount: String,
    requests: Arc<Mutex<Vec<Value>>>,
}
async fn completion(
    State(state): State<ProviderState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    assert_eq!(headers["authorization"], format!("Bearer {}", state.key));
    state.requests.lock().unwrap().push(body);
    let extraction = Extraction {
        document_type: DocumentType::Invoice,
        fields: DocumentFields {
            invoice_number: Some("SYNTHETIC-1".into()),
            total_amount: Some(state.amount),
            currency: Some("EUR".into()),
            ..Default::default()
        },
        evidence: vec![],
        warnings: vec![],
        model: "provider-spoofed-attribution".into(),
    };
    Json(
        json!({"choices":[{"finish_reason":"stop","message":{"content":serde_json::to_string(&extraction).unwrap()}}]}),
    )
}
impl Provider {
    async fn start(id: &str, amount: &str) -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = ProviderState {
            key: format!("synthetic-provider-{id}"),
            amount: amount.into(),
            requests: requests.clone(),
        };
        let app = Router::new()
            .route("/v1/chat/completions", post(completion))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            url,
            requests,
            task,
        }
    }
}
struct Process {
    url: String,
    child: tokio::process::Child,
}
impl Process {
    async fn start(profiles: Option<Value>, path: &Path) -> Self {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_ohagent-document-api"));
        command
            .env_clear()
            .env("OHAGENT_API_KEY", SERVICE)
            .env("OHAGENT_DOCUMENT_ADMIN_KEY", ADMIN)
            .env("OHAGENT_DOCUMENT_BIND", address.to_string())
            .env("OHAGENT_DOCUMENT_MODELS_FILE", path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for id in ["a", "b", "c"] {
            command.env(
                format!("SYNTHETIC_PROVIDER_{}", id.to_uppercase()),
                format!("synthetic-provider-{id}"),
            );
        }
        if let Some(profiles) = profiles {
            command.env("OHAGENT_DOCUMENT_PROFILES", profiles.to_string());
        }
        drop(reservation);
        let child = command.spawn().unwrap();
        let mut process = Self {
            url: format!("http://{address}/api/document-processing"),
            child,
        };
        for _ in 0..100 {
            if let Ok(response) = process
                .call(reqwest::Method::GET, "/models", ADMIN)
                .send()
                .await
            {
                assert!(matches!(response.status().as_u16(), 200 | 503));
                return process;
            }
            assert!(
                process.child.try_wait().unwrap().is_none(),
                "isolated API child exited during startup"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("isolated API did not start within two seconds");
    }
    fn call(&self, method: reqwest::Method, path: &str, key: &str) -> reqwest::RequestBuilder {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
            .request(method, format!("{}{path}", self.url))
            .bearer_auth(key)
    }
    async fn policy(&self) -> Value {
        let response = self
            .call(reqwest::Method::GET, "/models", ADMIN)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        response.json().await.unwrap()
    }
    async fn compare(&self) -> reqwest::Response {
        self.call(reqwest::Method::POST, "/compare", SERVICE).header("X-Tenant-Id", "tenant-isolated").json(&json!({"file_name":"scan.png","media_type":"image/png","content_base64":"iVBORw0KGgpmaXh0dXJl"})).send().await.unwrap()
    }
    async fn stop(mut self) {
        self.child.kill().await.unwrap();
    }
}
fn profiles(providers: &[Provider; 3]) -> Value {
    Value::Array(["a", "b", "c"].iter().zip(providers).map(|(id, provider)| json!({
        "id":id,"label":format!("Operator {id}"),"base_url":provider.url,"api_key_env":format!("SYNTHETIC_PROVIDER_{}", id.to_uppercase()),
        "json_mode":if *id == "b" {"json_object"} else {"json_schema"},"pdf_mode":"native","render_image":null,"model":format!("operator-{id}-initial"),"enabled":true
    })).collect())
}

#[tokio::test]
async fn missing_credentials_remain_failed_or_disabled_slots_without_global_outage() {
    let providers = [
        Provider::start("a", "12.30").await,
        Provider::start("b", "12.30").await,
        Provider::start("c", "12.30").await,
    ];
    for enabled in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let mut config = profiles(&providers);
        config[2]["api_key_env"] = "SYNTHETIC_NONEXISTENT_KEY".into();
        config[2]["enabled"] = enabled.into();
        let process = Process::start(Some(config), &directory.path().join("models.json")).await;
        let response = process.compare().await;
        assert_eq!(response.status(), 200);
        let result: Value = response.json().await.unwrap();
        assert_eq!(result["comparison"]["complete"], false);
        assert_eq!(result["comparison"]["results"][0]["status"], "succeeded");
        assert_eq!(result["comparison"]["results"][1]["status"], "succeeded");
        assert_eq!(
            result["comparison"]["results"][2]["status"],
            if enabled { "failed" } else { "disabled" }
        );
        assert_eq!(
            result["comparison"]["results"][2]["error_code"],
            if enabled {
                json!("provider_unavailable")
            } else {
                Value::Null
            }
        );
        let mut policy = process.policy().await;
        policy["models"][2]["enabled"] = false.into();
        let response = process
            .call(reqwest::Method::PUT, "/models", ADMIN)
            .json(&policy)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            200,
            "administrator can disable the unavailable slot"
        );
        assert!(
            providers[2].requests.lock().unwrap().is_empty(),
            "no placeholder credentials or fallback request"
        );
        process.stop().await;
    }
}

#[tokio::test]
async fn real_binary_uses_three_operator_profiles_and_mutates_only_public_registry() {
    let providers = [
        Provider::start("a", "12.30").await,
        Provider::start("b", "12.31").await,
        Provider::start("c", "12.30").await,
    ];
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("models.json");
    let config = profiles(&providers);
    let process = Process::start(Some(config.clone()), &path).await;
    let response = process.compare().await;
    assert_eq!(response.status(), 200);
    let result: Value = response.json().await.unwrap();
    assert_eq!(result["comparison"]["complete"], true);
    assert_eq!(
        result["comparison"]["disagreements"],
        json!(["fields.total_amount"])
    );
    assert_eq!(result["extraction"]["fields"]["total_amount"], "12.30");
    for (index, provider) in providers.iter().enumerate() {
        let requests = provider.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], config[index]["model"]);
        assert_eq!(
            requests[0]["response_format"]["type"],
            if index == 1 {
                "json_object"
            } else {
                "json_schema"
            }
        );
        assert!(requests[0]["messages"][1]["content"][1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
        assert_eq!(
            result["comparison"]["results"][index]["model"],
            config[index]["model"]
        );
    }
    let mut policy = process.policy().await;
    policy["models"][0]["model"] = "operator-a-updated-version".into();
    policy["models"][1]["enabled"] = false.into();
    let updated = process
        .call(reqwest::Method::PUT, "/models", ADMIN)
        .json(&policy)
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), 200);
    assert_eq!(
        process
            .call(reqwest::Method::PUT, "/models", SERVICE)
            .json(&policy)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let result: Value = process.compare().await.json().await.unwrap();
    assert_eq!(result["comparison"]["policy_revision"], 2);
    assert_eq!(result["comparison"]["results"][1]["status"], "disabled");
    assert_eq!(result["comparison"]["complete"], false);
    assert_eq!(
        providers[0].requests.lock().unwrap()[1]["model"],
        "operator-a-updated-version"
    );
    assert_eq!(
        providers[1].requests.lock().unwrap().len(),
        1,
        "disabled provider never called again"
    );
    let stored = std::fs::read_to_string(&path).unwrap();
    for forbidden in [
        "synthetic-provider-",
        "api_key",
        "base_url",
        "content_base64",
        "tenant-isolated",
        ADMIN,
        SERVICE,
    ] {
        assert!(!stored.contains(forbidden));
    }
    process.stop().await;
    let restarted = Process::start(Some(config), &path).await;
    let policy = restarted.policy().await;
    assert_eq!(policy["revision"], 2);
    assert_eq!(policy["models"][0]["model"], "operator-a-updated-version");
    assert_eq!(policy["models"][1]["enabled"], false);
    restarted.stop().await;
}

#[tokio::test]
async fn missing_or_incomplete_profiles_fail_closed_without_registry_or_network() {
    let providers = [
        Provider::start("a", "12.30").await,
        Provider::start("b", "12.30").await,
        Provider::start("c", "12.30").await,
    ];
    let config = profiles(&providers);
    let mut too_few = config.clone();
    too_few.as_array_mut().unwrap().pop();
    let mut unsafe_url = config.clone();
    unsafe_url[0]["base_url"] = "http://example.invalid/v1".into();
    let mut duplicate = config.clone();
    duplicate[1]["id"] = "a".into();
    for profiles in [None, Some(too_few), Some(unsafe_url), Some(duplicate)] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        let process = Process::start(profiles, &path).await;
        assert_eq!(process.compare().await.status(), 503);
        assert_eq!(
            process
                .call(reqwest::Method::GET, "/models", ADMIN)
                .send()
                .await
                .unwrap()
                .status(),
            503
        );
        assert!(
            !path.exists(),
            "invalid boot configuration must not initialize a registry"
        );
        process.stop().await;
    }
    for provider in providers {
        assert!(provider.requests.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn corrupt_reloaded_policy_is_server_unavailable_and_never_silently_repaired() {
    let providers = [
        Provider::start("a", "12.30").await,
        Provider::start("b", "12.30").await,
        Provider::start("c", "12.30").await,
    ];
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("models.json");
    let process = Process::start(Some(profiles(&providers)), &path).await;
    let policy = process.policy().await;
    std::fs::write(&path, "corrupt-policy-fixture").unwrap();
    assert_eq!(process.compare().await.status(), 503);
    assert_eq!(
        process
            .call(reqwest::Method::GET, "/models", ADMIN)
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    assert_eq!(
        process
            .call(reqwest::Method::PUT, "/models", ADMIN)
            .json(&policy)
            .send()
            .await
            .unwrap()
            .status(),
        503
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "corrupt-policy-fixture"
    );
    for provider in &providers {
        assert!(provider.requests.lock().unwrap().is_empty());
    }
    process.stop().await;
}
