//! OpenAI-compatible `/v1/chat/completions` endpoint.
//!
//! Allows Open WebUI (and any OpenAI-compatible client) to use ohAgent
//! as a drop-in LLM backend with model routing, logging, and usage tracking.
//!
//! ## Endpoint
//!
//! `POST /v1/chat/completions`
//!
//! Supports both streaming (SSE) and non-streaming modes.

use std::sync::Arc;

use axum::{
    extract::State,
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    Json,
};
use futures::StreamExt;
use jcode_message_types::{ContentBlock, Message, Role, StreamEvent};
use serde::{Deserialize, Serialize};

use crate::api::ApiState;

// ── /v1/models handler ──

#[derive(Debug, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

#[derive(Debug, Serialize)]
pub struct ModelList {
    pub object: String,
    pub data: Vec<ModelEntry>,
}

/// GET /v1/models — list available models for Open WebUI.
pub async fn list_models_handler() -> Json<ModelList> {
    // Return a static list of known models. Open WebUI uses this for the model picker.
    let models = vec![
        ("deepseek-v4-flash", "deepseek"),
        ("deepseek-v4", "deepseek"),
        ("claude-haiku-4-5", "anthropic"),
        ("claude-sonnet-4-6", "anthropic"),
        ("claude-opus-4-5", "anthropic"),
        ("gpt-4o-mini", "openai"),
        ("gpt-4o", "openai"),
    ];

    Json(ModelList {
        object: "list".into(),
        data: models
            .into_iter()
            .map(|(id, owner)| ModelEntry {
                id: id.into(),
                object: "model".into(),
                created: 1_700_000_000,
                owned_by: owner.into(),
            })
            .collect(),
    })
}

// ── Request types (OpenAI format) ──

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiMessage {
    pub role: String,
    pub content: String,
}

// ── Response types (OpenAI format) ──

#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Debug, Serialize)]
struct Choice {
    index: u32,
    message: ChoiceMessage,
    finish_reason: String,
}

#[derive(Debug, Serialize)]
struct ChoiceMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Serialize)]
struct ChatCompletionChunk {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<ChunkChoice>,
}

#[derive(Debug, Serialize)]
struct ChunkChoice {
    index: u32,
    delta: ChunkDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

// ── Conversion helpers ──

/// Convert OpenAI messages to jcode Message format.
/// System messages are extracted and returned separately.
fn convert_messages(openai_msgs: &[OpenAiMessage]) -> (Vec<Message>, String) {
    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<Message> = Vec::new();

    for m in openai_msgs {
        match m.role.as_str() {
            "system" => {
                system_parts.push(m.content.clone());
            }
            "user" => {
                messages.push(Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: m.content.clone(),
                        cache_control: None,
                    }],
                    timestamp: None,
                    tool_duration_ms: None,
                });
            }
            "assistant" => {
                messages.push(Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text {
                        text: m.content.clone(),
                        cache_control: None,
                    }],
                    timestamp: None,
                    tool_duration_ms: None,
                });
            }
            _ => {
                // "tool", "function" etc. — treat as user for simplicity
                messages.push(Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: format!("[{}] {}", m.role, m.content),
                        cache_control: None,
                    }],
                    timestamp: None,
                    tool_duration_ms: None,
                });
            }
        }
    }

    (messages, system_parts.join("\n\n"))
}

/// Main handler for POST /v1/chat/completions
pub async fn chat_completions_handler(
    State(state): State<ApiState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let (messages, system) = convert_messages(&req.messages);
    let input_tokens = estimate_tokens(&system, &messages);

    if req.stream {
        handle_streaming(state, req, messages, system, request_id, created, input_tokens).await
    } else {
        handle_non_streaming(state, req, messages, system, request_id, created, input_tokens)
            .await
    }
}

/// Non-streaming: collect full response, return JSON.
async fn handle_non_streaming(
    state: ApiState,
    req: ChatCompletionRequest,
    messages: Vec<Message>,
    system: String,
    id: String,
    created: u64,
    input_tokens: u32,
) -> Response {
    let provider = Arc::clone(state.bridge.provider());

    match provider.complete(&messages, &[], &system, None).await {
        Ok(mut stream) => {
            let mut content = String::new();
            while let Some(event) = stream.next().await {
                match event {
                    Ok(StreamEvent::TextDelta(text)) => content.push_str(&text),
                    Err(e) => {
                        tracing::error!(error = %e, "Provider error during completion");
                        return error_response("Provider error");
                    }
                    _ => {}
                }
            }

            let output_tokens = (content.len() / 4).max(1) as u32;

            let response = ChatCompletionResponse {
                id,
                object: "chat.completion".into(),
                created,
                model: req.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: ChoiceMessage {
                        role: "assistant".into(),
                        content,
                    },
                    finish_reason: "stop".into(),
                }],
                usage: Usage {
                    prompt_tokens: input_tokens,
                    completion_tokens: output_tokens,
                    total_tokens: input_tokens + output_tokens,
                },
            };

            (axum::http::StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Provider complete() failed");
            error_response(&format!("Provider error: {e}"))
        }
    }
}

/// Streaming: SSE events with delta chunks.
async fn handle_streaming(
    state: ApiState,
    req: ChatCompletionRequest,
    messages: Vec<Message>,
    system: String,
    id: String,
    created: u64,
    _input_tokens: u32,
) -> Response {
    let provider = Arc::clone(state.bridge.provider());

    match provider.complete(&messages, &[], &system, None).await {
        Ok(stream) => {
            let id_clone = id.clone();
            let model = req.model.clone();
            let stream = stream.map(move |event| {
                match event {
                    Ok(StreamEvent::TextDelta(text)) => {
                        let chunk = ChatCompletionChunk {
                            id: id_clone.clone(),
                            object: "chat.completion.chunk".into(),
                            created,
                            model: model.clone(),
                            choices: vec![ChunkChoice {
                                index: 0,
                                delta: ChunkDelta {
                                    role: Some("assistant".into()),
                                    content: Some(text),
                                },
                                finish_reason: None,
                            }],
                        };
                        let json = serde_json::to_string(&chunk).unwrap_or_default();
                        Ok(Event::default().data(json))
                    }
                    Ok(_) => {
                        // Ignore non-text events in streaming
                        Ok(Event::default().comment(""))
                    }
                    Err(e) => Err(axum::Error::new(e)),
                }
            });

            // Append [DONE] at the end
            let stream = stream.chain(futures::stream::once(async {
                Ok(Event::default().data("[DONE]"))
            }));

            Sse::new(stream).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Provider complete() failed for streaming");
            error_response(&format!("Provider error: {e}"))
        }
    }
}

fn estimate_tokens(system: &str, messages: &[Message]) -> u32 {
    let sys_chars = system.len();
    let msg_chars: usize = messages
        .iter()
        .map(|m| {
            m.content
                .iter()
                .map(|c| match c {
                    ContentBlock::Text { text, .. } => text.len(),
                    _ => 0,
                })
                .sum::<usize>()
        })
        .sum();
    ((sys_chars + msg_chars) / 4).max(1) as u32
}

fn error_response(msg: &str) -> Response {
    let body = serde_json::json!({
        "error": {
            "message": msg,
            "type": "server_error",
        }
    });
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(body),
    )
        .into_response()
}
