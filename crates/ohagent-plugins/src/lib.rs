//! ohAgent Plugin System — safe, versioned, chainable message processing.
//!
//! # Architecture
//!
//! Plugins are dynamically loaded `.so`/`.dylib`/`.dll` files that implement
//! the `MessagePlugin` trait. They intercept and transform messages before
//! they reach the LLM provider.
//!
//! ```text
//! User message → [Plugin₁] → [Plugin₂] → ... → [Pluginₙ] → JcodeBridge → LLM
//! ```
//!
//! # Commercial Plugin Model
//!
//! - ohAgent core + plugin system = **open source** (Apache 2.0)
//! - Individual plugins can be **closed-source proprietary** binaries
//! - Enterprise: ship pre-loaded Docker images with licensed plugins
//! - All processing is local — no data leaves the machine to plugin vendors
//!
//! # Writing a Plugin
//!
//! 1. Create a `cdylib` crate depending on `ohagent-plugins`
//! 2. Implement the `MessagePlugin` trait on a struct
//! 3. Export two `extern "C"` symbols:
//!    - `plugin_api_version() -> u32`
//!    - `create_plugin() -> *mut dyn MessagePlugin`
//! 4. Place the `.so` in the plugin directory
//!
//! # Example: PII Redaction Plugin (proprietary)
//!
//! ```rust,ignore
//! // ohagent-pii-redactor/src/lib.rs
//! use ohagent_plugins::*;
//!
//! pub struct PiiRedactorPlugin { /* regex patterns, secret lists */ }
//!
//! impl MessagePlugin for PiiRedactorPlugin {
//!     fn name(&self) -> &str { "pii-redactor" }
//!     fn transform_message(&self, msg: &mut PluginMessage) -> Result<(), PluginError> {
//!         // Replace emails, credit cards, API keys with [REDACTED]
//!         // ...
//!         Ok(())
//!     }
//! }
//!
//! #[no_mangle]
//! pub extern "C" fn plugin_api_version() -> u32 { CURRENT_PLUGIN_API_VERSION }
//!
//! #[no_mangle]
//! pub extern "C" fn create_plugin() -> *mut dyn MessagePlugin {
//!     Box::into_raw(Box::new(PiiRedactorPlugin::new()))
//! }
//! ```

pub mod manager;
pub mod types;

pub use manager::{PluginConfig, PluginEntry, PluginManager};
pub use types::*;
