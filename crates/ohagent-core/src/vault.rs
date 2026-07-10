//! HashiCorp Vault client — shared secrets resolution for all ohAgent services.
//!
//! Follows the same pattern as `shared/vault/vault.py` and `shared/vault/vault.go`.
//!
//! Resolution priority:
//! 1. Vault (if VAULT_TOKEN is set)
//! 2. Environment variables
//! 3. keys.toml on disk
//!
//! # Usage
//!
//! ```ignore
//! let vault = VaultClient::from_env();
//! if vault.available() {
//!     let key = vault.read("providers/deepseek/api-key").await?;
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

// ── Configuration ──

/// Default Vault address (local dev).
const DEFAULT_VAULT_ADDR: &str = "http://localhost:8200";
/// Default KV secrets engine mount path.
const DEFAULT_KV_PATH: &str = "kv";

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Vault errors.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("Vault HTTP {0}: {1}")]
    Http(u16, String),
    #[error("Vault unreachable: {0}")]
    Unreachable(String),
    #[error("Vault auth failed: {0}")]
    Auth(String),
    #[error("Vault request failed: {0}")]
    Request(String),
    #[error("Vault JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Vault network error: {0}")]
    Network(#[from] reqwest::Error),
}

/// Authentication method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    /// Static token from VAULT_TOKEN env var
    Token,
    /// Kubernetes service account JWT
    Kubernetes { role: String },
    /// AppRole with role_id + secret_id
    AppRole { role_id: String, secret_id: String },
    /// Token file (e.g., /vault/token in sidecar)
    TokenFile { path: String },
}

// ── Client ──

/// Minimal HashiCorp Vault KV v2 client.
///
/// No heavy SDK dependency — uses `reqwest` (already in ohagent-core).
#[derive(Clone)]
pub struct VaultClient {
    addr: String,
    token: Option<String>,
    kv_path: String,
    http: reqwest::Client,
    available: bool,
}

impl VaultClient {
    /// Create from environment: VAULT_ADDR, VAULT_TOKEN, VAULT_KV_PATH.
    pub fn from_env() -> Self {
        let addr = env_var("VAULT_ADDR").unwrap_or_else(|| DEFAULT_VAULT_ADDR.to_string());
        let token = env_var("VAULT_TOKEN");
        let kv_path = env_var("VAULT_KV_PATH").unwrap_or_else(|| DEFAULT_KV_PATH.to_string());
        let available = token.is_some();

        Self {
            addr: addr.trim_end_matches('/').to_string(),
            token,
            kv_path,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build reqwest client"),
            available,
        }
    }

    /// Create with explicit values.
    pub fn new(addr: &str, token: Option<&str>, kv_path: &str) -> Self {
        let token = token.filter(|t| !t.is_empty());
        Self {
            addr: addr.trim_end_matches('/').to_string(),
            token: token.map(|t| t.to_string()),
            kv_path: kv_path.to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build reqwest client"),
            available: token.is_some(),
        }
    }

    /// Whether Vault is configured and available.
    pub fn available(&self) -> bool {
        self.available
    }

    /// Set a token directly (e.g., after K8s auth).
    pub fn set_token(&mut self, token: &str) {
        self.token = Some(token.to_string());
        self.available = true;
    }

    /// Try to authenticate using the configured method.
    pub async fn authenticate(&mut self, method: &AuthMethod) -> Result<(), VaultError> {
        match method {
            AuthMethod::Token => {
                let _token = self
                    .token
                    .as_ref()
                    .ok_or_else(|| VaultError::Auth("No VAULT_TOKEN set".into()))?;
                // Validate with a lookup
                self.lookup_token().await?;
                info!("Vault token authenticated");
                self.available = true;
                Ok(())
            }
            AuthMethod::Kubernetes { role } => {
                let jwt = std::fs::read_to_string(
                    "/var/run/secrets/kubernetes.io/serviceaccount/token",
                )
                .map_err(|_| {
                    VaultError::Auth("Not in Kubernetes — no service account token".into())
                })?;

                let resp = self
                    .post("auth/kubernetes/login", &serde_json::json!({
                        "role": role,
                        "jwt": jwt.trim(),
                    }))
                    .await?;

                let token = resp["auth"]["client_token"]
                    .as_str()
                    .ok_or_else(|| VaultError::Auth("No client_token in response".into()))?;

                self.set_token(token);
                info!("Vault Kubernetes auth successful");
                Ok(())
            }
            AuthMethod::AppRole {
                role_id,
                secret_id,
            } => {
                let resp = self
                    .post("auth/approle/login", &serde_json::json!({
                        "role_id": role_id,
                        "secret_id": secret_id,
                    }))
                    .await?;

                let token = resp["auth"]["client_token"]
                    .as_str()
                    .ok_or_else(|| VaultError::Auth("No client_token in response".into()))?;

                self.set_token(token);
                info!("Vault AppRole auth successful");
                Ok(())
            }
            AuthMethod::TokenFile { path } => {
                let token = std::fs::read_to_string(path)
                    .map_err(|e| VaultError::Auth(format!("Failed to read token file: {e}")))?;
                self.set_token(token.trim());
                self.lookup_token().await?;
                info!("Vault token file auth successful");
                Ok(())
            }
        }
    }

    /// Validate the current token.
    async fn lookup_token(&self) -> Result<serde_json::Value, VaultError> {
        self.api_get("auth/token/lookup-self").await
    }

    /// Read a single secret value.
    ///
    /// Paths follow Vault KV v2 conventions: `"providers/deepseek/api-key"`.
    /// Returns the first value in the secret, or `None` if not found.
    pub async fn read(&self, path: &str) -> Option<String> {
        if !self.available {
            return None;
        }
        match self.read_secret(path).await {
            Ok(data) => {
                if let Some(obj) = data.as_object() {
                    for v in obj.values() {
                        if let Some(s) = v.as_str() {
                            if !s.is_empty() {
                                return Some(s.to_string());
                            }
                        }
                    }
                }
                None
            }
            Err(e) => {
                debug!(%path, error = %e, "Vault read");
                None
            }
        }
    }

    /// Read a secret value with a default fallback.
    pub async fn read_or(&self, path: &str, default: &str) -> String {
        self.read(path).await.unwrap_or_else(|| default.to_string())
    }

    /// Read all keys at a path.
    pub async fn read_all(&self, path: &str) -> HashMap<String, String> {
        if !self.available {
            return HashMap::new();
        }
        match self.read_secret(path).await {
            Ok(data) => {
                let mut map = HashMap::new();
                if let Some(obj) = data.as_object() {
                    for (k, v) in obj {
                        if let Some(s) = v.as_str() {
                            map.insert(k.clone(), s.to_string());
                        }
                    }
                }
                map
            }
            Err(e) => {
                debug!(%path, error = %e, "Vault read_all");
                HashMap::new()
            }
        }
    }

    /// Write secrets at a path.
    pub async fn write(&self, path: &str, data: HashMap<String, String>) -> Result<(), VaultError> {
        self.post(
            &format!("{}/data/{}", self.kv_path, path),
            &serde_json::json!({ "data": data }),
        )
        .await?;
        Ok(())
    }

    /// List paths under a prefix.
    pub async fn list_paths(&self, prefix: &str) -> Vec<String> {
        if !self.available {
            return Vec::new();
        }
        match self
            .api_request(
                reqwest::Method::from_bytes(b"LIST").unwrap(),
                &format!("{}/metadata/{}", self.kv_path, prefix),
            )
            .await
        {
            Ok(resp) => resp
                .get("data")
                .and_then(|d| d.get("keys"))
                .and_then(|k| k.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            Err(e) => {
                debug!(%prefix, error = %e, "Vault list_paths");
                Vec::new()
            }
        }
    }

    /// Check Vault health.
    pub async fn health_check(&self) -> Result<bool, VaultError> {
        let resp = self
            .http
            .get(format!("{}/v1/sys/health", self.addr))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }

    /// Check whether Vault is sealed.
    pub async fn is_sealed(&self) -> Result<bool, VaultError> {
        let resp = self
            .http
            .get(format!("{}/v1/sys/seal-status", self.addr))
            .send()
            .await?;
        let body: serde_json::Value = resp.json().await?;
        Ok(body
            .get("sealed")
            .and_then(|v| v.as_bool())
            .unwrap_or(true))
    }

    // ── Internal helpers ──

    async fn read_secret(&self, path: &str) -> Result<serde_json::Value, VaultError> {
        let resp = self
            .api_get(&format!("{}/data/{}", self.kv_path, path))
            .await?;
        let data = resp
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        Ok(data)
    }

    async fn api_get(&self, api_path: &str) -> Result<serde_json::Value, VaultError> {
        self.api_request(reqwest::Method::GET, api_path).await
    }

    async fn post(
        &self,
        api_path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, VaultError> {
        self.api_request_with_body(reqwest::Method::POST, api_path, Some(body))
            .await
    }

    async fn api_request(
        &self,
        method: reqwest::Method,
        api_path: &str,
    ) -> Result<serde_json::Value, VaultError> {
        self.api_request_with_body(method, api_path, None).await
    }

    async fn api_request_with_body(
        &self,
        method: reqwest::Method,
        api_path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, VaultError> {
        let url = format!("{}/v1/{}", self.addr, api_path.trim_start_matches('/'));
        let mut req = self
            .http
            .request(method, &url)
            .header("Content-Type", "application/json");

        if let Some(ref token) = self.token {
            req = req.header("X-Vault-Token", token);
        }

        if let Some(b) = body {
            req = req.json(b);
        }

        let resp = req.send().await.map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                VaultError::Unreachable(format!("Vault at {}: {e}", self.addr))
            } else {
                VaultError::Network(e)
            }
        })?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            let msg = text.chars().take(200).collect::<String>();
            warn!(%url, %status, %msg, "Vault HTTP error");
            return Err(VaultError::Http(status.as_u16(), msg));
        }

        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(VaultError::Json)?;
        Ok(value)
    }
}

/// Convenience helper: resolve a secret using the priority chain.
///
/// 1. If `vault` is available, try `vault_path`
/// 2. Fall back to `env_var_name` env var
/// 3. Fall back to `env_var_name` key in `keys_config`
pub async fn resolve_secret(
    vault: &VaultClient,
    vault_path: &str,
    env_var_name: &str,
    keys_config: &HashMap<String, String>,
) -> Option<String> {
    // 1. Vault first
    if vault.available() {
        if let Some(val) = vault.read(vault_path).await {
            debug!(%env_var_name, "Resolved from Vault");
            return Some(val);
        }
    }

    // 2. Environment
    if let Ok(val) = std::env::var(env_var_name) {
        if !val.is_empty() {
            debug!(%env_var_name, "Resolved from env");
            return Some(val);
        }
    }

    // 3. keys.toml
    if let Some(val) = keys_config.get(env_var_name) {
        if !val.is_empty() {
            debug!(%env_var_name, "Resolved from keys.toml");
            return Some(val.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_client_not_available_without_token() {
        std::env::remove_var("VAULT_TOKEN");
        let client = VaultClient::from_env();
        assert!(!client.available());
    }

    #[test]
    fn test_vault_client_available_with_token() {
        std::env::set_var("VAULT_TOKEN", "test-token");
        let client = VaultClient::from_env();
        assert!(client.available());
        std::env::remove_var("VAULT_TOKEN");
    }

    #[test]
    fn test_vault_client_new() {
        let c1 = VaultClient::new("http://vault:8200", Some("t"), "kv");
        assert!(c1.available());

        let c2 = VaultClient::new("http://vault:8200", None, "kv");
        assert!(!c2.available());

        let c3 = VaultClient::new("http://vault:8200", Some(""), "kv");
        assert!(!c3.available());
    }

    #[tokio::test]
    async fn test_resolve_secret_priority() {
        let vault = VaultClient::new("http://nonexistent:8200", Some("t"), "kv");
        let mut keys = HashMap::new();
        keys.insert("TEST_KEY".into(), "from-toml".into());

        // With vault unavailable and no env var, falls back to toml
        std::env::remove_var("TEST_KEY");
        let result = resolve_secret(&vault, "test/key", "TEST_KEY", &keys).await;
        assert_eq!(result, Some("from-toml".into()));

        // With env var set, takes env over toml
        std::env::set_var("TEST_KEY", "from-env");
        let result = resolve_secret(&vault, "test/key", "TEST_KEY", &keys).await;
        assert_eq!(result, Some("from-env".into()));
        std::env::remove_var("TEST_KEY");

        // With none available
        let empty_keys = HashMap::new();
        let result = resolve_secret(&vault, "test/key", "NONEXISTENT", &empty_keys).await;
        assert_eq!(result, None);
    }
}
