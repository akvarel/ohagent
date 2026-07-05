//! Integration test: JcodeBridge with DeepSeek V4 Flash.
//!
//! Requires DEEPSEEK_API_KEY environment variable.

#[cfg(test)]
mod deepseek_integration {
    use jcode_provider_core::Provider;

    /// End-to-end: create a headless DeepSeek session and send a prompt.
    ///
    /// Set DEEPSEEK_API_KEY before running:
    /// ```bash
    /// export DEEPSEEK_API_KEY=$(vault kv get -field=api_key secret/ohagent/deepseek)
    /// cargo test -p ohagent-core -- deepseek_headless_e2e --nocapture
    /// ```
    #[tokio::test]
    async fn deepseek_headless_e2e() {
        let _ = std::env::var("DEEPSEEK_API_KEY")
            .expect("DEEPSEEK_API_KEY must be set");

        // Build MultiProvider and switch to DeepSeek
        let multi = jcode_base::provider::MultiProvider::new();
        multi
            .set_model("deepseek:deepseek-v4-flash")
            .expect("switch to DeepSeek v4 flash");

        assert_eq!(multi.display_name(), "DeepSeek");
        assert_eq!(multi.model(), "deepseek-v4-flash");

        // Create bridge — MultiProvider IS a Provider
        let provider: std::sync::Arc<dyn Provider> = std::sync::Arc::new(multi);
        let bridge = ohagent_core::jcode_bridge::JcodeBridge::new(provider);

        // Create headless session
        let session = bridge
            .create_session(ohagent_core::jcode_bridge::SessionConfig {
                model: Some("deepseek-v4-flash".into()),
                working_dir: Some(std::env::current_dir().unwrap().to_string_lossy().into()),
                selfdev: false,
                report_back_to: None,
            })
            .await
            .expect("create headless session");

        eprintln!("Session created: {}", session.session_id());

        // Send prompt
        session
            .send_message("Reply with exactly one word: OK")
            .await
            .expect("send_message should succeed");

        eprintln!("✅ DeepSeek headless session works!");
    }
}
