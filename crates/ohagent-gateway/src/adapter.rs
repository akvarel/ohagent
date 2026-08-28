//! Platform adapter trait for messaging gateways.
//!
//! Each platform (Telegram, Discord, Slack) implements this trait.
//! The gateway manager discovers and manages multiple adapters.

use std::sync::Arc;

use crate::i18n::Lang;
use ohagent_core::jcode_bridge::JcodeBridge;

/// Regex patterns for noisy status/error messages that should not be sent
/// to chat platforms (they're operational noise, not user-facing).
static NOISY_STATUS_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(
        r"(?i)(auxiliary\s+.+\s+failed)|(compression\s+summary\s+failed)|(fallback\s+context\s+marker)|(configured\s+compression\s+model\s+.+\s+failed)|(no\s+auxiliary\s+llm\s+provider\s+configured)|(auto-lowered\s+compression\s+threshold)|(compacting\s+context)|(rate\s+limited\.\s+waiting\s+\d+)|(retrying\s+in\s+\d+)|(max\s+retries\s+\(\d+\).*(?:trying\s+fallback|exhausted|invalid\s+responses))|(stream\s+(?:drop|drop\s+mid\s+tool-call).+retry\s+\d+)|(stale\s+connections\s+from\s+a\s+previous\s+provider\s+issue)"
    ).unwrap()
});

/// Check whether a status/error message is operational noise that should be
/// suppressed from chat platforms. Returns true for messages like "rate
/// limited. waiting 5" that users don't need to see.
pub fn is_noisy_status(text: &str) -> bool {
    NOISY_STATUS_RE.is_match(text)
}

/// An incoming file/photo attachment.
#[derive(Debug, Clone)]
pub struct FileAttachment {
    /// Local path where the file was saved.
    pub local_path: String,
    /// Original file name (if available).
    pub file_name: Option<String>,
    /// MIME type (if detectable).
    pub mime_type: Option<String>,
    /// File size in bytes.
    pub size_bytes: u64,
}

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
    /// Optional file attachment (photo, document, etc.).
    pub attachment: Option<FileAttachment>,
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
    /// Optional inline keyboard buttons.
    /// Each inner vec is a row of buttons.
    pub inline_keyboard: Option<Vec<Vec<InlineButton>>>,
}

impl OutgoingMessage {
    /// Create a new OutgoingMessage, filtering noisy operational status messages.
    ///
    /// If `text` matches known noisy patterns (rate limits, retries, compression
    /// events), the message text is replaced with a brief silent acknowledgement
    /// so the user isn't spammed with operational noise.
    pub fn new_filtered(chat_id: String, text: String, markdown: bool) -> Self {
        let text = if crate::adapter::is_noisy_status(&text) {
            "⋯".to_string()
        } else {
            text
        };
        Self {
            chat_id,
            text,
            markdown,
            inline_keyboard: None,
        }
    }
}

/// A button in an inline keyboard.
#[derive(Debug, Clone)]
pub struct InlineButton {
    /// Button text.
    pub text: String,
    /// Callback data sent when button is pressed.
    pub callback_data: String,
}

/// Trait that every messaging platform must implement.
#[async_trait::async_trait]
pub trait PlatformAdapter: Send + Sync {
    /// Human-readable platform name.
    fn name(&self) -> &str;

    /// Start listening for incoming messages.
    /// Receives a reference to the JcodeBridge for session management.
    async fn start(
        &self,
        bridge: Arc<JcodeBridge>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Send a message back to the platform.
    async fn send_message(
        &self,
        msg: OutgoingMessage,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Set a "typing" indicator in the chat.
    async fn set_typing(
        &self,
        chat_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
