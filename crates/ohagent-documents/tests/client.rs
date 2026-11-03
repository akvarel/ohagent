include!("support/adversarial.rs");

use base64::{engine::general_purpose::STANDARD, Engine};
use ohagent_documents::*;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn request() -> ExtractRequest {
    ExtractRequest {
        file_name: "scan.png".into(),
        media_type: "image/png".into(),
        content_base64: STANDARD.encode(b"\x89PNG\r\n\x1a\nfixture"),
    }
}
fn extraction() -> Extraction {
    Extraction {
        document_type: DocumentType::Invoice,
        fields: DocumentFields {
            currency: Some("EUR".into()),
            total_amount: Some("12.30".into()),
            issue_date: Some("2026-11-03".into()),
            ..Default::default()
        },
        evidence: vec![FieldEvidence {
            field: "total_amount".into(),
            page: 1,
            quote: "12.30 EUR".into(),
        }],
        warnings: vec![],
        model: "configured-vision".into(),
    }
}
fn response(content: String) -> String {
    json!({"choices":[{"finish_reason":"stop","message":{"content":content}}]}).to_string()
}
async fn server(
    status: u16,
    body: String,
    delay: Duration,
) -> (VisionConfig, tokio::task::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut data = Vec::new();
        let header_end;
        loop {
            let mut part = [0u8; 4096];
            let count = stream.read(&mut part).await.unwrap();
            assert!(count > 0);
            data.extend_from_slice(&part[..count]);
            if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                header_end = pos + 4;
                break;
            }
        }
        let headers = String::from_utf8_lossy(&data[..header_end]);
        assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer test-only"));
        let length: usize = headers
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(|s| s.trim().parse().unwrap())
            })
            .unwrap();
        while data.len() < header_end + length {
            let mut part = [0u8; 4096];
            let count = stream.read(&mut part).await.unwrap();
            assert!(count > 0);
            data.extend_from_slice(&part[..count]);
        }
        let payload = serde_json::from_slice(&data[header_end..header_end + length]).unwrap();
        tokio::time::sleep(delay).await;
        let wire = format!("HTTP/1.1 {status} Mock\r\nContent-Length: {}\r\nContent-Type: application/json\r\nLocation: http://127.0.0.1:1/should-not-follow\r\nConnection: close\r\n\r\n{body}", body.len());
        let _ = stream.write_all(wire.as_bytes()).await;
        payload
    });
    (
        VisionConfig {
            base_url: format!("http://{address}/v1"),
            model: "configured-vision".into(),
            api_key: "test-only".into(),
        },
        handle,
    )
}

#[tokio::test]
async fn real_client_sends_image_and_strict_schema() {
    let (config, handle) = server(
        200,
        response(serde_json::to_string(&extraction()).unwrap()),
        Duration::ZERO,
    )
    .await;
    let result = OpenAiVisionProvider::new(config)
        .unwrap()
        .extract(request())
        .await
        .unwrap();
    assert_eq!(result.fields.total_amount.as_deref(), Some("12.30"));
    let payload = handle.await.unwrap();
    assert_eq!(payload["model"], "configured-vision");
    assert!(payload.get("tools").is_none());
    assert_eq!(payload["response_format"]["json_schema"]["strict"], true);
    assert_eq!(
        payload["response_format"]["json_schema"]["schema"]["additionalProperties"],
        false
    );
    assert!(payload["messages"][1]["content"][1]["image_url"]["url"]
        .as_str()
        .unwrap()
        .starts_with("data:image/png;base64,"));
}

#[tokio::test]
async fn real_client_sends_inline_pdf() {
    let (config, handle) = server(
        200,
        response(serde_json::to_string(&extraction()).unwrap()),
        Duration::ZERO,
    )
    .await;
    let mut input = request();
    input.file_name = "invoice.pdf".into();
    input.media_type = "application/pdf".into();
    input.content_base64 = STANDARD.encode(b"%PDF-1.7\nfixture");
    OpenAiVisionProvider::new(config)
        .unwrap()
        .extract(input)
        .await
        .unwrap();
    let payload = handle.await.unwrap();
    assert_eq!(payload["messages"][1]["content"][1]["type"], "file");
    assert!(payload["messages"][1]["content"][1]["file"]["file_data"]
        .as_str()
        .unwrap()
        .starts_with("data:application/pdf;base64,"));
}

#[tokio::test]
async fn json_object_mode_supplies_schema_in_prompt_and_validates_locally() {
    let (config, handle) = server(
        200,
        response(serde_json::to_string(&extraction()).unwrap()),
        Duration::ZERO,
    )
    .await;
    OpenAiVisionProvider::new(config)
        .unwrap()
        .with_json_object_mode()
        .extract(request())
        .await
        .unwrap();
    let payload = handle.await.unwrap();
    assert_eq!(payload["response_format"]["type"], "json_object");
    assert_eq!(payload["max_tokens"], 4096);
    assert!(payload.get("max_completion_tokens").is_none());
    assert!(payload["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("supplier_registration_number"));
    assert!(payload["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("JSON"));
}

#[tokio::test]
async fn real_client_rejects_malformed_unknown_and_invalid_output() {
    let mut unknown = serde_json::to_value(extraction()).unwrap();
    unknown["action"] = json!("pay");
    let mut wrong = serde_json::to_value(extraction()).unwrap();
    wrong["fields"]["total_amount"] = json!(12.3);
    let mut negative = extraction();
    negative.fields.total_amount = Some("-1".into());
    let mut currency = extraction();
    currency.fields.currency = Some("ZZZ".into());
    for content in [
        "not json".into(),
        unknown.to_string(),
        wrong.to_string(),
        serde_json::to_string(&negative).unwrap(),
        serde_json::to_string(&currency).unwrap(),
        format!(
            "```json\n{}\n```",
            serde_json::to_string(&extraction()).unwrap()
        ),
    ] {
        let (config, handle) = server(200, response(content), Duration::ZERO).await;
        assert!(matches!(
            OpenAiVisionProvider::new(config)
                .unwrap()
                .extract(request())
                .await,
            Err(DocumentError::InvalidOutput)
        ));
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn real_client_rejects_http_errors_redirects_and_oversized_response() {
    for status in [302, 401, 429, 500] {
        let (config, handle) =
            server(status, "provider secret detail".into(), Duration::ZERO).await;
        let error = OpenAiVisionProvider::new(config)
            .unwrap()
            .extract(request())
            .await
            .unwrap_err();
        assert!(matches!(error, DocumentError::HttpStatus(s) if s == status));
        assert!(!error.to_string().contains("secret"));
        handle.await.unwrap();
    }
    let (config, handle) = server(200, "x".repeat(MAX_RESPONSE_BYTES + 1), Duration::ZERO).await;
    assert!(matches!(
        OpenAiVisionProvider::new(config)
            .unwrap()
            .extract(request())
            .await,
        Err(DocumentError::ResponseTooLarge)
    ));
    handle.await.unwrap();
}

#[tokio::test]
async fn real_client_timeout() {
    let (config, handle) = server(200, "{}".into(), Duration::from_millis(100)).await;
    let client = OpenAiVisionProvider::with_timeout(config, Duration::from_millis(20)).unwrap();
    assert!(matches!(
        client.extract(request()).await,
        Err(DocumentError::Transport)
    ));
    handle.await.unwrap();
}

#[test]
fn request_validation_bounds_and_magic() {
    assert!(validate_request(&request()).is_ok());
    for (mime, bytes) in [
        ("image/jpeg", b"\xff\xd8\xfffixture".as_slice()),
        ("application/pdf", b"%PDF-1.7\nfixture".as_slice()),
    ] {
        let mut input = request();
        input.media_type = mime.into();
        input.content_base64 = STANDARD.encode(bytes);
        assert!(validate_request(&input).is_ok());
    }
    let mut input = request();
    input.media_type = "image/jpeg".into();
    assert!(validate_request(&input).is_err());
    input = request();
    input.content_base64 = "http://example.test/a.png".into();
    assert!(validate_request(&input).is_err());
    input = request();
    input.file_name = "../scan.png".into();
    assert!(validate_request(&input).is_err());
    input = request();
    input.media_type = "image/svg+xml".into();
    assert!(validate_request(&input).is_err());
    input = request();
    input.content_base64 = STANDARD.encode(vec![0; MAX_INPUT_BYTES + 1]);
    assert!(validate_request(&input).is_err());
}

#[test]
fn output_validation_bounds_semantics_and_unknown_fields() {
    assert!(validate_extraction(&extraction()).is_ok());
    for value in ["-1", "NaN", "inf", "1e2", "1,23", "", " 1", ".2", "1."] {
        let mut output = extraction();
        output.fields.total_amount = Some(value.into());
        assert!(validate_extraction(&output).is_err(), "{value}");
    }
    let mut output = extraction();
    output.fields.reading_date = Some("2026-02-30".into());
    assert!(validate_extraction(&output).is_err());
    output = extraction();
    output.evidence[0].field = "made_up".into();
    assert!(validate_extraction(&output).is_err());
    output = extraction();
    output.evidence[0].page = 0;
    assert!(validate_extraction(&output).is_err());
    output = extraction();
    output.warnings = vec!["w".repeat(2049)];
    assert!(validate_extraction(&output).is_err());
    let mut value = serde_json::to_value(extraction()).unwrap();
    value["fields"]["unknown"] = json!("x");
    assert!(serde_json::from_value::<Extraction>(value).is_err());
}
