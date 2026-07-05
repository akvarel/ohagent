//! ohagent-daemon: 24/7 daemon process for ohAgent.
//!
//! Runs as a long-lived process (systemd service or background).
//! Manages lifecycle, health checks, and graceful shutdown.

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::signal;
use tracing::info;
use tracing_subscriber::EnvFilter;

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
}

/// Main daemon state.
struct Daemon {
    health_port: u16,
    shutdown: Arc<tokio::sync::Notify>,
}

impl Daemon {
    fn new(health_port: u16) -> Self {
        Self {
            health_port,
            shutdown: Arc::new(tokio::sync::Notify::new()),
        }
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

    /// Run the daemon main loop.
    async fn run(self) -> Result<()> {
        info!(
            "ohAgent daemon v{} starting...",
            option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0")
        );

        // Start health check server
        self.start_health_server().await?;

        // TODO: Initialize JcodeBridge with configured provider
        // TODO: Start Gateway (Telegram, Discord, ...)
        // TODO: Start Cron Scheduler

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

    let daemon = Daemon::new(cli.health_port);
    daemon.run().await
}

#[tokio::main]
async fn main() -> Result<()> {
    run().await
}
