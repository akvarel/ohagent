//! Webhook handlers for messaging platforms (WhatsApp, Slack).
//!
//! Each platform gets its own webhook endpoint.
//! Webhooks receive POST requests from the platform and route them
//! through the appropriate adapter.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use ohagent_gateway::platforms::{
    slack::SlackAdapter, viber::ViberAdapter, whatsapp::WhatsAppAdapter,
};

/// Shared state for webhook handlers.
#[derive(Clone)]
pub struct WebhookState {
    pub whatsapp: Option<Arc<WhatsAppAdapter>>,
    pub slack: Option<Arc<SlackAdapter>>,
    pub viber: Option<Arc<ViberAdapter>>,
}

// ── WhatsApp webhook ──

#[derive(Deserialize)]
pub struct WaVerifyQuery {
    #[serde(rename = "hub.mode")]
    pub mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    pub verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    pub challenge: Option<String>,
}

/// GET /webhooks/whatsapp — Meta webhook verification.
pub async fn wa_verify(
    State(state): State<WebhookState>,
    Query(q): Query<WaVerifyQuery>,
) -> Response {
    let adapter = match &state.whatsapp {
        Some(a) => a,
        None => {
            return (StatusCode::SERVICE_UNAVAILABLE, "WhatsApp not configured").into_response()
        }
    };

    let mode = q.mode.as_deref().unwrap_or("");
    let token = q.verify_token.as_deref().unwrap_or("");
    let challenge = q.challenge.as_deref().unwrap_or("");

    match adapter.verify_webhook(mode, token, challenge) {
        Ok(challenge) => (StatusCode::OK, challenge).into_response(),
        Err(e) => (StatusCode::FORBIDDEN, e).into_response(),
    }
}

/// POST /webhooks/whatsapp — incoming message.
pub async fn wa_webhook(State(state): State<WebhookState>, body: String) -> Response {
    let adapter = match &state.whatsapp {
        Some(a) => a,
        None => {
            return (StatusCode::SERVICE_UNAVAILABLE, "WhatsApp not configured").into_response()
        }
    };

    match adapter.handle_webhook(&body).await {
        Ok(()) => (StatusCode::OK, "ok").into_response(),
        Err(e) => {
            tracing::error!(error = %e, "WhatsApp webhook error");
            (StatusCode::INTERNAL_SERVER_ERROR, e).into_response()
        }
    }
}

// ── Slack webhook ──

/// POST /webhooks/slack — Slack Events API.
pub async fn slack_webhook(
    State(state): State<WebhookState>,
    headers: axum::http::HeaderMap,
    body: String,
) -> Response {
    let adapter = match &state.slack {
        Some(a) => a,
        None => return (StatusCode::SERVICE_UNAVAILABLE, "Slack not configured").into_response(),
    };

    let signature = headers
        .get("X-Slack-Signature")
        .and_then(|v| v.to_str().ok());
    let timestamp = headers
        .get("X-Slack-Request-Timestamp")
        .and_then(|v| v.to_str().ok());

    match adapter.handle_webhook(&body, signature, timestamp).await {
        Ok((status, body)) => {
            (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), body).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Slack webhook error");
            (StatusCode::INTERNAL_SERVER_ERROR, e).into_response()
        }
    }
}

// ── Viber webhook ──

/// POST /webhooks/viber — Viber callbacks.
pub async fn viber_webhook(State(state): State<WebhookState>, body: String) -> Response {
    let adapter = match &state.viber {
        Some(a) => a,
        None => return (StatusCode::SERVICE_UNAVAILABLE, "Viber not configured").into_response(),
    };

    match adapter.handle_webhook(&body).await {
        Ok((status, body)) => {
            (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), body).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Viber webhook error");
            (StatusCode::INTERNAL_SERVER_ERROR, e).into_response()
        }
    }
}
