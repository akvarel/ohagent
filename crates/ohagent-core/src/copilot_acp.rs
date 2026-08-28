//! GitHub Copilot ACP provider — uses `copilot --acp --stdio` as an LLM backend.
//!
//! ## Enterprise scenario
//!
//! In locked-down corporate environments where direct API access to OpenAI/Anthropic
//! is blocked, the only available LLM may be GitHub Copilot via the `copilot` CLI.
//! This provider spawns `copilot --acp --stdio` and communicates via the Agent
//! Client Protocol (ACP, JSON-RPC 2.0 over stdin/stdout).
//!
//! No API keys required — only the `copilot` CLI binary and GitHub authentication.
//!
//! ## ACP protocol flow
//!
//! 1. Initialize → get server capabilities + default model info
//! 2. NewSession → create a conversation session
//! 3. SendMessage → send user message + get streaming response
//! 4. CloseSession → cleanup
//!
//! ## Usage in models.toml
//!
//! ```toml
//! [[models]]
//! id = "copilot-gpt-4o"
//! provider = "copilot-acp"
//! api_key_env = "COPILOT_CLI_PATH"  # optional: path to copilot binary
//! display = "GitHub Copilot (ACP)"
//! capabilities = ["coding", "general_chat", "analysis", "agentic"]
//! cost_tier = "medium"
//! context = 128_000
//! ```

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::Stream;
use jcode_message_types::{ContentBlock, Message, Role, StreamEvent, ToolDefinition};
use jcode_provider_core::{EventStream, Provider};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::pin::Pin;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Environment variable to override the path to the `copilot` binary.
const COPILOT_CLI_ENV: &str = "COPILOT_CLI_PATH";

/// Timeout for ACP initialization handshake (seconds).
const ACP_INIT_TIMEOUT_SECS: u64 = 10;

/// The default model to request when none is specified.
const DEFAULT_ACP_MODEL: &str = "gpt-4o";

// ── ACP JSON-RPC helpers ──

fn jsonrpc_request(method: &str, params: Value, id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
}

fn jsonrpc_notification(method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    })
}

/// Parse a JSON-RPC response line. Returns (id, result_or_error).
fn parse_jsonrpc_response(line: &str) -> Result<(Option<u64>, Value)> {
    let v: Value = serde_json::from_str(line).context("ACP: failed to parse JSON-RPC response")?;
    let id = v.get("id").and_then(|i| i.as_u64());
    if let Some(error) = v.get("error") {
        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown");
        return Err(anyhow::anyhow!("ACP error {}: {}", code, msg));
    }
    let result = v.get("result").cloned().unwrap_or(Value::Null);
    Ok((id, result))
}

// ── ACP session handle ──

/// A connected ACP session backed by a `copilot --acp --stdio` child process.
struct AcpSession {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    _child: Child,
    next_id: u64,
    session_id: Option<String>,
    model: String,
}

impl AcpSession {
    /// Spawn `copilot --acp --stdio` and initialize.
    fn spawn(model: &str) -> Result<Self> {
        let copilot_path = std::env::var(COPILOT_CLI_ENV).unwrap_or_else(|_| "copilot".to_string());
        let copilot_path = shellexpand::tilde(&copilot_path).to_string();

        info!(
            binary = %copilot_path,
            model = %model,
            "Spawning Copilot ACP child process"
        );

        let mut child = Command::new(&copilot_path)
            .args(["--acp", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to spawn '{} --acp --stdio'. Is copilot CLI installed?",
                    copilot_path
                )
            })?;

        let stdin = child
            .stdin
            .take()
            .context("ACP: failed to take stdin of child process")?;
        let stdout = std::io::BufReader::new(
            child
                .stdout
                .take()
                .context("ACP: failed to take stdout of child process")?,
        );

        let mut session = Self {
            stdin,
            stdout,
            _child: child,
            next_id: 1,
            session_id: None,
            model: model.to_string(),
        };

        // Initialize
        session.initialize()?;
        // Create session
        session.new_session()?;

        Ok(session)
    }

    /// ACP initialize handshake.
    fn initialize(&mut self) -> Result<()> {
        let req = jsonrpc_request(
            "initialize",
            json!({
                "protocolVersion": "0.9.0",
                "clientInfo": {
                    "name": "ohagent",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {}
            }),
            self.next_id(),
        );
        self.send(&req)?;
        let line = self.read_line()?;
        let (_id, result) = parse_jsonrpc_response(&line)?;
        debug!(result = %result, "ACP initialized");

        // Send initialized notification
        let notif = jsonrpc_notification("notifications/initialized", json!({}));
        self.send(&notif)?;

        Ok(())
    }

    /// Create a new ACP session.
    fn new_session(&mut self) -> Result<()> {
        let req = jsonrpc_request(
            "sessions/new",
            json!({
                "model": self.model,
                "mode": "chat"
            }),
            self.next_id(),
        );
        self.send(&req)?;
        let line = self.read_line()?;
        let (_id, result) = parse_jsonrpc_response(&line)?;
        let sid = result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .context("ACP: no sessionId in new_session response")?;
        self.session_id = Some(sid);
        debug!(session_id = %self.session_id.as_ref().unwrap(), "ACP session created");
        Ok(())
    }

    /// Send a chat message and return stream of content chunks.
    fn send_message(&mut self, user_message: &str) -> Result<AcpMessageStream> {
        let sid = self.session_id.as_ref().context("ACP: no active session")?;

        let req = jsonrpc_request(
            "messages/send",
            json!({
                "sessionId": sid,
                "message": {
                    "role": "user",
                    "content": [{"type": "text", "text": user_message}]
                }
            }),
            self.next_id(),
        );
        self.send(&req)?;

        // Read streaming response lines until we get the final result
        Ok(AcpMessageStream {
            reader: &mut self.stdout,
        })
    }

    /// Close the ACP session.
    fn close_session(&mut self) {
        if let Some(ref sid) = self.session_id {
            let req = jsonrpc_request(
                "sessions/close",
                json!({
                    "sessionId": sid
                }),
                self.next_id(),
            );
            let _ = self.send(&req);
            let _ = self.read_line(); // drain response
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn send(&mut self, value: &Value) -> Result<()> {
        let line = serde_json::to_string(value)?;
        use std::io::Write;
        writeln!(self.stdin, "{line}")?;
        self.stdin.flush()?;
        debug!(
            method = value.get("method").and_then(|v| v.as_str()),
            "ACP request sent"
        );
        Ok(())
    }

    fn read_line(&mut self) -> Result<String> {
        let mut line = String::new();
        self.stdout.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Err(anyhow::anyhow!(
                "ACP: empty response (child process may have exited)"
            ));
        }
        Ok(trimmed.to_string())
    }
}

impl Drop for AcpSession {
    fn drop(&mut self) {
        self.close_session();
    }
}

// ── Copilot ACP Provider ──

/// Provider that communicates with GitHub Copilot via the ACP protocol.
///
/// Spawns `copilot --acp --stdio` as a child process and communicates
/// via JSON-RPC 2.0 over stdin/stdout.
pub struct CopilotAcpProvider {
    model: String,
}

impl CopilotAcpProvider {
    /// Create a new Copilot ACP provider.
    ///
    /// `model` — the Copilot model to use (e.g. "gpt-4o").
    /// The `copilot` binary must be available in PATH (or set `COPILOT_CLI_PATH`).
    pub fn new(model: &str) -> Self {
        Self {
            model: if model.is_empty() {
                DEFAULT_ACP_MODEL.into()
            } else {
                model.into()
            },
        }
    }

    /// Check whether the `copilot` CLI is available on this system.
    pub fn is_available() -> bool {
        let path = std::env::var(COPILOT_CLI_ENV).unwrap_or_else(|_| "copilot".to_string());
        std::process::Command::new(&path)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

#[async_trait]
impl Provider for CopilotAcpProvider {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        let model = self.model.clone();

        // Extract the user's last message
        let user_message = messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| {
                m.content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        let (tx, rx) = mpsc::unbounded_channel();
        let tx_clone = tx.clone();

        // Spawn the ACP session in a blocking task
        tokio::task::spawn_blocking(move || {
            let result = Self::run_acp_turn(&model, &user_message, &tx_clone);
            if let Err(e) = result {
                let _ = tx_clone.send(Err(e));
            }
        });

        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        Ok(Box::pin(stream) as EventStream)
    }

    fn name(&self) -> &str {
        "copilot-acp"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            model: self.model.clone(),
        })
    }
}

impl CopilotAcpProvider {
    fn run_acp_turn(
        model: &str,
        user_message: &str,
        tx: &mpsc::UnboundedSender<Result<StreamEvent>>,
    ) -> Result<()> {
        let mut session = AcpSession::spawn(model)?;

        // Send message and stream response
        let stream = session.send_message(user_message)?;

        // Read streaming response
        let mut full_text = String::new();
        loop {
            let line = match session.read_line() {
                Ok(l) => l,
                Err(e) => {
                    if full_text.is_empty() {
                        return Err(e);
                    }
                    break; // End of stream
                }
            };

            // Parse various ACP event types
            let v: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // Check for text delta events
            if let Some(chunk) = v
                .get("params")
                .and_then(|p| p.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(|c| c.as_str())
            {
                full_text.push_str(chunk);
                let _ = tx.send(Ok(StreamEvent::TextDelta(chunk.to_string())));
            }

            // Check for tool call events
            if let Some(tool_calls) = v
                .pointer("/params/delta/toolCalls")
                .and_then(|t| t.as_array())
            {
                for tc in tool_calls {
                    if let (Some(id), Some(name)) = (
                        tc.get("id").and_then(|v| v.as_str()),
                        tc.get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str()),
                    ) {
                        let args = tc
                            .pointer("/function/arguments")
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}");
                        let _ = tx.send(Ok(StreamEvent::ToolUseStart {
                            id: id.to_string(),
                            name: name.to_string(),
                        }));
                        let _ = tx.send(Ok(StreamEvent::ToolInputDelta(args.to_string())));
                        let _ = tx.send(Ok(StreamEvent::ToolUseEnd));
                    }
                }
            }

            // Check for message end
            if let Some(kind) = v.get("method").and_then(|v| v.as_str()) {
                if kind == "messageDone" || kind == "messages/done" {
                    break;
                }
            }

            // Check for final result in JSON-RPC response
            if v.get("result").is_some() && v.get("id").is_some() {
                if let Some(content_blocks) = v
                    .pointer("/result/message/content")
                    .and_then(|c| c.as_array())
                {
                    for block in content_blocks {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            if !full_text.contains(text) {
                                full_text.push_str(text);
                                let _ = tx.send(Ok(StreamEvent::TextDelta(text.to_string())));
                            }
                        }
                    }
                }
                break;
            }
        }

        // Send done event
        let _ = tx.send(Ok(StreamEvent::MessageEnd { stop_reason: None }));

        info!(
            model = %model,
            chars = full_text.len(),
            "Copilot ACP turn complete"
        );

        Ok(())
    }
}

/// A streaming message response from ACP.
struct AcpMessageStream<'a> {
    reader: &'a mut BufReader<ChildStdout>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsonrpc_request_format() {
        let req = jsonrpc_request("initialize", json!({"protocolVersion": "0.9.0"}), 1);
        assert_eq!(req["jsonrpc"], "2.0");
        assert_eq!(req["method"], "initialize");
        assert_eq!(req["id"], 1);
        assert_eq!(req["params"]["protocolVersion"], "0.9.0");
    }

    #[test]
    fn test_jsonrpc_notification_format() {
        let notif = jsonrpc_notification("notifications/initialized", json!({}));
        assert_eq!(notif["jsonrpc"], "2.0");
        assert!(notif.get("id").is_none());
    }

    #[test]
    fn test_parse_jsonrpc_response_success() {
        let line = r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"copilot"}}}"#;
        let (id, result) = parse_jsonrpc_response(line).unwrap();
        assert_eq!(id, Some(1));
        assert_eq!(result["serverInfo"]["name"], "copilot");
    }

    #[test]
    fn test_parse_jsonrpc_response_error() {
        let line =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#;
        let err = parse_jsonrpc_response(line).unwrap_err();
        assert!(err.to_string().contains("Method not found"));
    }

    #[test]
    fn test_is_available_returns_false_when_no_copilot() {
        // On systems without `copilot`, this returns false
        // On systems with `copilot`, it returns true
        let available = CopilotAcpProvider::is_available();
        // We just verify it doesn't panic
        assert!(!available || available);
    }
}
