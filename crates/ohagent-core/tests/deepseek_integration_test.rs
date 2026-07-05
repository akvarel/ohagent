//! Integration test: JcodeBridge with DeepSeek V4 Flash.
//!
//! Requires DEEPSEEK_API_KEY environment variable.

#[cfg(test)]
mod deepseek_integration {
    use jcode_provider_core::Provider;

    fn register_runtimes() {
        use jcode_base::provider::external;
        use jcode_provider_openrouter_runtime::OpenRouterProvider;

        external::register_openrouter_factory(|spec| {
            use external::OpenRouterRuntimeSpec;
            let provider: std::sync::Arc<dyn Provider> = match spec {
                OpenRouterRuntimeSpec::Default => std::sync::Arc::new(OpenRouterProvider::new()?),
                OpenRouterRuntimeSpec::OpenRouterApiKey => {
                    std::sync::Arc::new(OpenRouterProvider::new_openrouter_api_key_runtime()?)
                }
                OpenRouterRuntimeSpec::CompatibleProfile(profile) => std::sync::Arc::new(
                    OpenRouterProvider::new_openai_compatible_profile_runtime(profile)?,
                ),
                OpenRouterRuntimeSpec::NamedProfile { name, config } => std::sync::Arc::new(
                    OpenRouterProvider::new_named_openai_compatible(&name, &config)?,
                ),
            };
            Ok(provider)
        });

        external::register_profile_catalog_refresh(
            jcode_provider_openrouter_runtime::maybe_schedule_openai_compatible_profile_catalog_refresh,
        );
        external::register_standard_openrouter_catalog_refresh(
            jcode_provider_openrouter_runtime::maybe_schedule_standard_openrouter_catalog_refresh,
        );
    }

    /// End-to-end: create a headless DeepSeek session and send a prompt.
    ///
    /// Set DEEPSEEK_API_KEY before running:
    /// ```bash
    /// export DEEPSEEK_API_KEY="sk-..."
    /// cargo test -p ohagent-core -- deepseek_headless_e2e --nocapture
    /// ```
    #[tokio::test]
    async fn deepseek_headless_e2e() {
        let _ = std::env::var("DEEPSEEK_API_KEY")
            .expect("DEEPSEEK_API_KEY must be set");

        register_runtimes();

        // Build MultiProvider and switch to DeepSeek
        let multi = jcode_base::provider::MultiProvider::new();
        multi
            .set_model("deepseek:deepseek-v4-flash")
            .expect("switch to DeepSeek v4 flash");

        assert_eq!(multi.display_name(), "DeepSeek");

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
