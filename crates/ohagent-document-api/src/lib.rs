//! Restricted document-processing API. Only trusted application backends may call it.
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use ohagent_documents::{
    validate_extraction, validate_request, DocumentError, ExtractRequest, OpenAiVisionProvider,
    VisionConfig, VisionProvider,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct DocumentApiState {
    provider: Option<Arc<dyn VisionProvider>>,
    api_key: Arc<String>,
    permits: Arc<Semaphore>,
}

impl DocumentApiState {
    pub fn unavailable(api_key: String) -> Self {
        Self {
            provider: None,
            api_key: Arc::new(api_key),
            permits: Arc::new(Semaphore::new(4)),
        }
    }

    pub fn with_provider(api_key: String, provider: Arc<dyn VisionProvider>) -> Self {
        Self {
            provider: Some(provider),
            ..Self::unavailable(api_key)
        }
    }

    /// Configuration is process-owned, never supplied in an upload request.
    pub fn from_env(api_key: String) -> Self {
        let config = (|| {
            Some(VisionConfig {
                base_url: std::env::var("OHAGENT_DOCUMENT_BASE_URL").ok()?,
                model: std::env::var("OHAGENT_DOCUMENT_MODEL").ok()?,
                api_key: std::env::var("OHAGENT_DOCUMENT_API_KEY").ok()?,
            })
        })();
        match config.and_then(|c| OpenAiVisionProvider::new(c).ok()) {
            Some(provider) => Self::with_provider(api_key, Arc::new(provider)),
            None => Self::unavailable(api_key),
        }
    }
}

pub fn router(state: DocumentApiState) -> Router {
    Router::new()
        .route("/api/document-processing/extract", post(extract))
        .layer(DefaultBodyLimit::max(15 * 1024 * 1024))
        .with_state(state)
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(serde_json::json!({"error": {"code": code, "message_key": format!("documents.{code}")}}))).into_response()
}

fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    let actual = headers
        .get("Authorization")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .or_else(|| headers.get("X-API-Key").and_then(|s| s.to_str().ok()));
    let Some(actual) = actual else {
        return false;
    };
    !expected.is_empty()
        && actual.len() == expected.len()
        && actual
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

async fn extract(
    State(state): State<DocumentApiState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // Authenticate before deserializing documents. No chat/session state or document logging.
    if !authorized(&headers, &state.api_key) {
        return error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let tenant = headers.get("X-Tenant-Id").and_then(|s| s.to_str().ok());
    if !tenant.is_some_and(|t| {
        !t.is_empty()
            && t.len() <= 128
            && t.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
    }) {
        return error(StatusCode::BAD_REQUEST, "invalid_tenant");
    }
    let request: ExtractRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_document"),
    };
    if validate_request(&request).is_err() {
        return error(StatusCode::BAD_REQUEST, "invalid_document");
    }
    let Some(provider) = &state.provider else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "provider_unavailable");
    };
    let Ok(_permit) = state.permits.try_acquire() else {
        return error(StatusCode::TOO_MANY_REQUESTS, "capacity_exceeded");
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(65),
        provider.extract(request),
    )
    .await;
    match result {
        Ok(Ok(extraction)) if validate_extraction(&extraction).is_ok() => {
            Json(extraction).into_response()
        }
        Ok(Ok(_)) | Ok(Err(DocumentError::InvalidOutput)) => {
            error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_extraction")
        }
        Ok(Err(DocumentError::InvalidInput)) => error(StatusCode::BAD_REQUEST, "invalid_document"),
        _ => error(StatusCode::BAD_GATEWAY, "provider_failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn request(headers: &[(&str, &str)], body: serde_json::Value) -> reqwest::Response {
        let state = DocumentApiState::unavailable("document-test-service-key".into());
        request_with_state(state, headers, body).await
    }

    async fn request_with_state(
        state: DocumentApiState,
        headers: &[(&str, &str)],
        body: serde_json::Value,
    ) -> reqwest::Response {
        let app = router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let mut request = client
            .post(format!("http://{address}/api/document-processing/extract"))
            .json(&body);
        for (key, value) in headers {
            request = request.header(*key, *value);
        }
        let response = request.send().await.unwrap();
        task.abort();
        response
    }

    fn document() -> serde_json::Value {
        serde_json::json!({"file_name":"sample.pdf","media_type":"application/pdf","content_base64":"JVBERi0xLjcK"})
    }

    struct FixtureProvider {
        invalid: bool,
    }

    #[async_trait::async_trait]
    impl VisionProvider for FixtureProvider {
        async fn extract(
            &self,
            _: ExtractRequest,
        ) -> Result<ohagent_documents::Extraction, DocumentError> {
            if self.invalid {
                return Err(DocumentError::InvalidOutput);
            }
            Ok(serde_json::from_value(serde_json::json!({
                "document_type":"invoice", "fields": {"invoice_number":"TEST-001", "issue_date":"2026-11-03", "currency":"EUR", "total_amount":"12.50", "supplier_name":"Artificial Supplier"},
                "evidence":[{"field":"total_amount", "page":1,"quote":"12.50 EUR"}],"warnings":[],"model":"fixture-vision"
            })).unwrap())
        }
    }

    #[tokio::test]
    async fn returns_typed_extraction_without_session_state() {
        let state = DocumentApiState::with_provider(
            "document-test-service-key".into(),
            Arc::new(FixtureProvider { invalid: false }),
        );
        let response = request_with_state(
            state,
            &[
                ("Authorization", "Bearer document-test-service-key"),
                ("X-Tenant-Id", "company-a"),
            ],
            document(),
        )
        .await;
        assert_eq!(response.status(), 200);
        let body = response.json::<serde_json::Value>().await.unwrap();
        assert_eq!(body["document_type"], "invoice");
        assert_eq!(body["fields"]["total_amount"], "12.50");
        assert!(body.get("tenant_id").is_none());
    }

    #[tokio::test]
    async fn invalid_model_output_never_becomes_success() {
        let state = DocumentApiState::with_provider(
            "document-test-service-key".into(),
            Arc::new(FixtureProvider { invalid: true }),
        );
        let response = request_with_state(
            state,
            &[
                ("Authorization", "Bearer document-test-service-key"),
                ("X-Tenant-Id", "company-a"),
            ],
            document(),
        )
        .await;
        assert_eq!(response.status(), 422);
    }

    #[tokio::test]
    async fn capacity_limit_fails_without_queueing_unbounded_requests() {
        let state = DocumentApiState::with_provider(
            "document-test-service-key".into(),
            Arc::new(FixtureProvider { invalid: false }),
        );
        let permits = state.permits.clone();
        let _held = permits.acquire_many(4).await.unwrap();
        let response = request_with_state(
            state,
            &[
                ("Authorization", "Bearer document-test-service-key"),
                ("X-Tenant-Id", "company-a"),
            ],
            document(),
        )
        .await;
        assert_eq!(response.status(), 429);
    }

    #[tokio::test]
    async fn rejects_tenant_path_injection() {
        let response = request(
            &[
                ("Authorization", "Bearer document-test-service-key"),
                ("X-Tenant-Id", "../company-b"),
            ],
            document(),
        )
        .await;
        assert_eq!(response.status(), 400);
    }

    #[tokio::test]
    async fn rejects_unauthenticated_upload() {
        assert_eq!(
            request(&[("X-Tenant-Id", "company-a")], document())
                .await
                .status(),
            401
        );
    }

    #[tokio::test]
    async fn requires_explicit_trusted_tenant() {
        let response = request(
            &[("Authorization", "Bearer document-test-service-key")],
            document(),
        )
        .await;
        assert_eq!(response.status(), 400);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
            "invalid_tenant"
        );
    }

    #[tokio::test]
    async fn rejects_bad_document_before_missing_provider() {
        let response = request(&[("Authorization", "Bearer document-test-service-key"), ("X-Tenant-Id", "company-a")],
            serde_json::json!({"file_name":"photo.png","media_type":"image/png","content_base64":"not-base64"})).await;
        assert_eq!(response.status(), 400);
    }

    #[tokio::test]
    async fn missing_provider_fails_closed_without_fake_extraction() {
        let response = request(
            &[
                ("Authorization", "Bearer document-test-service-key"),
                ("X-Tenant-Id", "company-a"),
            ],
            document(),
        )
        .await;
        assert_eq!(response.status(), 503);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
            "provider_unavailable"
        );
    }
}
