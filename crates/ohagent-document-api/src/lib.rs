//! Restricted document-processing API. Only trusted application backends may call it.
use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use ohagent_documents::{
    validate_extraction, validate_request, ComparisonService, DocumentError, ExtractRequest,
    ModelRegistry, OpenAiVisionProvider, RegistryError, VisionConfig, VisionProvider,
    MAX_COMPARISON_BYTES,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct DocumentApiState {
    provider: Option<Arc<dyn VisionProvider>>,
    api_key: Arc<String>,
    permits: Arc<Semaphore>,
    comparison: Option<Arc<ComparisonService>>,
    admin_key: Option<Arc<String>>,
}

impl DocumentApiState {
    pub fn unavailable(api_key: String) -> Self {
        Self {
            provider: None,
            api_key: Arc::new(api_key),
            permits: Arc::new(Semaphore::new(4)),
            comparison: None,
            admin_key: None,
        }
    }

    pub fn with_provider(api_key: String, provider: Arc<dyn VisionProvider>) -> Self {
        Self {
            provider: Some(provider),
            ..Self::unavailable(api_key)
        }
    }

    /// Inject a boot-owned comparison service, with an independent administrator key.
    /// Equal service/admin keys are rejected, never granting service callers admin access.
    pub fn with_comparison(
        mut self,
        service: Arc<ComparisonService>,
        admin_key: Option<String>,
    ) -> Self {
        self.comparison = Some(service);
        self.admin_key = admin_key
            .filter(|key| key.trim().len() >= 24 && key != self.api_key.as_str())
            .map(Arc::new);
        self
    }

    /// Configuration is process-owned, never supplied in an upload request.
    pub fn from_env(api_key: String) -> Self {
        let mut state = Self::legacy_from_env(api_key);
        state.admin_key = std::env::var("OHAGENT_DOCUMENT_ADMIN_KEY")
            .ok()
            .filter(|key| key.trim().len() >= 24 && key != state.api_key.as_str())
            .map(Arc::new);
        if let Ok(service) = ComparisonService::from_env() {
            state.comparison = Some(Arc::new(service));
        }
        state
    }

    fn legacy_from_env(api_key: String) -> Self {
        let config = (|| {
            Some(VisionConfig {
                base_url: std::env::var("OHAGENT_DOCUMENT_BASE_URL").ok()?,
                model: std::env::var("OHAGENT_DOCUMENT_MODEL").ok()?,
                api_key: std::env::var("OHAGENT_DOCUMENT_API_KEY").ok()?,
            })
        })();
        match config.and_then(|c| OpenAiVisionProvider::new(c).ok()) {
            Some(provider) => {
                let provider = match std::env::var("OHAGENT_DOCUMENT_JSON_MODE").as_deref() {
                    Ok("json_object") => provider.with_json_object_mode(),
                    Ok("json_schema") | Err(_) => provider,
                    _ => return Self::unavailable(api_key),
                };
                let provider = match std::env::var("OHAGENT_DOCUMENT_PDF_MODE").as_deref() {
                    Ok("podman") => {
                        let image = match std::env::var("OHAGENT_DOCUMENT_RENDER_IMAGE") {
                            Ok(image) => image,
                            Err(_) => return Self::unavailable(api_key),
                        };
                        match ohagent_documents::PdfRasterizer::new(image) {
                            Ok(renderer) => provider.with_pdf_rasterizer(renderer),
                            Err(_) => return Self::unavailable(api_key),
                        }
                    }
                    Ok("native") | Err(_) => provider,
                    _ => return Self::unavailable(api_key),
                };
                Self::with_provider(api_key, Arc::new(provider))
            }
            None => Self::unavailable(api_key),
        }
    }
}

pub fn router(state: DocumentApiState) -> Router {
    Router::new()
        .route("/api/document-processing/extract", post(extract))
        .route("/api/document-processing/compare", post(compare))
        .route(
            "/api/document-processing/models",
            get(models)
                .put(update_models)
                .layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .layer(DefaultBodyLimit::max(15 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), admit))
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

async fn admit(State(state): State<DocumentApiState>, request: Request, next: Next) -> Response {
    // Reject untrusted or excess requests before buffering any document bytes.
    let headers = request.headers();
    let is_admin = request.uri().path() == "/api/document-processing/models";
    let expected = if is_admin {
        state.admin_key.as_deref().map(String::as_str)
    } else {
        Some(state.api_key.as_str())
    };
    if !expected.is_some_and(|key| authorized(headers, key)) {
        return error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let tenant = headers.get("X-Tenant-Id").and_then(|s| s.to_str().ok());
    if !is_admin
        && !tenant.is_some_and(|t| {
            !t.is_empty()
                && t.len() <= 128
                && t.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
        })
    {
        return error(StatusCode::BAD_REQUEST, "invalid_tenant");
    }
    let Ok(_permit) = state.permits.try_acquire() else {
        return error(StatusCode::TOO_MANY_REQUESTS, "capacity_exceeded");
    };
    match tokio::time::timeout(std::time::Duration::from_secs(100), next.run(request)).await {
        Ok(response) => response,
        Err(_) => error(StatusCode::REQUEST_TIMEOUT, "request_timeout"),
    }
}

async fn extract(State(state): State<DocumentApiState>, body: axum::body::Bytes) -> Response {
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
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(95),
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

fn registry_error(value: RegistryError) -> Response {
    match value {
        RegistryError::Invalid => error(StatusCode::BAD_REQUEST, "invalid_model_registry"),
        RegistryError::Unavailable => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "model_registry_unavailable",
        ),
        RegistryError::Conflict => error(StatusCode::CONFLICT, "model_registry_conflict"),
        RegistryError::AllDisabled => error(StatusCode::SERVICE_UNAVAILABLE, "all_models_disabled"),
    }
}

async fn snapshot(service: Arc<ComparisonService>) -> Result<ModelRegistry, RegistryError> {
    tokio::task::spawn_blocking(move || service.snapshot())
        .await
        .map_err(|_| RegistryError::Unavailable)?
}

async fn models(State(state): State<DocumentApiState>) -> Response {
    let Some(service) = state.comparison else {
        return registry_error(RegistryError::Unavailable);
    };
    match snapshot(service).await {
        Ok(registry) => Json(registry).into_response(),
        Err(_) => registry_error(RegistryError::Unavailable),
    }
}

async fn update_models(
    State(state): State<DocumentApiState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    let body = match body {
        Ok(body) => body,
        Err(rejection) => return error(rejection.status(), "model_registry_too_large"),
    };
    if body.len() > 16 * 1024 {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "model_registry_too_large");
    }
    let proposed: ModelRegistry = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return registry_error(RegistryError::Invalid),
    };
    let Some(service) = state.comparison else {
        return registry_error(RegistryError::Unavailable);
    };
    match tokio::task::spawn_blocking(move || service.update(proposed)).await {
        Ok(Ok(registry)) => Json(registry).into_response(),
        Ok(Err(error)) => registry_error(error),
        Err(_) => registry_error(RegistryError::Unavailable),
    }
}

async fn compare(
    State(state): State<DocumentApiState>,
    body: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    let body = match body {
        Ok(body) => body,
        Err(rejection) => return error(rejection.status(), "invalid_document"),
    };
    let request: ExtractRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_document"),
    };
    if validate_request(&request).is_err() {
        return error(StatusCode::BAD_REQUEST, "invalid_document");
    }
    let Some(service) = state.comparison else {
        return registry_error(RegistryError::Unavailable);
    };
    let registry = match snapshot(service.clone()).await {
        Ok(registry) => registry,
        // An invalid persisted policy is a server configuration failure, not bad document input.
        Err(_) => return registry_error(RegistryError::Unavailable),
    };
    match service.compare(registry, request).await {
        Ok(result) => match serde_json::to_vec(&result) {
            Ok(bytes) if bytes.len() <= MAX_COMPARISON_BYTES => (
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                bytes,
            )
                .into_response(),
            _ => error(StatusCode::BAD_GATEWAY, "comparison_too_large"),
        },
        Err(error) => registry_error(error),
    }
}

#[cfg(test)]
mod comparison_tests;

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

    async fn incomplete_body_status(state: DocumentApiState, authenticated: bool) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let auth = if authenticated {
            "Authorization: Bearer document-test-service-key\r\nX-Tenant-Id: company-a\r\n"
        } else {
            ""
        };
        stream.write_all(format!("POST /api/document-processing/extract HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Type: application/json\r\nContent-Length: 10000000\r\n\r\n{{").as_bytes()).await.unwrap();
        let mut buffer = [0u8; 512];
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(1), stream.read(&mut buffer)).await;
        task.abort();
        let size = result
            .expect("request headers must be rejected before reading body")
            .unwrap();
        std::str::from_utf8(&buffer[..size])
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap()
    }

    #[tokio::test]
    async fn unauthorized_incomplete_body_is_rejected_immediately() {
        assert_eq!(
            incomplete_body_status(
                DocumentApiState::unavailable("document-test-service-key".into()),
                false
            )
            .await,
            401
        );
    }

    #[tokio::test]
    async fn saturated_incomplete_body_is_rejected_immediately() {
        let state = DocumentApiState::unavailable("document-test-service-key".into());
        let permits = state.permits.clone();
        let _held = permits.acquire_many(4).await.unwrap();
        assert_eq!(incomplete_body_status(state, true).await, 429);
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
