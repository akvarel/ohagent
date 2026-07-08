//! Version checker — periodically checks for Jcode updates via GitHub API.
//!
//! Two-tier strategy:
//!   1. Notification mode (default): daily check, push Telegram with [OK] [Update] buttons
//!   2. Auto-update mode (OHAGENT_AUTO_UPDATE_JCODE=1): git fetch + rebase → rebuild → exec restart

use std::time::Duration;
use std::process::Command;
use tracing::{info, warn, error};

/// Notifier trait for sending interactive update messages.
pub trait UpdateNotifier: Send + Sync {
    fn send_update_msg(&self, current: &str, latest: &str, message: &str) -> Result<String, String>;
    fn edit_update_msg(&self, msg_id: &str, text: &str);
}

#[derive(Debug, serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[allow(dead_code)] name: Option<String>,
    #[allow(dead_code)] published_at: Option<String>,
}

#[derive(Debug)]
pub enum CheckResult {
    UpToDate,
    UpdateAvailable { current: String, latest: String },
    Updated { from: String, to: String },
    UpdateFailed { error: String },
}

pub struct VersionChecker {
    current_version: String,
    releases_url: String,
    push: Option<std::sync::Arc<crate::push::PushService>>,
    notifier: Option<std::sync::Arc<dyn UpdateNotifier>>,
    tenant_id: String,
    interval: Duration,
    auto_update: bool,
}

impl VersionChecker {
    pub fn new(current_version: impl Into<String>, tenant_id: impl Into<String>) -> Self {
        let auto_update = std::env::var("OHAGENT_AUTO_UPDATE_JCODE")
            .map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false);
        Self {
            current_version: current_version.into(),
            releases_url: "https://api.github.com/repos/1jehuang/jcode/releases/latest".into(),
            push: None, notifier: None,
            tenant_id: tenant_id.into(),
            interval: Duration::from_secs(86_400),
            auto_update,
        }
    }

    pub fn with_push(mut self, push: std::sync::Arc<crate::push::PushService>) -> Self {
        self.push = Some(push); self
    }

    pub fn with_notifier(mut self, n: std::sync::Arc<dyn UpdateNotifier>) -> Self {
        self.notifier = Some(n); self
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval; self
    }

    pub async fn run(self) {
        let client = reqwest::Client::builder()
            .user_agent("ohagent-version-checker/1.0")
            .timeout(Duration::from_secs(10)).build().unwrap();

        info!(current=%self.current_version, auto_update=self.auto_update, "Version checker started");
        let mut tick = tokio::time::interval(self.interval);
        tick.tick().await;

        loop {
            tick.tick().await;
            let current_tag = self.current_version.split('-').next().unwrap_or(&self.current_version).to_string();

            match check_latest(&client, &self.releases_url).await {
                Ok(latest) if latest.tag_name != current_tag => {
                    info!(current=%current_tag, latest=%latest.tag_name, "New Jcode version available!");

                    // Show interactive notification
                    if let Some(ref n) = self.notifier {
                        let msg = format!(
                            "🔄 *Jcode update available*\n\nCurrent: `{current_tag}`\nLatest: `{latest}`\n\nAuto‑update will: git fetch + rebase + cargo build + restart",
                            current_tag = current_tag, latest = latest.tag_name
                        );
                        let _ = n.send_update_msg(&current_tag, &latest.tag_name, &msg);
                    } else if let Some(ref p) = self.push {
                        let _ = p.send(&self.tenant_id, &format!("🔄 Jcode {current_tag} → {latest}", latest=latest.tag_name)).await;
                    }

                    // Auto-update if enabled
                    if self.auto_update {
                        let c = current_tag.clone(); let l = latest.tag_name.clone();
                        tokio::spawn(async move { auto_update(&c, &l); });
                    }
                }
                Ok(_) => info!(version=%current_tag, "Jcode is up to date"),
                Err(e) => warn!(error=%e, "Version check failed"),
            }
        }
    }
}

async fn auto_update(current: &str, latest: &str) {
    info!("Auto-updating: {current} → {latest}");
    let _ = Command::new("git").args(["fetch", "upstream", "--tags"]).current_dir("jcode").output();
    let _ = Command::new("git").args(["rebase", "--onto", latest, current, "HEAD"]).current_dir("jcode").output();
    info!("Rebuilding ohagent-daemon...");
    let _ = Command::new("cargo").args(["build", "--release", "-p", "ohagent-daemon"]).current_dir("..").output();
    info!("Restarting daemon...");
    std::thread::sleep(Duration::from_secs(2));
    exec_restart();
}

fn exec_restart() -> ! {
    let exe = std::env::current_exe().unwrap_or_else(|_| "ohagent-daemon".into());
    let args: Vec<String> = std::env::args().collect();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let filtered: Vec<String> = args.iter().skip(1).filter(|a| *a != "--no-update").cloned().collect();
        let mut cmd = std::process::Command::new(&exe);
        cmd.args(&filtered);
        let err = cmd.exec();
        error!("exec failed: {err}");
    }
    error!("exec restart not supported on this platform — exiting");
    std::process::exit(1);
}

async fn check_latest(client: &reqwest::Client, url: &str) -> Result<GitHubRelease, Box<dyn std::error::Error + Send + Sync>> {
    let resp = client.get(url).header("Accept", "application/vnd.github+json").header("X-GitHub-Api-Version", "2022-11-28").send().await?;
    if !resp.status().is_success() { return Err(format!("HTTP {}", resp.status()).into()); }
    Ok(resp.json().await?)
}

pub fn detect_version() -> String {
    if let Ok(o) = Command::new("git").args(["describe", "--tags", "--always"]).current_dir("jcode").output() {
        if o.status.success() {
            let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !v.is_empty() { return v; }
        }
    }
    option_env!("CARGO_PKG_VERSION").map(|v| format!("v{v}")).unwrap_or_else(|| "unknown".into())
}
