//! Proactive push notifications — agent-initiated messages to users.
//!
//! Unlike request-response conversations, push notifications allow ohAgent
//! to send messages without user prompt. Uses:
//! - Reminders ("your meeting is in 10 minutes")
//! - Task completions ("build succeeded")
//! - Alerts ("error rate spiked to 5%")
//!
//! ## Architecture
//!
//! ```text
//! Agent / Cron job → PushService.send(tenant_id, message)
//!                              ↓
//!                    lookup tenant_id → chat_id
//!                              ↓
//!                    Telegram Bot API (sendMessage)
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use reqwest::Client;
use tracing::{debug, info, warn};

/// Result of a push operation.
#[derive(Debug, Clone)]
pub struct PushResult {
    pub success: bool,
    pub tenant_id: String,
    pub chat_id: String,
    pub error: Option<String>,
}

/// Proactive push notification service.
///
/// Stores tenant_id → chat_id mappings and sends messages via
/// the Telegram Bot API. Thread-safe, shared across daemon components.
pub struct PushService {
    bot_token: String,
    tenant_to_chat: Mutex<HashMap<String, String>>,
    client: Client,
}

impl PushService {
    /// Create a new push service with a Telegram bot token.
    /// Token resolved from env (injected by Vault).
    pub fn new(bot_token: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            tenant_to_chat: Mutex::new(HashMap::new()),
            client: Client::new(),
        }
    }

    /// Register a tenant's chat ID for push notifications.
    /// Called during pairing.
    pub fn register(&self, tenant_id: &str, chat_id: &str) {
        let mut map = self.tenant_to_chat.lock().unwrap();
        map.insert(tenant_id.to_string(), chat_id.to_string());
        info!(tenant = tenant_id, chat_id, "Push: registered tenant");
    }

    /// Unregister a tenant (on unpair / /new).
    pub fn unregister(&self, tenant_id: &str) {
        let mut map = self.tenant_to_chat.lock().unwrap();
        map.remove(tenant_id);
        debug!(tenant = tenant_id, "Push: unregistered tenant");
    }

    /// Get the chat_id for a tenant.
    pub fn chat_id_for(&self, tenant_id: &str) -> Option<String> {
        let map = self.tenant_to_chat.lock().unwrap();
        map.get(tenant_id).cloned()
    }

    /// List all registered tenants.
    pub fn list_tenants(&self) -> Vec<String> {
        let map = self.tenant_to_chat.lock().unwrap();
        map.keys().cloned().collect()
    }

    /// Send a push notification to a specific tenant.
    ///
    /// Returns Ok if the message was sent, Err if tenant not found or API error.
    pub async fn send(&self, tenant_id: &str, message: &str) -> PushResult {
        let chat_id = {
            let map = self.tenant_to_chat.lock().unwrap();
            map.get(tenant_id).cloned()
        };

        let chat_id = match chat_id {
            Some(id) => id,
            None => {
                return PushResult {
                    success: false,
                    tenant_id: tenant_id.to_string(),
                    chat_id: String::new(),
                    error: Some(format!("Tenant '{tenant_id}' not registered for push")),
                };
            }
        };

        self.send_raw(&chat_id, message).await
    }

    /// Send a push notification to a raw chat_id (platform-level).
    pub async fn send_raw(&self, chat_id: &str, message: &str) -> PushResult {
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.bot_token
        );

        let params = serde_json::json!({
            "chat_id": chat_id,
            "text": message,
            "parse_mode": "Markdown",
        });

        match self.client.post(&url).json(&params).send().await {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();

                let success = status.is_success();
                if success {
                    debug!(chat_id, len = message.len(), "Push: sent");
                } else {
                    warn!(chat_id, status = %status, %body, "Push: failed");
                }

                PushResult {
                    success,
                    tenant_id: String::new(), // raw call — no tenant context
                    chat_id: chat_id.to_string(),
                    error: if success { None } else { Some(body) },
                }
            }
            Err(e) => {
                warn!(chat_id, error = %e, "Push: network error");
                PushResult {
                    success: false,
                    tenant_id: String::new(),
                    chat_id: chat_id.to_string(),
                    error: Some(e.to_string()),
                }
            }
        }
    }

    /// Broadcast a message to all registered tenants.
    pub async fn broadcast(&self, message: &str) -> Vec<PushResult> {
        let tenants = self.list_tenants();
        let mut results = Vec::with_capacity(tenants.len());
        for tenant in &tenants {
            results.push(self.send(tenant, message).await);
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_lookup() {
        let svc = PushService::new("test_token");
        svc.register("tenant1", "12345");
        svc.register("tenant2", "67890");

        assert_eq!(svc.chat_id_for("tenant1"), Some("12345".into()));
        assert_eq!(svc.chat_id_for("tenant2"), Some("67890".into()));
        assert_eq!(svc.chat_id_for("nonexistent"), None);
    }

    #[test]
    fn test_unregister() {
        let svc = PushService::new("test_token");
        svc.register("t1", "111");
        svc.unregister("t1");
        assert!(svc.chat_id_for("t1").is_none());
    }

    #[test]
    fn test_list_tenants() {
        let svc = PushService::new("test_token");
        svc.register("a", "1");
        svc.register("b", "2");
        let mut tenants = svc.list_tenants();
        tenants.sort();
        assert_eq!(tenants, vec!["a", "b"]);
    }
}
