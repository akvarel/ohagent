//! Plugin type definitions — the contract between ohAgent core and plugins.

use serde::{Deserialize, Serialize};
use std::ffi::c_void;

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

/// FFI-safe wrapper around `Box<dyn MessagePlugin>`.
///
/// # Safety justification
///
/// On all Rust targets, a `*mut dyn Trait` fat pointer consists of two
/// pointer-width words: the data pointer and the vtable pointer.
/// `#[repr(C)] PluginBox` has the same layout. Passing it by value
/// across `extern "C"` is well-defined on all platforms Rust supports:
///
/// | ABI | Struct return strategy |
/// |---|---|
/// | SysV x86_64 | ≤16 bytes in RAX:RDX |
/// | Windows x64 | hidden pointer (>8 bytes) |
/// | aarch64 | ≤16 bytes in x0:x1 |
#[repr(C)]
pub struct PluginBox {
    data: *mut c_void,
    vtable: *mut c_void,
}

impl PluginBox {
    /// Wrap a `Box<dyn MessagePlugin>` into an FFI-safe representation.
    pub fn from_box(plugin: Box<dyn MessagePlugin>) -> Self {
        let fat = Box::into_raw(plugin); // *mut dyn MessagePlugin
        unsafe {
            // SAFETY: *mut dyn MessagePlugin has the same in-memory
            // representation as two consecutive *mut c_void (data + vtable)
            // on all supported targets. This transmute is equivalent to
            // reinterpreting the two-pointer fat pointer as a struct.
            let [data, vtable]: [*mut c_void; 2] = std::mem::transmute(fat);
            Self { data, vtable }
        }
    }

    /// Unwrap back to `Box<dyn MessagePlugin>`.
    ///
    /// # Safety
    ///
    /// Must have been created by `PluginBox::from_box` with a valid
    /// `Box<dyn MessagePlugin>` of the same trait.
    pub unsafe fn into_box(self) -> Box<dyn MessagePlugin> {
        // SAFETY: Reconstruct the fat pointer from data + vtable ptrs.
        let raw: *mut dyn MessagePlugin = std::mem::transmute([self.data, self.vtable]);
        Box::from_raw(raw)
    }
}

/// FFI-safe factory function. Returns a `PluginBox` instead of
/// `*mut dyn MessagePlugin` to avoid undefined behaviour (trait
/// objects have no stable C ABI).
pub type PluginFactory = unsafe extern "C" fn() -> PluginBox;
pub type PluginApiVersionFn = unsafe extern "C" fn() -> u32;
