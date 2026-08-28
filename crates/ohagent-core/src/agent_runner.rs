//! Agent runner — tool-executing agent loop for chat completions.
//!
//! Bridges the gap between simple chat (provider.complete()) and
//! tool-augmented agent turns. When tools are registered, the runner
//! passes them to the provider, executes tool calls, feeds results back,
//! and repeats until the LLM produces a final text response.
//!
//! ## Flow
//!
//! ```text
//! User message → Provider.complete(messages, tools) → StreamEvent::ToolUse*
//!                                              │
//!                                   ┌──────────▼──────────┐
//!                                   │ Execute tool locally│
//!                                   │ (bash, write, edit) │
//!                                   └──────────┬──────────┘
//!                                              │
//!                Provider.complete(messages + tool_result, tools) → repeat
//!                                              │
//!                                   ┌──────────▼──────────┐
//!                                   │ TextDelta → stream  │
//!                                   │ to user             │
//!                                   └─────────────────────┘
//! ```

use std::sync::Arc;

use futures::StreamExt;
use jcode_message_types::{ContentBlock, Message, Role, StreamEvent, ToolDefinition};
use jcode_provider_core::Provider as ProviderTrait;
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;

use crate::tools::{ToolRegistry, ToolResult};

/// Max tool-calling rounds before giving up (safety limit).
const MAX_TOOL_ROUNDS: usize = 5;

/// Max empty-response retries before giving up.
/// The model may produce no visible output (reasoning-only / guardrail).
/// We auto-inject a "continue" prompt up to this many times.
const MAX_EMPTY_RETRIES: u32 = 3;

/// Controls what tool execution information is streamed to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolProgressMode {
    /// Stream everything: tool call start, tool result, text deltas
    All,
    /// Stream only the final result, no intermediate tool call events
    None,
    /// Stream tool call start metadata but suppress large tool result outputs
    StreamingOnly,
}

impl Default for ToolProgressMode {
    fn default() -> Self {
        Self::All
    }
}

impl ToolProgressMode {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "none" => Self::None,
            "streaming" | "streaming-only" => Self::StreamingOnly,
            _ => Self::All,
        }
    }
}

/// Internal struct tracking an in-progress tool call while streaming.
struct PendingTool {
    id: String,
    name: String,
    input: String,
}

/// Events emitted during an agent turn.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Text delta — stream to user
    TextDelta(String),
    /// Tool call started
    ToolCallStart {
        id: String,
        name: String,
        input: String,
    },
    /// Tool execution result
    ToolResult {
        id: String,
        name: String,
        output: String,
        success: bool,
    },
    /// Turn complete
    Done { total_tokens: u32 },
    /// Error
    Error(String),
}

/// Run a tool-augmented agent turn.
///
/// This function:
/// 1. Passes `messages + tools` to the provider
/// 2. If the provider returns tool calls, executes them via `tool_registry`
/// 3. Feeds tool results back as new messages and repeats
/// 4. Streams text deltas to `event_tx`
///
/// Returns the total token count used.
pub async fn run_agent_turn(
    provider: Arc<dyn ProviderTrait>,
    messages: Vec<Message>,
    system: String,
    tool_defs: Vec<ToolDefinition>,
    tool_registry: Arc<ToolRegistry>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    tool_progress_mode: ToolProgressMode,
) -> Result<u32, String> {
    let mut current_messages = messages;
    let mut total_input_tokens: u32 = 0;
    let mut total_output_tokens: u32 = 0;
    let mut empty_retries: u32 = 0;

    for _round in 0..MAX_TOOL_ROUNDS {
        let stream = provider
            .complete(&current_messages, &tool_defs, &system, None)
            .await
            .map_err(|e| format!("Provider error: {e}"))?;

        let mut pending_tools: Vec<PendingTool> = Vec::new();
        let mut current_tool: Option<PendingTool> = None;
        let mut tool_input_buffer = String::new();
        let mut has_text = false;
        let mut has_tools = false;

        let mut stream = Box::pin(stream);
        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::TextDelta(text)) => {
                    has_text = true;
                    let _ = event_tx.send(AgentEvent::TextDelta(text));
                }
                Ok(StreamEvent::ToolUseStart { id, name })
                    if tool_progress_mode == ToolProgressMode::None =>
                {
                    has_tools = true;
                    current_tool = Some(PendingTool {
                        id,
                        name,
                        input: String::new(),
                    });
                    tool_input_buffer.clear();
                }
                Ok(StreamEvent::ToolUseStart { id, name }) => {
                    has_tools = true;
                    if tool_progress_mode != ToolProgressMode::None {
                        let _ = event_tx.send(AgentEvent::ToolCallStart {
                            id: id.clone(),
                            name: name.clone(),
                            input: String::new(),
                        });
                    }
                    current_tool = Some(PendingTool {
                        id,
                        name,
                        input: String::new(),
                    });
                    tool_input_buffer.clear();
                }
                Ok(StreamEvent::ToolInputDelta(delta)) => {
                    tool_input_buffer.push_str(&delta);
                }
                Ok(StreamEvent::ToolUseEnd) => {
                    if let Some(mut tool) = current_tool.take() {
                        tool.input = std::mem::take(&mut tool_input_buffer);
                        pending_tools.push(tool);
                    }
                }
                Ok(StreamEvent::MessageEnd { .. }) => {
                    break;
                }
                Ok(StreamEvent::TokenUsage {
                    input_tokens,
                    output_tokens,
                    ..
                }) => {
                    total_input_tokens += input_tokens.unwrap_or(0) as u32;
                    total_output_tokens += output_tokens.unwrap_or(0) as u32;
                }
                Ok(StreamEvent::Error { message, .. }) => {
                    let _ = event_tx.send(AgentEvent::Error(message.clone()));
                    return Err(message);
                }
                Ok(StreamEvent::RetryRollback { .. }) => {
                    // Discard partial tool calls on retry
                    current_tool = None;
                    tool_input_buffer.clear();
                }
                Err(e) => {
                    let msg = format!("Stream error: {e}");
                    let _ = event_tx.send(AgentEvent::Error(msg.clone()));
                    return Err(msg);
                }
                _ => {}
            }
        }

        // If no tool calls, the turn is complete — unless it was an empty
        // response with only reasoning (provider-side guardrail or silent
        // filter). In that case, inject a "continue" prompt automatically
        // instead of returning an empty Done.
        let empty_silent_response = !has_text && pending_tools.is_empty();
        if !has_tools || pending_tools.is_empty() {
            if empty_silent_response {
                // The model produced no visible output — likely a provider-side
                // guardrail or reasoning-only response. Inject a continuation
                // prompt to nudge the model to actually respond.
                empty_retries += 1;
                if empty_retries > MAX_EMPTY_RETRIES {
                    let msg = format!(
                        "Model returned empty response {} times (reasoning-only / guardrail). \
                         Rephrasing the request may help.",
                        empty_retries
                    );
                    tracing::warn!(msg);
                    let _ = event_tx.send(AgentEvent::Error(msg.clone()));
                    return Err(msg);
                }
                tracing::debug!(
                    retry = empty_retries,
                    max = MAX_EMPTY_RETRIES,
                    "Empty silent response — injecting continuation prompt"
                );
                current_messages.push(Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Continue. Please provide your response now.".to_string(),
                        cache_control: None,
                    }],
                    timestamp: None,
                    tool_duration_ms: None,
                });
                continue; // Go to next round with the continuation prompt
            }
            // Normal completion — the model produced text or explicitly ended
            let total = total_input_tokens + total_output_tokens;
            let _ = event_tx.send(AgentEvent::Done {
                total_tokens: total,
            });
            return Ok(total);
        }

        // Execute tools in parallel using spawn_blocking for sync handlers
        let mut tool_result_blocks: Vec<ContentBlock> = Vec::new();
        let mut assistant_content: Vec<ContentBlock> = Vec::new();

        // Map tool IDs to their raw input for assistant content construction
        let mut tool_input_by_id: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for tool in &pending_tools {
            tool_input_by_id.insert(tool.id.clone(), tool.input.clone());
        }

        // Build parallel tasks for independent tool calls
        let tool_tasks: Vec<_> = pending_tools
            .iter()
            .map(|tool| {
                let tool_registry = Arc::clone(&tool_registry);
                let tool_name = tool.name.clone();
                let tool_id = tool.id.clone();
                let tool_input = tool.input.clone();

                tokio::task::spawn_blocking(move || {
                    let params: JsonValue = match serde_json::from_str(&tool_input) {
                        Ok(v) => v,
                        Err(_) => serde_json::json!({"input": tool_input}),
                    };

                    let result = match tool_registry.execute(&tool_name, params) {
                        Some(r) => r,
                        None => ToolResult {
                            success: false,
                            output: format!("Unknown tool: {}", tool_name),
                            data: None,
                            error: Some(format!("Tool '{}' not found in registry", tool_name)),
                        },
                    };

                    (tool_id, tool_name, result)
                })
            })
            .collect();

        // Await all parallel tool executions
        let tool_results = futures::future::join_all(tool_tasks).await;

        for tool_result in tool_results {
            let (tool_id, tool_name, result) = match tool_result {
                Ok(r) => r,
                Err(e) => {
                    let msg = format!("Tool thread panicked: {e}");
                    let _ = event_tx.send(AgentEvent::Error(msg.clone()));
                    continue;
                }
            };

            if tool_progress_mode == ToolProgressMode::All {
                let _ = event_tx.send(AgentEvent::ToolResult {
                    id: tool_id.clone(),
                    name: tool_name.clone(),
                    output: result.output.clone(),
                    success: result.success,
                });
            } else if tool_progress_mode == ToolProgressMode::StreamingOnly {
                // Truncate large outputs for streaming-only mode
                let preview = if result.output.len() > 200 {
                    format!(
                        "{}... [{} bytes total]",
                        &result.output[..200],
                        result.output.len()
                    )
                } else {
                    result.output.clone()
                };
                let _ = event_tx.send(AgentEvent::ToolResult {
                    id: tool_id.clone(),
                    name: tool_name.clone(),
                    output: preview,
                    success: result.success,
                });
            }

            // Build assistant tool_use content block
            let tool_input_str = tool_input_by_id
                .get(&tool_id)
                .map(|s| s.as_str())
                .unwrap_or("");
            assistant_content.push(ContentBlock::ToolUse {
                id: tool_id.clone(),
                name: tool_name.clone(),
                input: serde_json::from_str(tool_input_str)
                    .unwrap_or(serde_json::Value::String(tool_input_str.to_string())),
                thought_signature: None,
            });

            // Build tool result content block
            let result_content = if result.success {
                result.output
            } else {
                format!(
                    "ERROR: {}",
                    result.error.unwrap_or_else(|| result.output.clone())
                )
            };

            tool_result_blocks.push(ContentBlock::ToolResult {
                tool_use_id: tool_id.clone(),
                content: result_content,
                is_error: Some(!result.success),
            });
        }

        // Add assistant message with tool calls
        current_messages.push(Message {
            role: Role::Assistant,
            content: assistant_content,
            timestamp: None,
            tool_duration_ms: None,
        });

        // Add tool results as user message (per OpenAI/Jcode convention)
        current_messages.push(Message {
            role: Role::User,
            content: tool_result_blocks,
            timestamp: None,
            tool_duration_ms: None,
        });
    }

    // Max rounds exceeded
    let msg = format!("Max tool rounds ({MAX_TOOL_ROUNDS}) exceeded");
    let _ = event_tx.send(AgentEvent::Error(msg.clone()));
    Err(msg)
}
