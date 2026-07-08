//! Version checker — periodically checks for Jcode updates via GitHub API.
//!
//! Runs as a background task in the ohAgent daemon.
//! Sends push notifications when a new Jcode version is available.

use std::time::Duration;
use tracing::{info, warn};

/// Latest release information from GitHub API.
#[derive(Debug, serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    published_at: Option<String>,
}

/// Check for Jcode updates and notify the user.
pub struct VersionChecker {
    /// Current Jcode version (from git describe, stripped to tag).
    current_version: String,
    /// GitHub API URL for the Jcode releases.
    releases_url: String,
    /// Push service for notification delivery.
    push: Option<std::sync::Arc<crate::push::PushService>>,
    /// Target tenant for notifications.
    tenant_id: String,
    /// Check interval.
    interval: Duration,
}

impl VersionChecker {
    /// Create a new version checker.
    pub fn new(
        current_version: impl Into<String>,
        tenant_id: impl Into<String>,
    ) -> Self {
        Self {
            current_version: current_version.into(),
            releases_url: "https://api.github.com/repos/1jehuang/jcode/releases/latest".into(),
            push: None,
            tenant_id: tenant_id.into(),
            interval: Duration::from_secs(86_400), // 24 hours
        }
    }

    /// Attach a push service for Telegram notifications.
    pub fn with_push(mut self, push: std::sync::Arc<crate::push::PushService>) -> Self {
        self.push = Some(push);
        self
    }

    /// Set a custom check interval (default: 24h).
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Start the background version check loop.
    pub async fn run(self) {
        let client = reqwest::Client::builder()
            .user_agent("ohagent-version-checker/1.0")
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create version checker HTTP client");

        info!(
            current = %self.current_version,
            interval_hours = self.interval.as_secs() / 3600,
            "Version checker started"
        );

        let mut interval = tokio::time::interval(self.interval);
        // Skip the first tick (don't check immediately)
        interval.tick().await;

        loop {
            interval.tick().await;

            match check_latest(&client, &self.releases_url).await {
                Ok(latest) => {
                    // Strip git-suffix from current: "v0.37.0-5-g3cb1287e" → "v0.37.0"
                    let current_tag = self.current_version
                        .split('-')
                        .next()
                        .unwrap_or(&self.current_version);

                    if latest.tag_name != current_tag {
                        info!(
                            current = %self.current_version,
                            latest = %latest.tag_name,
                            "New Jcode version available!"
                        );

                        if let Some(ref push) = self.push {
                            let msg = format!(
                                "🔄 *Jcode update available*\n\n\
                                 Current: `{current_tag}`\n\
                                 Latest: `{latest}`\n\n\
                                 The daemon will notify again in 24h.\n\
                                 Update manually: `cd jcode && git fetch upstream --tags && git rebase --onto {latest} {current_tag} HEAD`",
                                current_tag = current_tag,
                                latest = latest.tag_name,
                            );
                            let _ = push.send(&self.tenant_id, &msg).await;
                        }
                    } else {
                        info!(
                            version = %self.current_version,
                            "Jcode is up to date"
                        );
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Version check failed");
                }
            }
        }
    }
}

/// Fetch the latest release tag from GitHub.
async fn check_latest(
    client: &reqwest::Client,
    url: &str,
) -> Result<GitHubRelease, Box<dyn std::error::Error + Send + Sync>> {
    let resp = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {} from GitHub API", resp.status()).into());
    }

    let release: GitHubRelease = resp.json().await?;
    Ok(release)
}

/// Try to detect the current Jcode version from git.
/// Falls back to CARGO_PKG_VERSION or "unknown".
pub fn detect_version() -> String {
    // Try git describe on the jcode submodule
    if let Ok(output) = std::process::Command::new("git")
        .args(["describe", "--tags", "--always"])
        .current_dir("jcode")
        .output()
    {
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !version.is_empty() {
                return version;
            }
        }
    }

    // Try the submodule pointer
    if let Ok(output) = std::process::Command::new("git")
        .args(["submodule", "status", "jcode"])
        .output()
    {
        if output.status.success() {
            let status = String::from_utf8_lossy(&output.stdout);
            // Format: " a6bbd3f1 jcode (v0.37.0-5-ga6bbd3f1)"
            if let Some(start) = status.find('(') {
                if let Some(end) = status[start..].find(')') {
                    return status[start+1..start+end].to_string();
                }
            }
        }
    }

    // Fallback: package version
    option_env!("CARGO_PKG_VERSION")
        .map(|v| format!("v{v}"))
        .unwrap_or_else(|| "unknown".to_string())
}
