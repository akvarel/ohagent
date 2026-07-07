//! Plugin type definitions — the contract between ohAgent core and plugins.

use serde::{Deserialize, Serialize};

/// Current plugin API version. Bump when the trait changes incompatibly.
pub const CURRENT_PLUGIN_API_VERSION: u32 = 1;

/// A message passing through the plugin pipeline.
#[derive(Debug, Clone)]
pub struct PluginMessage {
    /// The text content of the message (will be modified by plugins).
    pub text: String,
    /// Tenant identifier.
    pub tenant_id: String,
    /// Source platform: "telegram", "slack", "whatsapp", "openai-api", "cron".
    pub platform: String,
    /// Attached files (paths only, not content — plugins can inspect filenames).
    pub attachment_paths: Vec<String>,
    /// Running log of what was redacted/changed by plugins in this pipeline.
    pub redaction_log: Vec<RedactionEntry>,
}

impl PluginMessage {
    pub fn new(text: String, tenant_id: String, platform: String) -> Self {
        Self {
            text,
            tenant_id,
            platform,
            attachment_paths: Vec::new(),
            redaction_log: Vec::new(),
        }
    }

    /// Add a redaction entry to the audit log.
    pub fn log_redaction(&mut self, plugin: &str, original: &str, replacement: &str, reason: &str) {
        self.redaction_log.push(RedactionEntry {
            plugin: plugin.to_string(),
            field: reason.to_string(),
            original_length: original.len() as u32,
            replacement_length: replacement.len() as u32,
            timestamp: chrono::Utc::now().timestamp(),
        });
    }
}

/// A single redaction event recorded by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionEntry {
    pub plugin: String,
    /// What was redacted (e.g. "email", "credit_card", "api_key", "ip_address")
    pub field: String,
    /// Length of original text (for audit — original content is NEVER stored).
    pub original_length: u32,
    /// Length of replacement text.
    pub replacement_length: u32,
    /// Unix timestamp of the redaction.
    pub timestamp: i64,
}

/// Error returned by plugins.
#[derive(Debug)]
pub struct PluginError {
    pub message: String,
    /// If true, the pipeline stops; if false, the message passes through unchanged.
    pub fatal: bool,
}

impl PluginError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), fatal: false }
    }
    pub fn fatal(message: impl Into<String>) -> Self {
        Self { message: message.into(), fatal: true }
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// The core plugin trait. Implemented by every plugin `.so`.
///
/// # Safety
///
/// Implementations must be `Send + Sync` (plugins may be called from
/// multiple threads). Each call is wrapped in `catch_unwind` to prevent
/// panics from crashing the daemon.
pub trait MessagePlugin: Send + Sync {
    /// Human-readable plugin name.
    fn name(&self) -> &str;

    /// Plugin version as (major, minor).
    fn version(&self) -> (u32, u32) { (1, 0) }

    /// Initialize the plugin. Called once at load time.
    /// Can be used to compile regexes, connect to local services, etc.
    fn init(&mut self) -> Result<(), PluginError> { Ok(()) }

    /// Transform a message before it reaches the LLM.
    ///
    /// The plugin may:
    /// - Modify `message.text` in-place
    /// - Add entries to `message.redaction_log`
    /// - Inspect `message.attachment_paths`
    ///
    /// Return `Err(PluginError::fatal(...))` to **block** the message entirely.
    /// Return `Err(PluginError::new(...))` to pass the message through unchanged.
    /// Return `Ok(())` to pass the (possibly modified) message forward.
    fn transform_message(&self, message: &mut PluginMessage) -> Result<(), PluginError>;

    /// Transform the LLM response before it reaches the user.
    /// Default: pass-through (no modification).
    fn transform_response(&self, _message: &mut PluginMessage) -> Result<(), PluginError> {
        Ok(())
    }

    /// Called when the daemon is shutting down.
    fn shutdown(&mut self) {}
}

/// Type alias for the FFI factory function.
pub type PluginFactory = unsafe extern "C" fn() -> *mut dyn MessagePlugin;
pub type PluginApiVersionFn = unsafe extern "C" fn() -> u32;
