//! WhatsApp Cloud API adapter via Meta Business Platform.
//!
//! ## Setup
//!
//! 1. Create a Meta Business App at <https://developers.facebook.com>
//! 2. Add WhatsApp product, configure webhook
//! 3. Set env vars:
//!    - `WHATSAPP_TOKEN` — temporary access token or permanent token
//!    - `WHATSAPP_PHONE_ID` — phone number ID from Meta dashboard
//!    - `WHATSAPP_VERIFY_TOKEN` — webhook verify token (any string)
//!
//! ## Webhook
//!
//! The daemon exposes:
//! - `GET  /webhooks/whatsapp` — webhook verification (Meta sends challenge)
//! - `POST /webhooks/whatsapp` — incoming messages
//!
//! ## Message format
//!
//! Incoming: Meta sends JSON with `entry[].changes[].value.messages[]`
//! Outgoing: POST to `graph.facebook.com/v21.0/{phone_id}/messages`

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

/// WhatsApp Cloud API adapter.
pub struct WhatsAppAdapter {
    token: String,
    phone_id: String,
    verify_token: String,
    client: Client,
    dispatcher: Option<Arc<Dispatcher>>,
    pairing_manager: Option<Arc<PairingManager>>,
    session_manager: Option<Arc<SessionManager>>,
}

impl WhatsAppAdapter {
    /// Create from environment variables.
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let token = std::env::var("WHATSAPP_TOKEN")
            .map_err(|_| "WHATSAPP_TOKEN not set")?;
        let phone_id = std::env::var("WHATSAPP_PHONE_ID")
            .map_err(|_| "WHATSAPP_PHONE_ID not set")?;
        let verify_token = std::env::var("WHATSAPP_VERIFY_TOKEN")
            .map_err(|_| "WHATSAPP_VERIFY_TOKEN not set")?;

        Ok(Self {
            token,
            phone_id,
            verify_token,
            client: Client::new(),
            dispatcher: None,
            pairing_manager: None,
            session_manager: None,
        })
    }

    /// Set the dispatcher (called during `start()`).
    pub fn with_dispatcher(mut self, d: Arc<Dispatcher>) -> Self {
        self.dispatcher = Some(d);
        self
    }

    /// Set pairing manager.
    pub fn with_pairing(mut self, p: Arc<PairingManager>) -> Self {
        self.pairing_manager = Some(p);
        self
    }

    // ── Webhook handlers (called from daemon axum routes) ──

    /// GET /webhooks/whatsapp — Meta webhook verification.
    pub fn verify_webhook(
        &self,
        mode: &str,
        token: &str,
        challenge: &str,
    ) -> Result<String, String> {
        if mode == "subscribe" && token == self.verify_token {
            Ok(challenge.to_string())
        } else {
            Err("Verification failed".into())
        }
    }

    /// POST /webhooks/whatsapp — process incoming message.
    pub async fn handle_webhook(&self, body: &str) -> Result<(), String> {
        let parsed: WebhookPayload =
            serde_json::from_str(body).map_err(|e| format!("Parse error: {e}"))?;

        let dispatcher = self
            .dispatcher
            .as_ref()
            .ok_or("Dispatcher not configured")?;

        for entry in &parsed.entry {
            for change in &entry.changes {
                let value = &change.value;

                // Skip status updates (delivered, read receipts)
                if let Some(statuses) = &value.statuses {
                    for s in statuses {
                        info!(
                            wa_msg_id = %s.id,
                            status = %s.status,
                            "WhatsApp message status"
                        );
                    }
                    continue;
                }

                // Process incoming messages
                if let Some(messages) = &value.messages {
                    for msg in messages {
                        let from = value
                            .contacts
                            .as_ref()
                            .and_then(|c| c.first())
                            .map(|c| c.profile.name.as_str())
                            .unwrap_or("unknown");

                        let text = match &msg.text {
                            Some(t) => t.body.clone(),
                            None => match &msg.interactive {
                                Some(i) => i
                                    .button_reply
                                    .as_ref()
                                    .map(|b| b.title.clone())
                                    .unwrap_or_else(|| i.list_reply.as_ref()
                                        .map(|l| l.title.clone())
                                        .unwrap_or_default()),
                                None => {
                                    warn!("Unsupported WhatsApp message type");
                                    continue;
                                }
                            },
                        };

                        if text.is_empty() {
                            continue;
                        }

                        info!(
                            wa_from = %from,
                            wa_msg_id = %msg.id,
                            text_len = text.len(),
                            "WhatsApp incoming"
                        );

                        let incoming = IncomingMessage {
                            chat_id: msg.from.clone(),
                            user_id: msg.from.clone(),
                            tenant_id: format!("wa:{}", msg.from),
                            text,
                            lang: Lang::En,
                            platform: "whatsapp".into(),
                            attachment: None,
                        };

                        if let Some(response) = dispatcher.handle_message(incoming).await {
                            let send_result = self
                                .send_message(OutgoingMessage {
                                    chat_id: msg.from.clone(),
                                    text: response.text,
                                    markdown: response.markdown,
                                })
                                .await;

                            if let Err(e) = send_result {
                                error!(error = %e, "Failed to send WhatsApp reply");
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Send a text message via WhatsApp Cloud API.
    async fn send_wa_text(&self, to: &str, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!(
            "https://graph.facebook.com/v21.0/{}/messages",
            self.phone_id
        );

        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "text",
            "text": {
                "preview_url": false,
                "body": text
            }
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .context("WhatsApp API request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let err_body = resp.text().await.unwrap_or_default();
            return Err(format!("WhatsApp API error {status}: {err_body}").into());
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl PlatformAdapter for WhatsAppAdapter {
    fn name(&self) -> &str {
        "whatsapp"
    }

    async fn start(
        &self,
        bridge: Arc<JcodeBridge>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("WhatsApp adapter registered (webhook mode — use daemon endpoint)");
        let _ = bridge; // webhook-based, bridge used by dispatcher
        Ok(())
    }

    async fn send_message(
        &self,
        msg: OutgoingMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.send_wa_text(&msg.chat_id, &msg.text).await
    }
}

// ── Webhook payload types ──

#[derive(Debug, Deserialize)]
pub struct WebhookPayload {
    pub entry: Vec<WebhookEntry>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookEntry {
    pub changes: Vec<WebhookChange>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookChange {
    pub value: WebhookValue,
}

#[derive(Debug, Deserialize)]
pub struct WebhookValue {
    #[serde(default)]
    pub contacts: Option<Vec<Contact>>,
    #[serde(default)]
    pub messages: Option<Vec<WaMessage>>,
    #[serde(default)]
    pub statuses: Option<Vec<WaStatus>>,
}

#[derive(Debug, Deserialize)]
pub struct Contact {
    pub profile: ContactProfile,
}

#[derive(Debug, Deserialize)]
pub struct ContactProfile {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct WaMessage {
    pub from: String,
    pub id: String,
    #[serde(default)]
    pub text: Option<TextBody>,
    #[serde(default)]
    pub interactive: Option<Interactive>,
}

#[derive(Debug, Deserialize)]
pub struct TextBody {
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct Interactive {
    #[serde(default)]
    pub button_reply: Option<ButtonReply>,
    #[serde(default)]
    pub list_reply: Option<ListReply>,
}

#[derive(Debug, Deserialize)]
pub struct ButtonReply {
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct ListReply {
    pub title: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct WaStatus {
    pub id: String,
    pub status: String,
}
