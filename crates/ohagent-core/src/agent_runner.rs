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
    ToolCallStart { id: String, name: String },
    /// Tool execution result
    ToolResult { id: String, name: String, output: String, success: bool },
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
) -> Result<u32, String> {
    let mut current_messages = messages;
    let mut total_input_tokens: u32 = 0;
    let mut total_output_tokens: u32 = 0;

    for round in 0..MAX_TOOL_ROUNDS {
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
                Ok(StreamEvent::ToolUseStart { id, name }) => {
                    has_tools = true;
                    let _ = event_tx.send(AgentEvent::ToolCallStart {
                        id: id.clone(),
                        name: name.clone(),
                    });
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
                Ok(StreamEvent::TokenUsage { input_tokens, output_tokens, .. }) => {
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

        // If no tool calls, the turn is complete
        if !has_tools || pending_tools.is_empty() {
            // Add assistant response to messages for future turns
            // (we don't track full content here — caller handles it)
            let total = total_input_tokens + total_output_tokens;
            let _ = event_tx.send(AgentEvent::Done {
                total_tokens: total,
            });
            return Ok(total);
        }

        // Execute tools and add results to conversation
        let mut tool_result_blocks: Vec<ContentBlock> = Vec::new();
        let mut assistant_content: Vec<ContentBlock> = Vec::new();

        for tool in &pending_tools {
            // Try to parse tool input as JSON params
            let params: JsonValue = match serde_json::from_str(&tool.input) {
                Ok(v) => v,
                Err(_) => serde_json::json!({"input": tool.input}),
            };

            let result = match tool_registry.execute(&tool.name, params) {
                Some(r) => r,
                None => ToolResult {
                    success: false,
                    output: format!("Unknown tool: {}", tool.name),
                    data: None,
                    error: Some(format!("Tool '{}' not found in registry", tool.name)),
                },
            };

            let _ = event_tx.send(AgentEvent::ToolResult {
                id: tool.id.clone(),
                name: tool.name.clone(),
                output: result.output.clone(),
                success: result.success,
            });

            // Build assistant tool_use content block
            assistant_content.push(ContentBlock::ToolUse {
                id: tool.id.clone(),
                name: tool.name.clone(),
                input: serde_json::from_str(&tool.input).unwrap_or(serde_json::Value::String(tool.input.clone())),
                thought_signature: None,
            });

            // Build tool result content block
            let result_content = if result.success {
                result.output
            } else {
                format!("ERROR: {}", result.error.unwrap_or_else(|| result.output.clone()))
            };

            tool_result_blocks.push(ContentBlock::ToolResult {
                tool_use_id: tool.id.clone(),
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
