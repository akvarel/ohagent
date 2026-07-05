//! ohagent-daemon: 24/7 daemon process for ohAgent.
//!
//! Runs as a long-lived process (systemd service or background).
//! Manages lifecycle, health checks, graceful shutdown,
//! and hosts the messaging gateway.

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::signal;
use tracing::info;
use tracing_subscriber::EnvFilter;

use ohagent_core::jcode_bridge::JcodeBridge;
use ohagent_gateway::platforms::telegram::TelegramAdapter;
use ohagent_gateway::adapter::PlatformAdapter;
use jcode_provider_core::Provider;

/// Register external provider runtimes (OpenRouter, OpenAI-compatible profiles).
///
/// Must be called once at startup before creating any MultiProvider.
fn setup_provider_runtimes() {
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

    tracing::info!("Provider runtimes registered (OpenRouter + compatible profiles)");
}

/// ohAgent — 24/7 personal AI agent built on Jcode.
#[derive(Parser, Debug)]
#[command(name = "ohagent", version, about)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "~/.ohagent/config.toml")]
    config: String,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Health check port
    #[arg(long, default_value = "9090")]
    health_port: u16,

    /// Enable Telegram gateway
    #[arg(long, default_value = "true")]
    telegram: bool,
}

/// Main daemon state.
struct Daemon {
    health_port: u16,
    enable_telegram: bool,
    shutdown: Arc<tokio::sync::Notify>,
    bridge: Arc<JcodeBridge>,
}

impl Daemon {
    fn new(health_port: u16, enable_telegram: bool) -> Result<Self> {
        // Register provider runtimes before creating any provider
        setup_provider_runtimes();

        // Build the provider
        let provider: Arc<dyn Provider> = {
            let multi = jcode_base::provider::MultiProvider::new();

            // Configure from environment / Vault
            if let Ok(api_key) = std::env::var("DEEPSEEK_API_KEY") {
                multi
                    .set_model("deepseek:deepseek-v4-flash")
                    .map_err(|e| anyhow::anyhow!("Failed to set DeepSeek model: {e}"))?;
                info!(
                    provider = %multi.display_name(),
                    "Provider configured via DEEPSEEK_API_KEY"
                );
                let _ = api_key; // Already consumed by MultiProvider via env
            } else if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
                multi
                    .set_model("claude:claude-sonnet-4-6")
                    .map_err(|e| anyhow::anyhow!("Failed to set Claude model: {e}"))?;
                info!(
                    provider = %multi.display_name(),
                    "Provider configured via ANTHROPIC_API_KEY"
                );
                let _ = api_key;
            } else {
                info!("No provider API key found. Using default provider (may need /login).");
            }

            Arc::new(multi)
        };

        let bridge = Arc::new(JcodeBridge::new(provider));

        Ok(Self {
            health_port,
            enable_telegram,
            shutdown: Arc::new(tokio::sync::Notify::new()),
            bridge,
        })
    }

    /// Start the health check HTTP server.
    async fn start_health_server(&self) -> Result<()> {
        let port = self.health_port;
        let shutdown = Arc::clone(&self.shutdown);

        tokio::spawn(async move {
            use axum::{Router, routing::get, Json};
            use serde_json::json;
            use std::net::SocketAddr;

            let app = Router::new().route(
                "/health",
                get(|| async {
                    Json(json!({
                        "status": "ok",
                        "service": "ohagent",
                        "version": env!("CARGO_PKG_VERSION"),
                    }))
                }),
            );

            let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
            info!("Health check server listening on http://{addr}");

            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown.notified().await;
                    info!("Health server shutting down");
                })
                .await
                .unwrap();
        });

        Ok(())
    }

    /// Start the Telegram gateway.
    async fn start_telegram(&self) -> Result<()> {
        let adapter = TelegramAdapter::from_env().map_err(|e| {
            anyhow::anyhow!(
                "Failed to initialize Telegram adapter: {e}. \
                 Set TELEGRAM_BOT_TOKEN or disable with --telegram=false."
            )
        })?;

        info!("Telegram adapter configured, starting bot...");

        let bridge = Arc::clone(&self.bridge);
        tokio::spawn(async move {
            if let Err(e) = adapter.start(bridge).await {
                tracing::error!(error = %e, "Telegram gateway crashed");
            }
        });

        Ok(())
    }

    /// Run the daemon main loop.
    async fn run(self) -> Result<()> {
        info!(
            "ohAgent daemon v{} starting...",
            option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0")
        );

        // Start health check server
        self.start_health_server().await?;

        // Start Telegram gateway (if enabled)
        if self.enable_telegram {
            match self.start_telegram().await {
                Ok(()) => info!("Telegram gateway started"),
                Err(e) => tracing::warn!(error = %e, "Telegram gateway not started"),
            }
        }

        info!("ohAgent daemon ready");

        // Wait for shutdown signal
        self.wait_for_shutdown().await;

        info!("ohAgent daemon stopped");
        Ok(())
    }

    /// Wait for SIGTERM or SIGINT, then trigger graceful shutdown.
    async fn wait_for_shutdown(&self) {
        let shutdown = Arc::clone(&self.shutdown);

        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Received SIGINT (Ctrl+C), initiating shutdown");
            }
            result = async {
                #[cfg(unix)]
                {
                    use tokio::signal::unix::{signal, SignalKind};
                    let mut sigterm = signal(SignalKind::terminate()).unwrap();
                    sigterm.recv().await;
                    info!("Received SIGTERM, initiating shutdown");
                }
            } => {
                let _ = result;
            }
        }

        shutdown.notify_waiters();
    }
}

/// Main entry point for ohAgent daemon.
pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .init();

    let daemon = Daemon::new(cli.health_port, cli.telegram)?;
    daemon.run().await
}

#[tokio::main]
async fn main() -> Result<()> {
    run().await
}
