#[cfg(test)]
mod tests {
    use super::*;

    async fn request(headers: &[(&str, &str)], body: serde_json::Value) -> reqwest::Response {
        let state = DocumentApiState::unavailable("document-test-service-key".into());
        let app = router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let mut request = client.post(format!("http://{address}/api/document-processing/extract")).json(&body);
        for (key, value) in headers { request = request.header(*key, *value); }
        let response = request.send().await.unwrap();
        task.abort();
        response
    }

    fn document() -> serde_json::Value {
        serde_json::json!({"file_name":"sample.pdf","media_type":"application/pdf","content_base64":"JVBERi0xLjcK"})
    }

    #[tokio::test]
    async fn rejects_unauthenticated_upload() {
        assert_eq!(request(&[("X-Tenant-Id", "company-a")], document()).await.status(), 401);
    }

    #[tokio::test]
    async fn requires_explicit_trusted_tenant() {
        let response = request(&[("Authorization", "Bearer document-test-service-key")], document()).await;
        assert_eq!(response.status(), 400);
        assert_eq!(response.json::<serde_json::Value>().await.unwrap()["error"]["code"], "invalid_tenant");
    }

    #[tokio::test]
    async fn rejects_bad_document_before_missing_provider() {
        let response = request(&[("Authorization", "Bearer document-test-service-key"), ("X-Tenant-Id", "company-a")],
            serde_json::json!({"file_name":"photo.png","media_type":"image/png","content_base64":"not-base64"})).await;
        assert_eq!(response.status(), 400);
    }

    #[tokio::test]
    async fn missing_provider_fails_closed_without_fake_extraction() {
        let response = request(&[("Authorization", "Bearer document-test-service-key"), ("X-Tenant-Id", "company-a")], document()).await;
        assert_eq!(response.status(), 503);
        assert_eq!(response.json::<serde_json::Value>().await.unwrap()["error"]["code"], "provider_unavailable");
    }
}
