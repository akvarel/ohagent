//! Viber platform adapter via REST API + webhooks.
//!
//! ## Setup
//!
//! 1. Create a Viber Bot at <https://partners.viber.com/> → Create Bot Account
//! 2. Set env vars:
//!    - `VIBER_AUTH_TOKEN` — Bot authentication token from Viber Admin Panel
//!
//! ## Webhook
//!
//! The daemon auto-registers webhook on startup:
//! - `POST /webhooks/viber` — receives Viber callbacks + webhook registration validation
//!
//! ## API docs
//!
//! See <https://developers.viber.com/docs/api/rest-bot-api/>

use std::sync::Arc;

use anyhow::Context;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use ohagent_core::jcode_bridge::JcodeBridge;

use crate::adapter::{IncomingMessage, OutgoingMessage, PlatformAdapter};
use crate::dispatch::Dispatcher;
use crate::i18n::Lang;
use crate::pairing::PairingManager;
use crate::session::SessionManager;

/// Viber REST API base URL.
const VIBER_API_BASE: &str = "https://chatapi.viber.com/pa";

/// Viber adapter.
pub struct ViberAdapter {
    auth_token: String,
    webhook_url: Option<String>,
    client: Client,
    dispatcher: Option<Arc<Dispatcher>>,
    pairing_manager: Option<Arc<PairingManager>>,
    session_manager: Option<Arc<SessionManager>>,
}

impl ViberAdapter {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let auth_token =
            std::env::var("VIBER_AUTH_TOKEN").map_err(|_| "VIBER_AUTH_TOKEN not set")?;
        let webhook_url = std::env::var("VIBER_WEBHOOK_URL").ok();

        if webhook_url.is_none() {
            warn!("VIBER_WEBHOOK_URL not set — webhook will not be registered. Set it to your public HTTPS URL.");
        }

        Ok(Self {
            auth_token,
            webhook_url,
            client: Client::new(),
            dispatcher: None,
            pairing_manager: None,
            session_manager: None,
        })
    }

    pub fn with_dispatcher(mut self, d: Arc<Dispatcher>) -> Self {
        self.dispatcher = Some(d);
        self
    }

    pub fn with_pairing(mut self, p: Arc<PairingManager>) -> Self {
        self.pairing_manager = Some(p);
        self
    }

    /// Register webhook with Viber API (called at startup).
    async fn register_webhook(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let webhook_url = match &self.webhook_url {
            Some(u) => u.trim_end_matches('/').to_string() + "/webhooks/viber",
            None => return Ok(()),
        };

        let body = serde_json::json!({
            "url": webhook_url,
            "event_types": ["message", "subscribed", "conversation_started"],
            "send_name": true,
            "send_photo": true,
        });

        let resp = self
            .client
            .post(format!("{VIBER_API_BASE}/set_webhook"))
            .header("X-Viber-Auth-Token", &self.auth_token)
            .json(&body)
            .send()
            .await
            .context("Viber set_webhook request failed")?;

        let result: ViberApiResponse = resp
            .json()
            .await
            .context("Viber set_webhook parse failed")?;

        if result.status != 0 {
            return Err(format!(
                "Viber webhook registration failed: {} (status {})",
                result.status_message.unwrap_or_default(),
                result.status,
            )
            .into());
        }

        info!(
            webhook_url = %webhook_url,
            "Viber webhook registered"
        );

        Ok(())
    }

    /// Handle incoming Viber webhook callback.
    ///
    /// Processes:
    /// - `webhook` — webhook validation (first POST from Viber)
    /// - `message` — text messages from users
    /// - `subscribed` — user subscribed to bot
    /// - `conversation_started` — user started conversation
    pub async fn handle_webhook(&self, body: &str) -> Result<(u16, String), String> {
        let parsed: ViberCallback =
            serde_json::from_str(body).map_err(|e| format!("Viber callback parse: {e}"))?;

        match parsed.event.as_str() {
            "webhook" => {
                // Webhook validation — Viber sends this once to verify
                info!("Viber webhook validation received");
                return Ok((
                    200,
                    serde_json::json!({
                        "status": 0,
                        "status_message": "ok",
                        "event_types": ["message", "subscribed", "conversation_started"],
                    })
                    .to_string(),
                ));
            }

            "conversation_started" => {
                // User started a conversation (e.g. clicked "Send Message")
                let user_id = parsed
                    .user
                    .as_ref()
                    .map(|u| u.id.as_str())
                    .unwrap_or("unknown");
                let subscriber = parsed
                    .subscriber
                    .as_ref()
                    .map(|s| s.id.as_str())
                    .unwrap_or(user_id);

                info!(
                    viber_user = %user_id,
                    "Viber conversation started"
                );

                // Optionally send a welcome message
                if let Some(dispatcher) = &self.dispatcher {
                    let incoming = IncomingMessage {
                        chat_id: subscriber.to_string(),
                        user_id: user_id.to_string(),
                        tenant_id: format!("viber:{}", subscriber),
                        text: "/start".to_string(),
                        lang: Lang::En,
                        platform: "viber".into(),
                        attachment: None,
                    };

                    if let Some(response) = dispatcher.handle_message(incoming).await {
                        if let Err(e) = self
                            .send_viber_text(subscriber, &response.text, response.markdown)
                            .await
                        {
                            error!(error = %e, "Failed to send Viber welcome");
                        }
                    }
                }

                return Ok((200, "{\"status\":0}".into()));
            }

            "subscribed" => {
                let user_id = parsed
                    .user
                    .as_ref()
                    .map(|u| u.id.as_str())
                    .unwrap_or("unknown");
                let subscriber = parsed
                    .subscriber
                    .as_ref()
                    .map(|s| s.id.as_str())
                    .unwrap_or(user_id);

                info!(
                    viber_user = %user_id,
                    "Viber user subscribed"
                );

                return Ok((200, "{\"status\":0}".into()));
            }

            "message" => {
                // Text message from user
                if let Some(ref msg) = parsed.message {
                    // Skip non-text messages
                    let text = match msg.text.as_deref() {
                        Some(t) if msg.msg_type.as_deref() == Some("text") => t.to_string(),
                        _ => {
                            // Silently acknowledge non-text messages
                            return Ok((200, "{\"status\":0}".into()));
                        }
                    };

                    let user_id = parsed
                        .user
                        .as_ref()
                        .map(|u| u.id.as_str())
                        .unwrap_or("unknown");
                    let subscriber = parsed
                        .subscriber
                        .as_ref()
                        .map(|s| s.id.as_str())
                        .unwrap_or(user_id);

                    info!(
                        viber_user = %user_id,
                        text_len = text.len(),
                        "Viber incoming message"
                    );

                    let dispatcher = self.dispatcher.as_ref().ok_or("No dispatcher")?;

                    let incoming = IncomingMessage {
                        chat_id: subscriber.to_string(),
                        user_id: user_id.to_string(),
                        tenant_id: format!("viber:{}", subscriber),
                        text,
                        lang: Lang::En,
                        platform: "viber".into(),
                        attachment: None,
                    };

                    if let Some(response) = dispatcher.handle_message(incoming).await {
                        if let Err(e) = self
                            .send_viber_text(subscriber, &response.text, response.markdown)
                            .await
                        {
                            error!(error = %e, "Failed to send Viber reply");
                        }
                    }
                }
                return Ok((200, "{\"status\":0}".into()));
            }

            _ => {
                warn!(event = %parsed.event, "Unknown Viber event");
                return Ok((200, "{\"status\":0}".into()));
            }
        }
    }

    /// Send a text message via Viber REST API.
    async fn send_viber_text(
        &self,
        chat_id: &str,
        text: &str,
        _markdown: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Viber supports Markdown-like formatting with [b], [i], [url] tags
        let mut formatted_text = text.to_string();

        // Convert basic markdown to Viber format if markdown is true
        if _markdown {
            // Bold: **text** → [b]text[/b]
            formatted_text = formatted_text
                .replace("**", "[b]") // This is simplistic; real parsing would be better
                .replace("**", "[/b]");
            // Italic: *text* → [i]text[/i] (avoiding double-replace issues)
        }

        let body = serde_json::json!({
            "receiver": chat_id,
            "min_api_version": 1,
            "type": "text",
            "text": formatted_text,
        });

        let resp = self
            .client
            .post(format!("{VIBER_API_BASE}/send_message"))
            .header("X-Viber-Auth-Token", &self.auth_token)
            .json(&body)
            .send()
            .await
            .context("Viber send_message request failed")?;

        let result: ViberApiResponse = resp
            .json()
            .await
            .context("Viber send_message parse failed")?;

        if result.status != 0 && result.status != 1 {
            // Status 1 = message delivered but not yet read — not an error
            warn!(
                status = result.status,
                message = ?result.status_message,
                "Viber send_message non-zero status"
            );
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl PlatformAdapter for ViberAdapter {
    fn name(&self) -> &str {
        "viber"
    }

    async fn start(
        &self,
        _bridge: Arc<JcodeBridge>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Viber adapter starting, registering webhook...");
        self.register_webhook().await?;
        info!("Viber adapter ready (webhook mode — use daemon endpoint)");
        Ok(())
    }

    async fn send_message(
        &self,
        msg: OutgoingMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.send_viber_text(&msg.chat_id, &msg.text, msg.markdown)
            .await
    }
}

// ── Viber API types ──

/// Response from Viber REST API.
#[derive(Debug, Deserialize)]
struct ViberApiResponse {
    status: i64,
    #[serde(default)]
    status_message: Option<String>,
}

/// Incoming webhook callback from Viber.
#[derive(Debug, Deserialize)]
struct ViberCallback {
    event: String,
    #[serde(default)]
    user: Option<ViberUser>,
    #[serde(default)]
    subscriber: Option<ViberUser>,
    #[serde(default)]
    message: Option<ViberMessage>,
    #[serde(default)]
    timestamp: Option<i64>,
    #[serde(default)]
    chat_hostname: Option<String>,
    #[serde(default)]
    message_token: Option<i64>,
}

/// Viber user info.
#[derive(Debug, Deserialize)]
struct ViberUser {
    id: String,
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    avatar: Option<String>,
    #[allow(dead_code)]
    country: Option<String>,
    #[allow(dead_code)]
    language: Option<String>,
}

/// Viber message object.
#[derive(Debug, Deserialize)]
struct ViberMessage {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    media: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    file_name: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    size: Option<i64>,
    #[allow(dead_code)]
    #[serde(default)]
    tracking_data: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viber_callback_parse_text_message() {
        let body = r#"{
            "event": "message",
            "timestamp": 1234567890,
            "chat_hostname": "host-123",
            "message_token": 123456,
            "message": {
                "type": "text",
                "text": "Hello from Viber!",
                "tracking_data": "track-1"
            },
            "user": {
                "id": "abc123",
                "name": "John Doe",
                "avatar": "http://avatar.url",
                "country": "LV",
                "language": "en"
            },
            "subscriber": {
                "id": "xyz789",
                "name": "John Doe"
            }
        }"#;

        let parsed: ViberCallback = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.event, "message");
        assert_eq!(
            parsed.message.as_ref().unwrap().text.as_deref(),
            Some("Hello from Viber!")
        );
        assert_eq!(parsed.user.as_ref().unwrap().id, "abc123");
    }

    #[test]
    fn test_viber_callback_parse_conversation_started() {
        let body = r#"{
            "event": "conversation_started",
            "timestamp": 1234567890,
            "chat_hostname": "host-123",
            "message_token": 123456,
            "user": {
                "id": "abc123",
                "name": "John Doe"
            },
            "subscriber": {
                "id": "xyz789"
            }
        }"#;

        let parsed: ViberCallback = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.event, "conversation_started");
        assert!(parsed.message.is_none());
    }

    #[test]
    fn test_viber_callback_parse_webhook() {
        let body = r#"{"event": "webhook", "timestamp": 1234567890}"#;
        let parsed: ViberCallback = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.event, "webhook");
    }
}
