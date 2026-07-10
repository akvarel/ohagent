//! WebSocket real-time streaming for chat completions.
//!
//! Endpoint: `GET /v1/ws/chat` (upgrades to WebSocket)
//!
//! Uses `agent_runner` so tool calls (bash, write, edit, read, ls)
//! are visible to the client as they happen.
//!
//! ## Protocol
//!
//! Client → Server (JSON):
//! ```json
//! {"type": "chat", "model": "deepseek-chat", "messages": [...], "temperature": 0.7}
//! {"type": "cancel"}
//! ```
//!
//! Server → Client (JSON):
//! ```json
//! {"type": "token", "content": "Hello"}
//! {"type": "tool_call_start", "id": "call_1", "name": "bash", "input": "ls -la"}
//! {"type": "tool_result",  "id": "call_1", "name": "bash", "output": "total 42\n...", "success": true}
//! {"type": "done", "usage": {"prompt_tokens": 100, "completion_tokens": 50}}
//! {"type": "error", "message": "Provider error"}
//! ```

use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::ws::{Message, WebSocket},
    extract::WebSocketUpgrade,
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use jcode_message_types::{ContentBlock, Message as JcodeMessage, Role, ToolDefinition};
use tokio::sync::mpsc;

use crate::api::ApiState;
use ohagent_core::agent_runner::{self, AgentEvent};

/// Handle WebSocket upgrade and spawn the streaming loop.
pub async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<ApiState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Cancel signal sent from reader task to writer task.
enum WsCommand {
    Cancel,
    Chat {
        model: String,
        messages: Vec<OpenAiWsMessage>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    },
}

#[derive(Debug, serde::Deserialize)]
struct OpenAiWsMessage {
    role: String,
    content: String,
}

async fn handle_ws(ws: WebSocket, state: ApiState) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<WsCommand>();

    // Reader task: parse incoming JSON and send commands
    let mut read_handle = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(text) => {
                    let cmd = match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(val) => {
                            match val["type"].as_str() {
                                Some("chat") => {
                                    let model = val["model"].as_str().unwrap_or("deepseek-chat");
                                    let messages: Vec<OpenAiWsMessage> =
                                        serde_json::from_value(val["messages"].clone())
                                            .unwrap_or_default();
                                    let temperature = val["temperature"].as_f64().map(|t| t as f32);
                                    let max_tokens = val["max_tokens"].as_u64().map(|t| t as u32);
                                    WsCommand::Chat {
                                        model: model.to_string(),
                                        messages,
                                        temperature,
                                        max_tokens,
                                    }
                                }
                                Some("cancel") => WsCommand::Cancel,
                                _ => continue,
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Invalid WebSocket message");
                            continue;
                        }
                    };
                    if cmd_tx.send(cmd).is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Writer task: process commands and stream agent_runner events
    let mut write_task = tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                WsCommand::Cancel => {
                    send_json(&mut ws_tx, &serde_json::json!({"type": "cancelled"})).await;
                }
                WsCommand::Chat { model: _model, messages, temperature, max_tokens } => {
                    let (jcode_msgs, mut system) = convert_messages(&messages);

                    // Build system prompt (AGENTS.md, memory context, skills)
                    let system = if let Some(ref builder) = state.system_prompt_builder {
                        let budget = crate::system_prompt::PromptBudget::from_window(128_000);
                        let project_dir = std::env::current_dir()
                            .unwrap_or_else(|_| std::path::PathBuf::from("."));

                        let user_msg = messages.last()
                            .map(|m| m.content.as_str())
                            .unwrap_or("");

                        let compressed = state.memory.as_ref().and_then(|mem| {
                            ohagent_memory::rolling_summary::load_or_create(
                                mem.store(), "default", "default",
                            ).ok()
                            .and_then(|rs| if rs.compressed_history.is_empty() { None } else { Some(rs.compressed_history) })
                        });

                        let rag_strings: Vec<String> = if let Some(ref mem) = state.memory {
                            mem.search("default", user_msg).ok()
                                .map(|r| r.into_iter().take(5)
                                    .map(|r| format!("[{}] {}", r.entry.id, r.entry.content))
                                    .collect())
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        };

                        let assembled = builder.assemble(
                            &project_dir, user_msg, &system,
                            compressed.as_deref(), &rag_strings, &budget,
                        );
                        assembled.system
                    } else {
                        system
                    };

                    let started = Instant::now();
                    let input_tokens = ohagent_core::context_estimator::estimate_conversation_tokens(
                        &jcode_msgs, &system,
                    );

                    // Session heartbeat
                    if let Some(ref ss) = state.session_store {
                        let tenant = "default";
                        let shash = &messages.first()
                            .map(|m| {
                                use std::hash::{Hash, Hasher};
                                let mut h = std::collections::hash_map::DefaultHasher::new();
                                m.content.hash(&mut h);
                                format!("{:x}", h.finish())
                            })
                            .unwrap_or_else(|| "default".into());
                        let _ = ss.heartbeat(tenant, shash, messages.len() as u32, input_tokens as u64, ".");
                    }

                    // Resolve provider
                    let provider: Arc<dyn jcode_provider_core::Provider> = if let Some(ref router) = state.model_router {
                        match router.lock() {
                            Ok(r) => {
                                let msg = messages.last()
                                    .map(|m| m.content.as_str())
                                    .unwrap_or("");
                                match r.route_with_messages("default", msg, Some(&jcode_msgs), Some(&system)) {
                                    Ok(rm) => rm.provider,
                                    Err(_) => Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>,
                                }
                            }
                            Err(_) => Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>,
                        }
                    } else {
                        Arc::clone(state.bridge.provider()) as Arc<dyn jcode_provider_core::Provider>
                    };

                    let _ = (temperature, max_tokens);

                    // Build tool definitions from the tool registry
                    let tool_defs: Vec<ToolDefinition> = if let Some(ref tr) = state.tool_registry {
                        tr.list().into_iter().map(|(name, desc)| {
                            let schema = tr.get(&name)
                                .map(|t| t.parameters_schema.clone())
                                .unwrap_or(serde_json::Value::Null);
                            ToolDefinition {
                                name,
                                description: desc,
                                input_schema: schema,
                            }
                        }).collect()
                    } else {
                        Vec::new()
                    };

                    // Track total response text for done event
                    let mut total_content = String::new();

                    // Run agent_runner (supports tool calls)
                    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();

                    let runner_provider = Arc::clone(&provider);
                    let runner_msgs = jcode_msgs.clone();
                    let runner_system = system.clone();
                    let runner_defs = tool_defs.clone();
                    let runner_tr = state.tool_registry.clone()
                        .unwrap_or_else(|| Arc::new(ohagent_core::tools::ToolRegistry::new()));

                    let mut first_event = true;
                    let handle = tokio::spawn(async move {
                        agent_runner::run_agent_turn(
                            runner_provider,
                            runner_msgs,
                            runner_system,
                            runner_defs,
                            runner_tr,
                            event_tx,
                        ).await
                    });

                    // Drain agent events and send to client
                    while let Some(event) = event_rx.recv().await {
                        // Check for cancel
                        if let Ok(cancel_cmd) = cmd_rx.try_recv() {
                            if matches!(cancel_cmd, WsCommand::Cancel) {
                                send_json(&mut ws_tx, &serde_json::json!({
                                    "type": "cancelled",
                                    "partial_content": total_content,
                                })).await;
                                return;
                            }
                        }

                        match event {
                            AgentEvent::TextDelta(text) => {
                                if first_event {
                                    first_event = false;
                                    send_json(&mut ws_tx, &serde_json::json!({
                                        "type": "started",
                                        "took_ms": started.elapsed().as_millis(),
                                    })).await;
                                }
                                total_content.push_str(&text);
                                send_json(&mut ws_tx, &serde_json::json!({
                                    "type": "token",
                                    "content": text,
                                })).await;
                            }
                            AgentEvent::ToolCallStart { id, name, .. } => {
                                send_json(&mut ws_tx, &serde_json::json!({
                                    "type": "tool_call_start",
                                    "id": id,
                                    "name": name,
                                })).await;
                            }
                            AgentEvent::ToolResult { id, name, output, success } => {
                                send_json(&mut ws_tx, &serde_json::json!({
                                    "type": "tool_result",
                                    "id": id,
                                    "name": name,
                                    "output": output,
                                    "success": success,
                                })).await;
                            }
                            AgentEvent::Done { total_tokens } => {
                                let elapsed_ms = started.elapsed().as_millis();
                                let tokens_per_sec = if elapsed_ms > 0 {
                                    (total_tokens as f64 / (elapsed_ms as f64 / 1000.0)) as u32
                                } else {
                                    0
                                };
                                send_json(&mut ws_tx, &serde_json::json!({
                                    "type": "done",
                                    "content": total_content,
                                    "usage": {
                                        "prompt_tokens": input_tokens,
                                        "completion_tokens": total_tokens.saturating_sub(input_tokens),
                                        "total_tokens": total_tokens,
                                    },
                                    "took_ms": elapsed_ms,
                                    "tokens_per_sec": tokens_per_sec,
                                })).await;
                            }
                            AgentEvent::Error(message) => {
                                send_json(&mut ws_tx, &serde_json::json!({
                                    "type": "error",
                                    "message": message,
                                })).await;
                                return;
                            }
                        }
                    }

                    // Await runner completion
                    if let Err(e) = handle.await {
                        send_json(&mut ws_tx, &serde_json::json!({
                            "type": "error",
                            "message": format!("Agent runner error: {e}"),
                        })).await;
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = &mut read_handle => { write_task.abort(); }
        _ = &mut write_task => { read_handle.abort(); }
    }
}

async fn send_json(
    tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    value: &serde_json::Value,
) {
    let text = serde_json::to_string(value).unwrap_or_default();
    let _ = tx.send(Message::Text(text.into())).await;
}

/// Convert OpenAI messages to jcode Message format.
fn convert_messages(openai_msgs: &[OpenAiWsMessage]) -> (Vec<JcodeMessage>, String) {
    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<JcodeMessage> = Vec::new();

    for m in openai_msgs {
        match m.role.as_str() {
            "system" => {
                system_parts.push(m.content.clone());
            }
            "user" => {
                messages.push(JcodeMessage {
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
                messages.push(JcodeMessage {
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
                messages.push(JcodeMessage {
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
