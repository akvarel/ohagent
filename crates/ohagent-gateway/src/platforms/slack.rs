//! Slack adapter via Events API + Web API.
//!
//! ## Setup
//!
//! 1. Create a Slack App at <https://api.slack.com/apps>
//! 2. Enable Events API, subscribe to `message.channels`, `app_mention`
//! 3. Set env vars:
//!    - `SLACK_BOT_TOKEN` — Bot User OAuth Token (xoxb-...)
//!    - `SLACK_SIGNING_SECRET` — Signing Secret from Basic Information
//!
//! ## Webhook
//!
//! The daemon exposes:
//! - `POST /webhooks/slack` — receives Slack events + URL verification
//!
//! ## Message format
//!
//! Incoming: Slack Events API JSON with `type: event_callback`
//! Outgoing: POST to `chat.postMessage` with bot token

use std::sync::Arc;

use anyhow::Context;
use reqwest::Client;
use serde::Deserialize;
use tracing::{error, info, warn};

use ohagent_core::jcode_bridge::JcodeBridge;

use crate::adapter::{IncomingMessage, OutgoingMessage, PlatformAdapter};
use crate::dispatch::Dispatcher;
use crate::i18n::Lang;
use crate::pairing::PairingManager;
use crate::session::SessionManager;

/// Slack adapter.
pub struct SlackAdapter {
    bot_token: String,
    signing_secret: String,
    client: Client,
    dispatcher: Option<Arc<Dispatcher>>,
    pairing_manager: Option<Arc<PairingManager>>,
    session_manager: Option<Arc<SessionManager>>,
}

impl SlackAdapter {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let bot_token = std::env::var("SLACK_BOT_TOKEN")
            .map_err(|_| "SLACK_BOT_TOKEN not set")?;
        let signing_secret = std::env::var("SLACK_SIGNING_SECRET")
            .map_err(|_| "SLACK_SIGNING_SECRET not set")?;

        Ok(Self {
            bot_token,
            signing_secret,
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

    // ── Webhook handler ──

    /// POST /webhooks/slack — process Slack event.
    ///
    /// Handles:
    /// - URL verification (type: url_verification)
    /// - Event callbacks (type: event_callback)
    pub async fn handle_webhook(
        &self,
        body: &str,
        slack_signature: Option<&str>,
        slack_timestamp: Option<&str>,
    ) -> Result<(u16, String), String> {
        // Verify Slack signing secret for all requests
        if let (Some(sig), Some(ts)) = (slack_signature, slack_timestamp) {
            if !verify_slack_signature(&self.signing_secret, ts, body, sig) {
                warn!("Slack signature verification failed");
                return Ok((401, "invalid_signature".into()));
            }
        } else {
            warn!("Missing Slack signature headers");
            return Ok((401, "missing_signature".into()));
        }

        // URL verification challenge
        let parsed: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("Parse: {e}"))?;

        // Handle URL verification
        if parsed.get("type").and_then(|v| v.as_str()) == Some("url_verification") {
            let challenge = parsed["challenge"].as_str().unwrap_or("");
            return Ok((200, challenge.to_string()));
        }

        // Handle event callback
        let event: SlackEvent = serde_json::from_str(body)
            .map_err(|e| format!("Event parse: {e}"))?;

        // Skip bot's own messages
        if let Some(ref e) = event.event {
            if e.bot_id.is_some() || e.subtype == Some("bot_message".into()) {
                return Ok((200, "ok".into()));
            }

            let text = e.text.as_deref().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return Ok((200, "ok".into()));
            }

            // Clean bot mention from text (e.g. "<@U123> hello" → "hello")
            let cleaned = clean_slack_text(&text);

            let channel = e.channel.clone();
            let user = e.user.clone().unwrap_or_default();

            info!(
                slack_channel = %channel,
                slack_user = %user,
                text_len = cleaned.len(),
                "Slack incoming"
            );

            let dispatcher = self.dispatcher.as_ref().ok_or("No dispatcher")?;

            let incoming = IncomingMessage {
                chat_id: channel.clone(),
                user_id: user,
                tenant_id: format!("slack:{}", channel),
                text: cleaned,
                lang: Lang::En,
                platform: "slack".into(),
            attachment: None,
            };

            if let Some(response) = dispatcher.handle_message(incoming).await {
                if let Err(e) = self
                    .send_message(OutgoingMessage {
                        chat_id: channel,
                        text: response.text,
                        markdown: response.markdown,
                                    inline_keyboard: response.inline_keyboard,
                    })
                    .await
                {
                    error!(error = %e, "Failed to send Slack reply");
                }
            }
        }

        Ok((200, "ok".into()))
    }

    /// Send a message to a Slack channel.
    async fn send_slack_text(
        &self,
        channel: &str,
        text: &str,
        markdown: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut body = serde_json::json!({
            "channel": channel,
            "text": text,
        });

        if markdown {
            body["mrkdwn"] = serde_json::Value::Bool(true);
        }

        let resp = self
            .client
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.bot_token)
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send()
            .await
            .context("Slack API request failed")?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp
            .json()
            .await
            .unwrap_or(serde_json::json!({"ok": false}));

        if !resp_body["ok"].as_bool().unwrap_or(false) {
            let err = resp_body["error"].as_str().unwrap_or("unknown");
            return Err(format!("Slack API error: {err}").into());
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl PlatformAdapter for SlackAdapter {
    fn name(&self) -> &str {
        "slack"
    }

    async fn start(
        &self,
        bridge: Arc<JcodeBridge>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Slack adapter registered (webhook mode — use daemon endpoint)");
        let _ = bridge;
        Ok(())
    }

    async fn send_message(
        &self,
        msg: OutgoingMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.send_slack_text(&msg.chat_id, &msg.text, msg.markdown)
            .await
    }
}

// ── Slack event types ──

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SlackEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub event: Option<InnerEvent>,
    pub challenge: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct InnerEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub channel: String,
    pub user: Option<String>,
    pub text: Option<String>,
    pub bot_id: Option<String>,
    pub subtype: Option<String>,
    pub ts: Option<String>,
}

/// Verify a Slack request signature (HMAC-SHA256).
///
/// Replay protection: rejects timestamps more than 5 minutes old.
fn verify_slack_signature(
    signing_secret: &str,
    timestamp: &str,
    body: &str,
    signature: &str,
) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    // Check timestamp freshness (prevent replay attacks)
    let ts: i64 = match timestamp.parse() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    if (now - ts).abs() > 300 {
        return false;
    }

    // Recompute signature: v0=hex(HMAC-SHA256(secret, "v0:{ts}:{body}"))
    let sig_base = format!("v0:{timestamp}:{body}");
    let mut mac = Hmac::<Sha256>::new_from_slice(signing_secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(sig_base.as_bytes());
    let computed = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

    // Constant-time comparison
    let expected = computed.as_bytes();
    let actual = signature.as_bytes();
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual.iter())
            .fold(0, |acc, (a, b)| acc | (a ^ b))
            == 0
}
fn clean_slack_text(text: &str) -> String {
    // Remove bot mentions like <@U12345> — simple approach
    let mut result = text.to_string();
    while let Some(start) = result.find("<@") {
        if let Some(end) = result[start..].find('>') {
            let abs_end = start + end + 1;
            result.replace_range(start..abs_end, "");
        } else {
            break;
        }
    }
    result.trim().to_string()
}
