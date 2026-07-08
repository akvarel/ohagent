//! Version checker — periodically checks for Jcode updates via GitHub API.
//!
//! Two-tier strategy:
//!   1. Notification mode (default): check daily, push Telegram message when update found
//!   2. Auto-update mode (OHAGENT_AUTO_UPDATE_JCODE=1): git fetch + rebase submodule,
//!      rebuild ohAgent via cargo, then notify to restart daemon
//!
//! Uses jcode's built-in `jcode-update-core` for version comparison (semver).
//! Does NOT download jcode binaries — ohAgent builds from source via submodule.

use std::time::Duration;
use std::process::Command;
use tracing::{info, warn, error};

/// Latest release information from GitHub API.
#[derive(Debug, serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    published_at: Option<String>,
}

/// Result of a version check.
#[derive(Debug)]
pub enum CheckResult {
    /// Already on latest version.
    UpToDate,
    /// New version available, but auto-update is disabled.
    UpdateAvailable { current: String, latest: String },
    /// Successfully updated submodule. Needs daemon restart.
    Updated { from: String, to: String },
    /// Auto-update failed.
    UpdateFailed { error: String },
}

/// Check for Jcode updates and notify the user.
pub struct VersionChecker {
    /// Current Jcode version (from git describe).
    current_version: String,
    /// GitHub API URL for the Jcode releases.
    releases_url: String,
    /// Push service for notification delivery.
    push: Option<std::sync::Arc<crate::push::PushService>>,
    /// Target tenant for notifications.
    tenant_id: String,
    /// Check interval.
    interval: Duration,
    /// Whether auto-update is enabled (git fetch + rebase + cargo build).
    auto_update: bool,
}

impl VersionChecker {
    /// Create a new version checker.
    pub fn new(
        current_version: impl Into<String>,
        tenant_id: impl Into<String>,
    ) -> Self {
        let auto_update = std::env::var("OHAGENT_AUTO_UPDATE_JCODE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        Self {
            current_version: current_version.into(),
            releases_url: "https://api.github.com/repos/1jehuang/jcode/releases/latest".into(),
            push: None,
            tenant_id: tenant_id.into(),
            interval: Duration::from_secs(86_400), // 24 hours
            auto_update,
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
            auto_update = self.auto_update,
            interval_hours = self.interval.as_secs() / 3600,
            "Version checker started"
        );

        let mut interval = tokio::time::interval(self.interval);
        // Skip first tick — don't check immediately on startup
        interval.tick().await;

        loop {
            interval.tick().await;

            let current_tag = self.current_version
                .split('-')
                .next()
                .unwrap_or(&self.current_version)
                .to_string();

            match check_latest(&client, &self.releases_url).await {
                Ok(latest) => {
                    if latest.tag_name != current_tag {
                        info!(
                            current = %current_tag,
                            latest = %latest.tag_name,
                            "New Jcode version available!"
                        );

                        let result = if self.auto_update {
                            self.try_auto_update(&current_tag, &latest.tag_name)
                        } else {
                            CheckResult::UpdateAvailable {
                                current: current_tag.clone(),
                                latest: latest.tag_name.clone(),
                            }
                        };

                        self.notify(&result, &current_tag, &latest.tag_name).await;
                    } else {
                        info!(version = %current_tag, "Jcode is up to date");
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Version check failed");
                }
            }
        }
    }

    /// Attempt to auto-update the jcode submodule + rebuild ohAgent.
    fn try_auto_update(&self, current: &str, latest: &str) -> CheckResult {
        info!("Auto-updating jcode submodule: {current} → {latest}");

        // Step 1: git fetch upstream tags
        let fetch = Command::new("git")
            .args(["fetch", "upstream", "--tags"])
            .current_dir("jcode")
            .output();

        match fetch {
            Ok(o) if o.status.success() => {
                info!("git fetch upstream: OK");
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr).to_string();
                error!("git fetch failed: {err}");
                return CheckResult::UpdateFailed { error: err };
            }
            Err(e) => {
                return CheckResult::UpdateFailed { error: e.to_string() };
            }
        }

        // Step 2: git rebase onto new tag
        let rebase = Command::new("git")
            .args(["rebase", "--onto", latest, current, "HEAD"])
            .current_dir("jcode")
            .output();

        match rebase {
            Ok(o) if o.status.success() => {
                info!("git rebase onto {latest}: OK");
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr).to_string();
                error!("git rebase failed: {err}");
                // Try to abort
                let _ = Command::new("git")
                    .args(["rebase", "--abort"])
                    .current_dir("jcode")
                    .output();
                return CheckResult::UpdateFailed { error: err };
            }
            Err(e) => {
                return CheckResult::UpdateFailed { error: e.to_string() };
            }
        }

        // Step 3: cargo build --release -p ohagent-daemon
        info!("Rebuilding ohagent-daemon...");
        let build = Command::new("cargo")
            .args(["build", "--release", "-p", "ohagent-daemon"])
            .current_dir("..") // back to ohAgent root
            .output();

        match build {
            Ok(o) if o.status.success() => {
                info!("cargo build: OK");
                CheckResult::Updated {
                    from: current.to_string(),
                    to: latest.to_string(),
                }
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr).to_string();
                error!("cargo build failed: {err}");
                CheckResult::UpdateFailed { error: err }
            }
            Err(e) => {
                CheckResult::UpdateFailed { error: e.to_string() }
            }
        }
    }

    /// Send notification to Telegram.
    async fn notify(&self, result: &CheckResult, current: &str, latest: &str) {
        let push = match &self.push {
            Some(p) => p,
            None => return,
        };

        let msg = match result {
            CheckResult::UpdateAvailable { .. } => {
                format!(
                    "🔄 *Jcode update available*\n\n\
                     Current: `{current}`\n\
                     Latest: `{latest}`\n\n\
                     To update manually:\n\
                     `cd jcode && git fetch upstream --tags && git rebase --onto {latest} {current} HEAD`\n\n\
                     Or enable auto‑update: `export OHAGENT_AUTO_UPDATE_JCODE=1`\n\
                     Then restart the daemon."
                )
            }
            CheckResult::Updated { from, to } => {
                format!(
                    "✅ *Jcode updated!*\n\n\
                     `{from}` → `{to}`\n\n\
                     The daemon was rebuilt automatically.\n\
                     *Restart the daemon to apply:* `systemctl restart ohagent`\n\
                     (auto‑restart coming soon)."
                )
            }
            CheckResult::UpdateFailed { error } => {
                format!(
                    "❌ *Auto‑update failed*\n\n\
                     {current} → {latest}\n\n\
                     Error: `{error}`\n\n\
                     Try manual update:\n\
                     `cd jcode && git fetch upstream --tags && git rebase --onto {latest} {current} HEAD`"
                )
            }
            _ => return,
        };

        let _ = push.send(&self.tenant_id, &msg).await;
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
pub fn detect_version() -> String {
    // Try git describe on the jcode submodule
    if let Ok(output) = Command::new("git")
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
    if let Ok(output) = Command::new("git")
        .args(["submodule", "status", "jcode"])
        .output()
    {
        if output.status.success() {
            let status = String::from_utf8_lossy(&output.stdout);
            if let Some(start) = status.find('(') {
                if let Some(end) = status[start..].find(')') {
                    return status[start+1..start+end].to_string();
                }
            }
        }
    }

    option_env!("CARGO_PKG_VERSION")
        .map(|v| format!("v{v}"))
        .unwrap_or_else(|| "unknown".to_string())
}
