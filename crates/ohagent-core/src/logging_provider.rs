//! Logging provider — wraps a Provider to log all prompts and responses.
//!
//! ## How it works
//!
//! 1. `complete()` is intercepted — messages are converted to JSON and stored
//!    with delta compression (only new messages since the last turn).
//! 2. The returned `EventStream` is wrapped to collect `TextDelta` events.
//! 3. When the stream ends (or the wrapper is dropped), the collected response
//!    text is logged as an "assistant" entry.
//!
//! ## Per-tenant toggle
//!
//! Logging is ON by default. Tenants can disable with `/logging off`.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use async_trait::async_trait;
use futures::Stream;
use jcode_message_types::{Message, StreamEvent, ToolDefinition};
use jcode_provider_core::{EventStream, Provider};

use crate::message_log::MessageLog;

/// A Provider wrapper that logs all messages.
pub struct LoggingProvider {
    inner: Arc<dyn Provider>,
    message_log: Arc<MessageLog>,
    tenant_id: String,
    turn_seq: Arc<Mutex<u32>>,
}

impl LoggingProvider {
    pub fn new(
        inner: Arc<dyn Provider>,
        message_log: Arc<MessageLog>,
        tenant_id: String,
    ) -> Self {
        Self {
            inner,
            message_log,
            tenant_id,
            turn_seq: Arc::new(Mutex::new(0)),
        }
    }

    /// Derive a session hash from messages for delta chaining.
    fn session_hash(tenant: &str, messages: &[Message]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        tenant.hash(&mut h);
        for msg in messages.iter().take(4) {
            // Hash role + text content for stability
            format!("{:?}:{:?}", msg.role, msg.content).hash(&mut h);
        }
        format!("{:016x}", h.finish())
    }
}

#[async_trait]
impl Provider for LoggingProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn display_name(&self) -> String {
        self.inner.display_name()
    }

    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system: &str,
        resume_session_id: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        let enabled = self.message_log.is_enabled_for(&self.tenant_id);

        if enabled {
            let turn = {
                let mut seq = self.turn_seq.lock().unwrap();
                *seq += 1;
                *seq
            }; // lock dropped here

            // Convert messages to JSON for logging
            let msgs_json: Vec<serde_json::Value> = messages
                .iter()
                .map(|m| serde_json::to_value(m).unwrap_or(serde_json::Value::Null))
                .collect();

            // Log the prompt (stores delta automatically)
            let sh = Self::session_hash(&self.tenant_id, messages);
            let _ = self.message_log.log_messages(
                &self.tenant_id,
                &sh,
                "user",
                turn,
                &msgs_json,
            );

            // Call inner provider
            let inner_stream = self
                .inner
                .complete(messages, tools, system, resume_session_id)
                .await?;

            // Wrap stream to collect response
            let wrapped = LoggingStream {
                inner: inner_stream,
                collected: String::new(),
                on_done: Some(Box::new({
                    let log = Arc::clone(&self.message_log);
                    let tid = self.tenant_id.clone();
                    let t = turn;
                    let sh2 = sh.clone();
                    move |response_text: String| {
                        if !response_text.is_empty() {
                            let resp_msg = vec![serde_json::json!({
                                "role": "assistant",
                                "content": response_text,
                            })];
                            let _ = log.log_messages(&tid, &sh, "assistant", t, &resp_msg);
                        }
                    }
                })),
            };

            Ok(Box::pin(wrapped))
        } else {
            // Logging disabled — passthrough
            self.inner
                .complete(messages, tools, system, resume_session_id)
                .await
        }
    }

    async fn complete_split(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        system_static: &str,
        system_dynamic: &str,
        resume_session_id: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        // complete_split just rearranges messages — delegate to complete
        // for logging simplicity (we log the final merged messages).
        if self.message_log.is_enabled_for(&self.tenant_id) {
            self.complete(
                messages,
                tools,
                &format!("{system_static}\n{system_dynamic}"),
                resume_session_id,
            )
            .await
        } else {
            self.inner
                .complete_split(messages, tools, system_static, system_dynamic, resume_session_id)
                .await
        }
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            inner: self.inner.fork(),
            message_log: Arc::clone(&self.message_log),
            tenant_id: self.tenant_id.clone(),
            turn_seq: Arc::clone(&self.turn_seq),
        })
    }
}

/// Stream wrapper that collects TextDelta and logs on completion.
struct LoggingStream {
    inner: EventStream,
    collected: String,
    on_done: Option<Box<dyn FnOnce(String) + Send + 'static>>,
}

impl Stream for LoggingStream {
    type Item = anyhow::Result<StreamEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(StreamEvent::TextDelta(ref text)))) => {
                self.collected.push_str(text);
                Poll::Ready(Some(Ok(StreamEvent::TextDelta(text.clone()))))
            }
            Poll::Ready(Some(other)) => Poll::Ready(Some(other)),
            Poll::Ready(None) => {
                // Stream ended — execute the done callback
                if let Some(cb) = self.on_done.take() {
                    cb(std::mem::take(&mut self.collected));
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for LoggingStream {
    fn drop(&mut self) {
        // If the stream is dropped before completion, still log what we have
        if let Some(cb) = self.on_done.take() {
            if !self.collected.is_empty() {
                cb(std::mem::take(&mut self.collected));
            }
        }
    }
}
