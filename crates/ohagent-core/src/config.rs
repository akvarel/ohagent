//! ohAgent Configuration — centralized settings with optional TOML config file.
//!
//! Default paths resolve to `~/.ohagent/`. Override any path in
//! `~/.ohagent/config.toml` or specify a different config file via `--config`.
//!
//! ```toml
//! [paths]
//! data_dir = "~/.ohagent"
//! keys_file = "~/.ohagent/keys.toml"
//!
//! [server]
//! health_port = 9090
//! log_level = "info"
//!
//! [telegram]
//! enabled = true
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level ohAgent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OhAgentConfig {
    /// Filesystem paths for data, keys, databases.
    pub paths: PathsConfig,
    /// HTTP server settings.
    pub server: ServerConfig,
    /// Telegram gateway settings.
    pub telegram: TelegramConfig,
    /// API rate limiter settings.
    pub rate_limiter: RateLimiterConfig,
    /// Model routing provider preferences.
    pub providers: ProviderConfig,
}

/// Filesystem paths used by ohAgent at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PathsConfig {
    /// Root data directory (default: ~/.ohagent).
    pub data_dir: String,
    /// TOML file with raw API keys (fallback when Vault unavailable).
    pub keys_file: String,
    /// Working directory for Jcode sessions (bash, git, file ops).
    pub workspace: String,
    /// SQLite database for prompt/response message logging.
    pub message_log_db: String,
    /// SQLite database for per-tenant token usage tracking.
    pub usage_db: String,
    /// SQLite database for persistent session store.
    pub sessions_db: String,
    /// SQLite database for deep memory engine.
    pub memory_db: String,
    /// SQLite database for self-learning skills.
    pub skills_db: String,
    /// TOML plugin pipeline configuration.
    pub plugins_config: String,
    /// Directory for `.so`/`.dylib` plugin libraries.
    pub plugins_dir: String,
    /// TOML file for per-model preference overrides.
    pub model_prefs: String,
    /// JSON file for disabled model ids.
    pub disabled_models: String,
    /// Directory for file uploads from Telegram/WhatsApp.
    pub uploads_dir: String,
}

impl Default for PathsConfig {
    fn default() -> Self {
        let home = "~/.ohagent";
        Self {
            data_dir: home.to_string(),
            keys_file: format!("{home}/keys.toml"),
            workspace: format!("{home}/workspace"),
            message_log_db: format!("{home}/message_log.db"),
            usage_db: format!("{home}/usage.db"),
            sessions_db: format!("{home}/sessions.db"),
            memory_db: format!("{home}/memory.db"),
            skills_db: format!("{home}/skills.db"),
            plugins_config: format!("{home}/plugins.toml"),
            plugins_dir: format!("{home}/plugins"),
            model_prefs: format!("{home}/model_prefs.toml"),
            disabled_models: format!("{home}/disabled_models.json"),
            uploads_dir: format!("{home}/uploads"),
        }
    }
}

/// HTTP server settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Port for health check and REST API (default: 9090).
    pub health_port: u16,
    /// Log level filter (trace, debug, info, warn, error).
    pub log_level: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            health_port: 9090,
            log_level: "info".to_string(),
        }
    }
}

/// Telegram gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    /// Whether to start the Telegram bot on boot.
    pub enabled: bool,
    /// Optional webhook URL (HTTPS). None = long-polling mode.
    pub webhook_url: Option<String>,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            webhook_url: None,
        }
    }
}

/// API rate limiter settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimiterConfig {
    /// Max requests per minute per tenant.
    pub requests_per_minute: u32,
    /// Max burst size.
    pub burst_size: u32,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,
            burst_size: 10,
        }
    }
}

/// Provider routing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// Primary model identifier (e.g. "deepseek:deepseek-v4-flash").
    pub primary_model: String,
    /// Fallback model when primary is unavailable.
    pub fallback_model: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            primary_model: "deepseek:deepseek-v4-flash".to_string(),
            fallback_model: "claude:claude-sonnet-4-6".to_string(),
        }
    }
}

impl Default for OhAgentConfig {
    fn default() -> Self {
        Self {
            paths: PathsConfig::default(),
            server: ServerConfig::default(),
            telegram: TelegramConfig::default(),
            rate_limiter: RateLimiterConfig::default(),
            providers: ProviderConfig::default(),
        }
    }
}

impl OhAgentConfig {
    /// Load configuration from a TOML file.
    ///
    /// If the file doesn't exist, returns default config with a warning.
    /// If parsing fails, returns an error.
    pub fn load(path: &str) -> std::result::Result<Self, ConfigError> {
        let expanded = shellexpand::tilde(path).to_string();
        match std::fs::read_to_string(&expanded) {
            Ok(content) => {
                if content.trim().is_empty() {
                    tracing::debug!(path = %expanded, "Config file is empty — using defaults");
                    return Ok(Self::default());
                }
                toml::from_str(&content).map_err(|e| ConfigError::Parse {
                    path: expanded.clone(),
                    detail: e.to_string(),
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(path = %expanded, "Config file not found — using defaults");
                Ok(Self::default())
            }
            Err(e) => Err(ConfigError::Io {
                path: expanded,
                detail: e.to_string(),
            }),
        }
    }

    /// Resolve a path from the config, expanding `~` to the home directory.
    pub fn resolve_path(&self, raw: &str) -> PathBuf {
        PathBuf::from(shellexpand::tilde(raw).as_ref())
    }
}

/// Config file loading error.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to parse config file {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("Failed to read config file {path}: {detail}")]
    Io { path: String, detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_valid() {
        let cfg = OhAgentConfig::default();
        assert_eq!(cfg.server.health_port, 9090);
        assert!(cfg.paths.keys_file.contains("keys.toml"));
        assert!(cfg.paths.data_dir.contains(".ohagent"));
    }

    #[test]
    fn test_deserialize_toml() {
        let toml_str = r#"
[server]
health_port = 8080
log_level = "debug"

[telegram]
enabled = false
"#;
        let cfg: OhAgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.server.health_port, 8080);
        assert_eq!(cfg.server.log_level, "debug");
        assert!(!cfg.telegram.enabled);
        // Paths should use defaults
        assert!(cfg.paths.data_dir.contains(".ohagent"));
    }

    #[test]
    fn test_load_nonexistent_file_returns_default() {
        let cfg = OhAgentConfig::load("/nonexistent/path/config.toml").unwrap();
        assert_eq!(cfg.server.health_port, 9090);
    }

    #[test]
    fn test_invalid_toml_returns_error() {
        let tmp = std::env::temp_dir().join("ohagent_test_bad.toml");
        std::fs::write(&tmp, "[[[invalid toml").unwrap();
        let err = OhAgentConfig::load(tmp.to_str().unwrap()).unwrap_err();
        assert!(matches!(err, ConfigError::Parse { .. }));
        let _ = std::fs::remove_file(&tmp);
    }
}
