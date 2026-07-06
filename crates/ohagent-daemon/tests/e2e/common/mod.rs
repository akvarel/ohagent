//! Common test helpers for E2E tests.
//!
//! Provides daemon lifecycle management and HTTP client helpers.

use std::process::{Child, Command};
use std::time::Duration;
use tokio::time::sleep;

/// The port the test daemon listens on.
pub const TEST_PORT: u16 = 19090;

/// Base URL for the test daemon.
pub fn base_url() -> String {
    format!("http://127.0.0.1:{TEST_PORT}")
}

/// Start the ohagent daemon as a background subprocess for testing.
///
/// Returns the child process handle. The caller should call `stop_daemon()`
/// to clean up.
pub fn start_daemon() -> Child {
    // Build the daemon binary first if needed
    let binary = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/ohagent-daemon");

    // Use cargo run if the binary doesn't exist
    let child = if binary.exists() {
        Command::new(&binary)
            .env("OHAGENT_HEALTH_PORT", TEST_PORT.to_string())
            .env("RUST_LOG", "warn")
            .env("VAULT_TOKEN", "") // No Vault for tests
            .arg("--health-port")
            .arg(TEST_PORT.to_string())
            .arg("--log-level")
            .arg("warn")
            .spawn()
            .expect("Failed to start ohagent-daemon")
    } else {
        // Fall back to cargo run
        Command::new("cargo")
            .args([
                "run",
                "-p",
                "ohagent-daemon",
                "--",
                "--health-port",
                &TEST_PORT.to_string(),
                "--log-level",
                "warn",
            ])
            .env("OHAGENT_HEALTH_PORT", TEST_PORT.to_string())
            .env("RUST_LOG", "warn")
            .env("VAULT_TOKEN", "")
            .spawn()
            .expect("Failed to start ohagent-daemon via cargo run")
    };

    child
}

/// Stop the test daemon.
pub fn stop_daemon(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Wait for the daemon to become healthy (up to 30 seconds).
pub async fn wait_for_healthy() -> bool {
    let client = reqwest::Client::new();
    for i in 0..60 {
        match client
            .get(format!("{}/health", base_url()))
            .timeout(Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => return true,
            _ => {
                if i == 0 {
                    eprintln!("Waiting for daemon to start...");
                }
                sleep(Duration::from_millis(500)).await;
            }
        }
    }
    eprintln!("Daemon did not become healthy within 30 seconds");
    false
}

/// Create a reqwest client for API calls.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap()
}
