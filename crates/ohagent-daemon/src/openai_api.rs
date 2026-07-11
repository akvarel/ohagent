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
use jcode_message_types::{ContentBlock, Message, Role, StreamEvent, ToolDefinition};
use serde::{Deserialize, Serialize};
use crate::api::ApiState;
use ohagent_core::agent_runner::{self, AgentEvent};

// ── /v1/models handler ──

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

#[derive(Debug, Serialize)]
pub struct ModelList {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

/// GET /v1/models — list available models from the catalog.
/// Dynamic: reads from the ModelRouter, only shows enabled models.
pub async fn list_models_handler(State(state): State<ApiState>) -> Json<ModelList> {
    let models: Vec<ModelInfo> = if let Some(ref router) = state.model_router {
        match router.lock() {
            Ok(r) => r
                .catalog()
                .iter()
                .filter(|m| r.is_enabled(&m.id))
                .map(|m| ModelInfo {
                    id: m.id.clone(),
                    object: "model".into(),
                    created: 1_700_000_000,
                    owned_by: m.provider.clone(),
                })
                .collect(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    Json(ModelList {
        object: "list".into(),
        data: models,
    })
}

// ── /v1/models/prefs handler ──

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelPref {
    pub capability: String,
    pub model_id: String,
}

#[derive(Debug, Serialize)]
pub struct ModelPrefs {
    pub tenant: String,
    pub prefs: Vec<ModelPref>,
}

/// GET /v1/models/prefs?tenant=X — get model preferences for a tenant.
pub async fn get_model_prefs(
    State(state): State<ApiState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<ModelPrefs> {
    let tenant = params.get("tenant").map(|t| t.as_str()).unwrap_or("default");

    let prefs = if let Some(ref router) = state.model_router {
        match router.lock() {
            Ok(r) => {
                let map = r.list_prefs(tenant);
                map.into_iter()
                    .map(|(capability, model_id)| ModelPref {
                        capability,
                        model_id,
                    })
                    .collect()
            }
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    Json(ModelPrefs {
        tenant: tenant.to_string(),
        prefs,
    })
}

/// POST /v1/models/prefs — set a model preference.
/// Body: {"tenant": "...", "capability": "...", "model_id": "..."}
pub async fn set_model_pref(
    State(state): State<ApiState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let tenant = body["tenant"].as_str().unwrap_or("default");
    let capability = body["capability"]
        .as_str()
        .ok_or_else(|| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "capability required"})),
            )
        })?;
    let model_id = body["model_id"].as_str().ok_or_else(|| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "model_id required"})),
        )
    })?;

    let router = state.model_router.as_ref().ok_or_else(|| {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "model router not available"})),
        )
    })?;

    match router.lock() {
        Ok(mut r) => match r.set_pref(tenant, capability, model_id) {
            Ok(()) => Ok(Json(serde_json::json!({
                "ok": true,
                "tenant": tenant,
                "capability": capability,
                "model_id": model_id
            }))),
            Err(e) => Err((
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("{}", e)})),
            )),
        },
        Err(_) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "lock failed"})),
        )),
    }
}

// ── /v1/models/status handler ──

/// GET /v1/models/status — list all models with enabled/disabled state.
pub async fn model_status_handler(
    State(state): State<ApiState>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let router = state.model_router.as_ref().ok_or_else(|| {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "model router not available"})),
        )
    })?;

    match router.lock() {
        Ok(r) => {
            let statuses: Vec<serde_json::Value> = r.model_statuses()
                .iter()
                .map(|s| serde_json::json!({
                    "id": s.id, "display": s.display,
                    "provider": s.provider, "cost_tier": s.cost_tier,
                    "enabled": s.enabled, "has_api_key": s.has_api_key,
                }))
                .collect();
            Ok(Json(serde_json::json!({"models": statuses})))
        }
        Err(_) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "lock failed"})),
        )),
    }
}

// ── /v1/models/toggle handler ──

/// POST /v1/models/toggle — enable or disable a model.
/// Body: {"model_id": "gpt-4o", "enabled": false}
pub async fn toggle_model_handler(
    State(state): State<ApiState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let model_id = body["model_id"].as_str().ok_or_else(|| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "model_id required"})),
        )
    })?;
    let enabled = body["enabled"].as_bool().unwrap_or(true);

    let router = state.model_router.as_ref().ok_or_else(|| {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "model router not available"})),
        )
    })?;

    match router.lock() {
        Ok(mut r) => match r.set_enabled(model_id, enabled) {
            Ok(()) => Ok(Json(serde_json::json!({
                "ok": true, "model_id": model_id, "enabled": enabled
            }))),
            Err(e) => Err((
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("{}", e)})),
            )),
        },
        Err(_) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "lock failed"})),
        )),
    }
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
    let input_tokens = ohagent_core::context_estimator::estimate_conversation_tokens(
        &messages, &system,
    );

    // ── Build layered system prompt (rules + skills + compressed history) ──
    let system = if let Some(ref builder) = state.system_prompt_builder {
        let budget = crate::system_prompt::PromptBudget::from_window(128_000);
        let project_dir = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."));

        // User's last message — for skills-on-demand + memory RAG
        let user_message = req.messages.last()
            .map(|m| m.content.as_str())
            .unwrap_or("");

        let compressed = state.memory.as_ref().and_then(|mem| {
            ohagent_memory::rolling_summary::load_or_create(
                mem.store(), "default", "default",
            )
            .ok()
            .and_then(|rs| if rs.compressed_history.is_empty() { None } else { Some(rs.compressed_history) })
        });

        // ── Memory RAG: search for relevant facts ──
        let rag_strings: Vec<String> = if let Some(ref mem) = state.memory {
            match mem.search("default", user_message) {
                Ok(results) if !results.is_empty() => {
                    let count = results.len();
                    let strings: Vec<String> = results
                        .into_iter()
                        .take(5) // top 5
                        .map(|r| format!("[{}] {}", r.entry.id, r.entry.content))
                        .collect();
                    tracing::info!(
                        rag_results = count,
                        "Memory RAG retrieved relevant facts"
                    );
                    strings
                }
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let assembled = builder.assemble(
            &project_dir,
            user_message,
            &system,
            compressed.as_deref(),
            &rag_strings,
            &budget,
            false,
        );

        tracing::info!(
            rules_tokens = assembled.layer_tokens.rules,
            skills_tokens = assembled.layer_tokens.skills,
            compressed_tokens = assembled.layer_tokens.compressed_history,
            needs_compression = assembled.needs_compression,
            "Layered system prompt assembled"
        );

        assembled.system
    } else {
        system
    };

    // ── Plugin pipeline: redact PII/secrets before reaching the LLM ──
    let (messages, system) = if let Some(ref pm) = state.plugin_manager {
        let user_msg = req.messages.last()
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let plugin_msg = ohagent_plugins::PluginMessage::new(
            user_msg.to_string(),
            "default".to_string(),
            "openai-api".to_string(),
        );
        let mut pipeline = pm.lock().unwrap();
        match pipeline.run_pipeline(plugin_msg) {
            Ok(Some(processed)) => {
                if !processed.redaction_log.is_empty() {
                    tracing::info!(
                        redactions = processed.redaction_log.len(),
                        plugin = "openai-api",
                        "Plugin pipeline redacted sensitive data"
                    );
                }
                if processed.text != user_msg {
                    // Rebuild messages with redacted text
                    let mut new_messages = messages;
                    if let Some(last) = new_messages.last_mut() {
                        if let Some(jcode_message_types::ContentBlock::Text { ref mut text, .. }) = last.content.first_mut() {
                            *text = processed.text;
                        }
                    }
                    (new_messages, system)
                } else {
                    (messages, system)
                }
            }
            Ok(None) => {
                tracing::warn!("Plugin pipeline blocked the message");
                return error_response("Message blocked by security policy");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Plugin pipeline error — passing through");
                (messages, system)
            }
        }
    } else {
        (messages, system)
    };

    // ── Context-aware model routing when ModelRouter is available ──
    let routed: Option<ohagent_core::model_router::RoutedModel> = if let Some(ref router) = state.model_router {
        let msg = req.messages.last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let tenant = "default"; // todo: extract from headers or user prefs
        match router.lock() {
            Ok(r) => {
                match r.route_with_messages(tenant, &msg, Some(&messages), Some(&system)) {
                    Ok(rm) => {
                        tracing::info!(
                            model = %rm.display_name,
                            context = %rm.model_id,
                            tokens_est = input_tokens,
                            "Context-aware routing selected model"
                        );
                        Some(rm)
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "route_with_messages failed — falling back to direct provider"
                        );
                        None
                    }
                }
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // ── Session heartbeat: persist active session for daemon restart recovery ──
    if let Some(ref ss) = state.session_store {
        let tenant = "default";
        // Derive stable session_hash from first user message
        let session_hash = &req.messages.first()
            .map(|m| {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                m.content.hash(&mut h);
                format!("{:x}", h.finish())
            })
            .unwrap_or_else(|| "default".into());
        let total_messages = req.messages.len() as u32;
        let _ = ss.heartbeat(tenant, session_hash, total_messages, input_tokens as u64, ".");
    }

    // ── Tool-augmented path: use agent_runner when tools are registered ──
    let tool_registry = state.tool_registry.clone();
    let has_tools = tool_registry.as_ref()
        .map(|tr| !tr.list().is_empty())
        .unwrap_or(false);

    if has_tools {
        if req.stream {
            handle_streaming_with_tools(state, req, messages, system, request_id, created).await
        } else {
            handle_non_streaming_with_tools(state, req, messages, system, request_id, created).await
        }
    } else if !req.stream
        && std::env::var("OHAGENT_CMC_ENABLED").as_deref() == Ok("1")
        && state.model_router.is_some()
    {
        // ── CMC reasoning path ──
        // Only for non-streaming requests when CMC is explicitly enabled.
        // Uses multi-branch confidence-momentum controller to reduce tokens.
        handle_cmc_reasoning(state, req, messages, system, request_id, created).await
    } else if req.stream {
        handle_streaming(state, req, messages, system, request_id, created, input_tokens, routed).await
    } else {
        handle_non_streaming(state, req, messages, system, request_id, created, input_tokens, routed)
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
    routed: Option<ohagent_core::model_router::RoutedModel>,
) -> Response {
    let provider: Arc<dyn jcode_provider_core::Provider> = if let Some(ref rm) = routed {
        Arc::clone(&rm.provider)
    } else {
        Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>
    };

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
    routed: Option<ohagent_core::model_router::RoutedModel>,
) -> Response {
    let provider: Arc<dyn jcode_provider_core::Provider> = if let Some(ref rm) = routed {
        Arc::clone(&rm.provider)
    } else {
        Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>
    };

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

// ── CMC reasoning handler ──

/// Non-streaming path through the CMC (Confidence Momentum Controller).
///
/// Spawns multiple cheap model branches, aggregates via EMA gate,
/// widens when confidence is low, and stops when answer converges.
/// Saves 30-50% tokens vs naive single-model calls.
async fn handle_cmc_reasoning(
    state: ApiState,
    req: ChatCompletionRequest,
    messages: Vec<Message>,
    system: String,
    id: String,
    created: u64,
) -> Response {
    let router = match state.model_router {
        Some(ref r) => Arc::clone(r),
        None => return error_response("CMC requires ModelRouter"),
    };

    // Build PricingRegistry from router's catalog
    let pricing = {
        let r = match router.lock() {
            Ok(r) => r,
            Err(_) => return error_response("Failed to lock model router"),
        };
        ohagent_core::pricing::PricingRegistry::from_catalog(&r.catalog())
    };

    let budget = crate::reasoning::default_cmc_budget();
    let user_message = req.messages.last()
        .map(|m| m.content.clone())
        .unwrap_or_default();

    let mut integration = crate::reasoning::CmcRouterIntegration::new(
        router,
        "default".to_string(),
        0.5, // β=0.5 — balanced between cheap and thorough
        budget,
        pricing,
    );

    tracing::info!(
        message_len = user_message.len(),
        "CMC reasoning started (β=0.5)"
    );

    match integration.reason(&user_message, 10).await {
        Ok((Some(answer), tokens)) => {
            tracing::info!(
                answer_len = answer.len(),
                total_tokens = tokens,
                "CMC reasoning completed"
            );
            let output_tokens = (answer.len() / 4).max(1) as u32;
            let response = ChatCompletionResponse {
                id,
                object: "chat.completion".into(),
                created,
                model: req.model,
                choices: vec![Choice {
                    index: 0,
                    message: ChoiceMessage {
                        role: "assistant".into(),
                        content: answer,
                    },
                    finish_reason: "stop".into(),
                }],
                usage: Usage {
                    prompt_tokens: 0,
                    completion_tokens: output_tokens,
                    total_tokens: tokens as u32,
                },
            };
            (axum::http::StatusCode::OK, Json(response)).into_response()
        }
        Ok((None, _tokens)) => {
            tracing::warn!("CMC returned no answer — falling back to direct provider");
            // Fall through to handle_non_streaming
            handle_non_streaming(state, req, messages, system, id, created, 0, None).await
        }
        Err(e) => {
            tracing::error!(error = %e, "CMC reasoning failed — falling back to direct provider");
            handle_non_streaming(state, req, messages, system, id, created, 0, None).await
        }
    }
}

// ── Tool-augmented handlers (agent_runner) ──

/// Non-streaming with tool-calling loop.
async fn handle_non_streaming_with_tools(
    state: ApiState,
    req: ChatCompletionRequest,
    messages: Vec<Message>,
    system: String,
    id: String,
    created: u64,
) -> Response {
    let tr = match state.tool_registry {
        Some(ref tr) => Arc::clone(tr),
        None => return error_response("Tool registry not available"),
    };

    let provider: Arc<dyn jcode_provider_core::Provider> = if let Some(ref router) = state.model_router {
        match router.lock() {
            Ok(r) => {
                let msg = req.messages.last().map(|m| m.content.as_str()).unwrap_or("");
                match r.route_with_messages("default", msg, Some(&messages), Some(&system)) {
                    Ok(rm) => rm.provider,
                    Err(_) => Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>,
                }
            }
            Err(_) => Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>,
        }
    } else {
        Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>
    };

    let tool_defs: Vec<ToolDefinition> = tr.list().into_iter().map(|(name, desc)| {
        let tool = tr.get(&name);
        ToolDefinition {
            name,
            description: desc,
            input_schema: tool.map(|t| t.parameters_schema.clone()).unwrap_or_default(),
        }
    }).collect();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let _handle = tokio::spawn(async move {
        agent_runner::run_agent_turn(provider, messages, system, tool_defs, tr, tx, agent_runner::ToolProgressMode::All).await
    });

    let mut content = String::new();
    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::TextDelta(text) => content.push_str(&text),
            AgentEvent::ToolCallStart { name, .. } => {
                tracing::info!(tool = %name, "Agent calling tool");
            }
            AgentEvent::ToolResult { name, output, success, .. } => {
                tracing::info!(tool = %name, success, "Tool result ({} bytes)", output.len());
            }
            AgentEvent::Error(msg) => {
                return error_response(&msg);
            }
            AgentEvent::Done { .. } => break,
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
            message: ChoiceMessage { role: "assistant".into(), content },
            finish_reason: "stop".into(),
        }],
        usage: Usage { prompt_tokens: 0, completion_tokens: output_tokens, total_tokens: output_tokens },
    };

    (axum::http::StatusCode::OK, Json(response)).into_response()
}

/// Streaming with tool-calling loop.
async fn handle_streaming_with_tools(
    state: ApiState,
    req: ChatCompletionRequest,
    messages: Vec<Message>,
    system: String,
    id: String,
    created: u64,
) -> Response {
    let tr = match state.tool_registry {
        Some(ref tr) => Arc::clone(tr),
        None => return error_response("Tool registry not available"),
    };

    let provider: Arc<dyn jcode_provider_core::Provider> = if let Some(ref router) = state.model_router {
        match router.lock() {
            Ok(r) => {
                let msg = req.messages.last().map(|m| m.content.as_str()).unwrap_or("");
                match r.route_with_messages("default", msg, Some(&messages), Some(&system)) {
                    Ok(rm) => rm.provider,
                    Err(_) => Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>,
                }
            }
            Err(_) => Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>,
        }
    } else {
        Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>
    };

    let tool_defs: Vec<ToolDefinition> = tr.list().into_iter().map(|(name, desc)| {
        let tool = tr.get(&name);
        ToolDefinition {
            name,
            description: desc,
            input_schema: tool.map(|t| t.parameters_schema.clone()).unwrap_or_default(),
        }
    }).collect();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    tokio::spawn(async move {
        let _ = agent_runner::run_agent_turn(provider, messages, system, tool_defs, tr, tx, agent_runner::ToolProgressMode::All).await;
    });

    let id_clone = id.clone();
    let model = req.model.clone();
    let stream = async_stream::stream! {
        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::TextDelta(text) => {
                    let chunk = ChatCompletionChunk {
                        id: id_clone.clone(),
                        object: "chat.completion.chunk".into(),
                        created,
                        model: model.clone(),
                        choices: vec![ChunkChoice {
                            index: 0,
                            delta: ChunkDelta { role: Some("assistant".into()), content: Some(text) },
                            finish_reason: None,
                        }],
                    };
                    let json = serde_json::to_string(&chunk).unwrap_or_default();
                    yield Ok(Event::default().data(json));
                }
                AgentEvent::ToolCallStart { name, id: tid, .. } => {
                    yield Ok(Event::default().comment(format!("tool:{name}:{tid}")));
                }
                AgentEvent::Error(msg) => {
                    yield Err(axum::Error::new(std::io::Error::new(std::io::ErrorKind::Other, msg)));
                    break;
                }
                AgentEvent::Done { .. } => break,
                _ => {}
            }
        }
        yield Ok(Event::default().data("[DONE]"));
    };

    Sse::new(stream).into_response()
}
