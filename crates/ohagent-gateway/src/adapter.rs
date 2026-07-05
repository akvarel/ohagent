//! Platform adapter trait for messaging gateways.
//!
//! Each platform (Telegram, Discord, Slack) implements this trait.
//! The gateway manager discovers and manages multiple adapters.

use std::sync::Arc;

use ohagent_core::jcode_bridge::JcodeBridge;
use crate::i18n::Lang;

/// Information about an incoming message from any platform.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// Unique chat/conversation identifier (platform-scoped).
    pub chat_id: String,
    /// The sender's platform-scoped user ID.
    pub user_id: String,
    /// The tenant identifier (derived from user/chat).
    pub tenant_id: String,
    /// Text content of the message.
    pub text: String,
    /// Language preference of the sender.
    pub lang: Lang,
    /// Platform name (e.g. "telegram", "discord").
    pub platform: String,
}

/// An outgoing message to be sent to a platform.
#[derive(Debug, Clone)]
pub struct OutgoingMessage {
    /// Target chat ID.
    pub chat_id: String,
    /// Message text (may contain Markdown formatting).
    pub text: String,
    /// Whether to parse as MarkdownV2.
    pub markdown: bool,
}

/// Trait that every messaging platform must implement.
#[async_trait::async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// Human-readable platform name.
    fn name(&self) -> &str;

    /// Start listening for incoming messages.
    /// Receives a reference to the JcodeBridge for session management.
    async fn start(&self, bridge: Arc<JcodeBridge>) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Send a message back to the platform.
    async fn send_message(&self, msg: OutgoingMessage) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Set a "typing" indicator in the chat.
    async fn set_typing(&self, chat_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _ = chat_id;
        Ok(())
    }

    /// Get the platform prefix for session IDs.
    fn session_prefix(&self) -> String {
        format!("{}:", self.name())
    }

    /// Build a session ID from platform name and chat ID.
    fn session_id_for(&self, chat_id: &str) -> String {
        format!("{}{}", self.session_prefix(), chat_id)
    }
}
