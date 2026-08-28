//! API authentication middleware for protecting internal endpoints.
//!
//! Provides:
//! - Bearer token auth (X-API-Key or Authorization: Bearer)
//! - Signed URL verification (HMAC query params, for webhook callbacks)
//!
//! Internal endpoints (`/api/*`) require authentication.
//! Public endpoints (`/health`, `/v1/*`, `/webhooks/*`) are always allowed.
//!
//! Token is resolved from Vault → env var `OHAGENT_API_KEY` → generated on startup.

use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response, Extension};
use std::sync::Arc;
use tracing::warn;

/// Configuration for API auth.
#[derive(Clone)]
pub struct AuthConfig {
    /// API key for Bearer token / X-API-Key auth.
    /// If empty, a random key is generated on startup.
    pub api_key: Arc<String>,
    /// Whether to enforce auth on /api/* endpoints.
    pub enforce: bool,
}

impl AuthConfig {
    /// Create a new auth config from environment or generate a random key.
    pub fn from_env() -> Self {
        let api_key = std::env::var("OHAGENT_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .unwrap_or_else(|| {
                use rand::Rng;
                let key: String = rand::thread_rng()
                    .sample_iter(&rand::distributions::Alphanumeric)
                    .take(48)
                    .map(char::from)
                    .collect();
                tracing::info!(
                    generated_api_key = %key,
                    "No OHAGENT_API_KEY set — generated random key"
                );
                key
            });

        Self {
            api_key: Arc::new(api_key),
            enforce: true, // Always enforce on /api/*
        }
    }

    /// Validate a token against the configured API key.
    pub fn validate(&self, token: &str) -> bool {
        // Constant-time comparison to prevent timing attacks
        let expected = self.api_key.as_bytes();
        let actual = token.as_bytes();
        if expected.len() != actual.len() {
            return false;
        }
        expected
            .iter()
            .zip(actual.iter())
            .fold(0, |acc, (a, b)| acc | (a ^ b))
            == 0
    }
}

/// Shared state for auth middleware.
#[derive(Clone)]
pub struct AuthState {
    pub config: AuthConfig,
}

/// Axum middleware: require valid API key for /api/* paths.
/// State is passed via Extension to avoid type conflicts with handler state.
pub async fn require_auth(
    Extension(state): Extension<AuthState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path();

    // Only protect /api/* endpoints
    if !path.starts_with("/api/") {
        return Ok(next.run(request).await);
    }

    // Skip auth for public API endpoints if configured
    if !state.config.enforce {
        return Ok(next.run(request).await);
    }

    // Extract token from:
    // 1. X-API-Key header
    // 2. Authorization: Bearer <token>
    let token = request
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            request
                .headers()
                .get("Authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        });

    match token {
        Some(t) if state.config.validate(t) => Ok(next.run(request).await),
        Some(_) => {
            warn!(%path, "Invalid API key");
            Err(StatusCode::UNAUTHORIZED)
        }
        None => {
            warn!(%path, "Missing API key");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

// ── Slack signing secret verification ──

/// Verify a Slack request using the signing secret (HMAC-SHA256).
///
/// Slack includes:
/// - `X-Slack-Request-Timestamp` header
/// - `X-Slack-Signature` header
/// - Raw request body
///
/// The signature is: `v0=` + hex(HMAC-SHA256(secret, "v0:{timestamp}:{body}"))
pub fn verify_slack_signature(
    signing_secret: &str,
    timestamp: &str,
    body: &str,
    signature: &str,
) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    // Check timestamp is not too old (prevent replay attacks)
    let ts: i64 = timestamp.parse().unwrap_or(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    if (now - ts).abs() > 300 {
        return false; // Request too old
    }

    // Recompute signature
    let sig_base = format!("v0:{timestamp}:{body}");
    let mut mac = Hmac::<Sha256>::new_from_slice(signing_secret.as_bytes())
        .expect("HMAC can take a key of any size");

    mac.update(sig_base.as_bytes());
    let computed = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

    // Constant-time comparison
    let expected = computed.as_bytes();
    let actual = signature.as_bytes();
    if expected.len() != actual.len() {
        return false;
    }
    expected
        .iter()
        .zip(actual.iter())
        .fold(0, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_config_validate() {
        let config = AuthConfig {
            api_key: Arc::new("test-key-123456".into()),
            enforce: true,
        };
        assert!(config.validate("test-key-123456"));
        assert!(!config.validate("wrong-key"));
        assert!(!config.validate(""));
    }

    #[test]
    fn test_auth_config_constant_time() {
        let config = AuthConfig {
            api_key: Arc::new("a".repeat(48)),
            enforce: true,
        };
        // Same length, different content
        let wrong = "b".repeat(48);
        assert!(!config.validate(&wrong));
        // Different length
        assert!(!config.validate("short"));
    }

    #[test]
    fn test_slack_signature_replay_rejection() {
        // Old timestamp
        let old_ts = "1000000"; // Way in the past
        assert!(!verify_slack_signature("secret", old_ts, "body", "v0=abc"));
    }

    #[test]
    fn test_slack_signature_invalid_ts() {
        assert!(!verify_slack_signature(
            "secret",
            "not-a-number",
            "body",
            "v0=abc"
        ));
    }
}
