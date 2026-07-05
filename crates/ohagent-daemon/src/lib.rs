//! ohagent-daemon: 24/7 daemon process for ohAgent.
//!
//! Runs as a long-lived process (systemd service or background).
//! Manages lifecycle, health checks, and graceful shutdown.

use tracing::info;

/// Start the ohAgent daemon.
///
/// This is the main entry point that:
/// 1. Loads configuration from Vault + config files
/// 2. Starts the gateway (Telegram, etc.)
/// 3. Starts the cron scheduler
/// 4. Runs the main event loop
pub async fn run() -> anyhow::Result<()> {
    info!("ohAgent daemon starting...");
    // TODO: Implement daemon lifecycle
    Ok(())
}
