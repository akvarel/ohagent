#[tokio::test]
async fn real_client_rejects_incomplete_schema_refusal_tools_and_truncation() {
    let content = serde_json::to_string(&extraction()).unwrap();
    let mut missing = serde_json::to_value(extraction()).unwrap();
    missing["fields"]
        .as_object_mut()
        .unwrap()
        .remove("invoice_number");
    let responses = [
        response(missing.to_string()),
        json!({"choices":[{"finish_reason":"length","message":{"content":content}}]}).to_string(),
        json!({"choices":[{"finish_reason":"stop","message":{"content":content,"refusal":"cannot"}}]}).to_string(),
        json!({"choices":[{"finish_reason":"stop","message":{"content":content,"tool_calls":[]}}]}).to_string(),
        json!({"choices":[]}).to_string(),
        "{".to_string(),
    ];
    for body in responses {
        let (config, handle) = server(200, body, Duration::ZERO).await;
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
async fn real_client_type_mismatch_never_reaches_network() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = VisionConfig {
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        model: "configured-vision".into(),
        api_key: "test-only".into(),
    };
    let mut input = request();
    input.media_type = "application/pdf".into();
    assert!(matches!(
        OpenAiVisionProvider::new(config)
            .unwrap()
            .extract(input)
            .await,
        Err(DocumentError::InvalidInput)
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn real_client_server_owns_model_attribution() {
    let mut output = extraction();
    output.model = "document-injected-model".into();
    let (config, handle) = server(
        200,
        response(serde_json::to_string(&output).unwrap()),
        Duration::ZERO,
    )
    .await;
    let output = OpenAiVisionProvider::new(config)
        .unwrap()
        .extract(request())
        .await
        .unwrap();
    assert_eq!(output.model, "configured-vision");
    handle.await.unwrap();
}

#[tokio::test]
async fn real_client_bounds_chunked_response_without_content_length() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = VisionConfig {
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        model: "configured-vision".into(),
        api_key: "test-only".into(),
    };
    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        // Read the small request in full before responding to avoid a connection reset.
        let mut data = Vec::new();
        loop {
            let mut part = [0; 8192];
            let n = stream.read(&mut part).await.unwrap();
            assert!(n > 0);
            data.extend_from_slice(&part[..n]);
            if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&data[..pos]);
                let length: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse().unwrap())
                    })
                    .unwrap();
                if data.len() >= pos + 4 + length {
                    break;
                }
            }
        }
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let body = "x".repeat(MAX_RESPONSE_BYTES + 1);
        let _ = stream
            .write_all(format!("{:x}\r\n{body}\r\n0\r\n\r\n", body.len()).as_bytes())
            .await;
    });
    assert!(matches!(
        OpenAiVisionProvider::new(config)
            .unwrap()
            .extract(request())
            .await,
        Err(DocumentError::ResponseTooLarge)
    ));
    handle.await.unwrap();
}

#[test]
fn config_rejects_insecure_or_ambiguous_endpoints_and_deadlines() {
    let config = || VisionConfig {
        base_url: "https://vision.example.test/v1".into(),
        model: "configured-vision".into(),
        api_key: "test-only".into(),
    };
    for url in [
        "file:///tmp/x",
        "http://example.test/v1",
        "https://user:password@example.test/v1",
        "https://example.test/v1?x=1",
        "https://example.test/v1#x",
    ] {
        let mut value = config();
        value.base_url = url.into();
        assert!(matches!(
            OpenAiVisionProvider::new(value),
            Err(DocumentError::InvalidConfig)
        ));
    }
    assert!(OpenAiVisionProvider::with_timeout(config(), Duration::ZERO).is_err());
    assert!(OpenAiVisionProvider::with_timeout(config(), Duration::from_secs(61)).is_err());
    let mut value = config();
    value.api_key = "secret\nheader".into();
    assert!(OpenAiVisionProvider::new(value).is_err());
    let mut value = config();
    value.model.clear();
    assert!(OpenAiVisionProvider::new(value).is_err());
}

#[test]
fn exact_input_limit_and_output_collection_bounds() {
    let mut bytes = vec![0; MAX_INPUT_BYTES];
    bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut input = request();
    input.content_base64 = STANDARD.encode(&bytes);
    assert!(validate_request(&input).is_ok());
    bytes.push(0);
    input.content_base64 = STANDARD.encode(&bytes);
    assert!(validate_request(&input).is_err());
    let mut output = extraction();
    output.evidence = vec![output.evidence[0].clone(); 65];
    assert!(validate_extraction(&output).is_err());
    output = extraction();
    output.warnings = vec!["warning".into(); 33];
    assert!(validate_extraction(&output).is_err());
    output = extraction();
    output.fields.currency = Some("eur".into());
    assert!(validate_extraction(&output).is_err());
    output = extraction();
    output.fields.reading_value = Some("NaN".into());
    assert!(validate_extraction(&output).is_err());
}
