//! Plugin manager — loads, validates, and runs the plugin pipeline.

use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use tracing::{error, info, warn};

use crate::types::{
    MessagePlugin, PluginApiVersionFn, PluginBox, PluginError, PluginFactory, PluginMessage,
    RedactionEntry, CURRENT_PLUGIN_API_VERSION,
};

/// A loaded plugin instance.
struct LoadedPlugin {
    /// The plugin name.
    name: String,
    /// The plugin implementation.
    plugin: Box<dyn MessagePlugin>,
    /// The library handle — must be kept alive.
    _library: libloading::Library,
}

/// Manages the plugin lifecycle: load, run pipeline, unload.
pub struct PluginManager {
    /// Loaded plugins in pipeline order.
    plugins: Vec<LoadedPlugin>,
    /// Plugin directory path.
    plugin_dir: PathBuf,
    /// Plugin config: which plugins to load and their settings.
    config: PluginConfig,
    /// Audit log — ring buffer of last N redaction events.
    audit_log: Vec<RedactionEntry>,
    max_audit_entries: usize,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PluginConfig {
    /// List of plugin `.so` filenames to load, in pipeline order.
    pub plugins: Vec<PluginEntry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PluginEntry {
    /// Filename of the `.so` file (e.g. "ohagent-pii-redactor.so").
    pub file: String,
    /// Whether this plugin is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Arbitrary config passed to the plugin (plugin-specific).
    #[serde(default)]
    pub config: serde_json::Value,
}

fn default_true() -> bool { true }

impl PluginManager {
    /// Create a new plugin manager.
    pub fn new(plugin_dir: PathBuf, config: PluginConfig) -> Self {
        Self {
            plugins: Vec::new(),
            plugin_dir,
            config,
            audit_log: Vec::new(),
            max_audit_entries: 1000,
        }
    }

    /// Set max audit log entries.
    pub fn with_audit_limit(mut self, limit: usize) -> Self {
        self.max_audit_entries = limit;
        self
    }

    /// Get the audit log.
    pub fn audit_log(&self) -> Vec<RedactionEntry> {
        self.audit_log.clone()
    }

    /// Clear the audit log.
    pub fn clear_audit_log(&mut self) {
        self.audit_log.clear();
    }

    /// Get plugin names in pipeline order.
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.iter().map(|p| p.name.clone()).collect()
    }

    /// Load all plugins from the configured directory.
    ///
    /// Returns the count of successfully loaded plugins.
    pub fn load_all(&mut self) -> usize {
        let mut loaded = 0;

        // Collect filenames first to avoid borrow conflict with self.load_one()
        let entries: Vec<_> = self.config.plugins.iter().filter(|e| e.enabled).cloned().collect();

        for entry in &entries {
            match self.load_one(&entry.file) {
                Ok(name) => {
                    info!(%name, file = %entry.file, "Plugin loaded");
                    loaded += 1;
                }
                Err(e) => {
                    error!(file = %entry.file, error = %e, "Failed to load plugin");
                }
            }
        }

        loaded
    }

    /// Load a single plugin `.so` file.
    fn load_one(&mut self, filename: &str) -> Result<String, String> {
        let path = self.plugin_dir.join(filename);

        // SAFETY: libloading loads native code. We validate the API version
        // before calling any other function, and wrap calls in catch_unwind.
        let library = unsafe {
            libloading::Library::new(&path)
                .map_err(|e| format!("dlopen({}): {e}", path.display()))?
        };

        // Check API version first
        let api_version: PluginApiVersionFn = unsafe {
            *library
                .get(b"plugin_api_version")
                .map_err(|e| format!("Symbol 'plugin_api_version' not found: {e}"))?
        };

        let version = unsafe { api_version() };
        if version != CURRENT_PLUGIN_API_VERSION {
            return Err(format!(
                "Plugin API version mismatch: plugin={version}, daemon={CURRENT_PLUGIN_API_VERSION}"
            ));
        }

        // Create the plugin instance
        let create: PluginFactory = unsafe {
            *library
                .get(b"create_plugin")
                .map_err(|e| format!("Symbol 'create_plugin' not found: {e}"))?
        };

        let plugin_box: PluginBox = unsafe { create() };
        let mut plugin: Box<dyn MessagePlugin> = unsafe { plugin_box.into_box() };
        let name = plugin.name().to_string();

        // Initialize the plugin
        plugin
            .init()
            .map_err(|e| format!("Plugin {} init failed: {e}", name))?;

        self.plugins.push(LoadedPlugin {
            name: name.clone(),
            plugin,
            _library: library,
        });

        Ok(name)
    }

    /// Run the full plugin pipeline on a message.
    ///
    /// Returns:
    /// - `Ok(Some(msg))` — message processed, continue to LLM
    /// - `Ok(None)` — message blocked by a plugin (fatal error)
    /// - `Err(e)` — pipeline error, message passes through unchanged
    pub fn run_pipeline(&mut self, msg: PluginMessage) -> Result<Option<PluginMessage>, String> {
        let mut msg = msg;

        for loaded in &self.plugins {
            let plugin_name = loaded.name.clone();

            // Wrap in catch_unwind — plugin panics must not crash the daemon
            let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), PluginError> {
                loaded.plugin.transform_message(&mut msg)
            }));

            match result {
                Ok(Ok(())) => {
                    if !msg.redaction_log.is_empty() {
                        info!(
                            plugin = %plugin_name,
                            total_redactions = msg.redaction_log.len(),
                            "Plugin redacted data"
                        );
                    }
                }
                Ok(Err(PluginError { message, fatal: true })) => {
                    warn!(
                        plugin = %plugin_name,
                        error = %message,
                        "Plugin blocked the message"
                    );
                    return Ok(None);
                }
                Ok(Err(PluginError { message, fatal: false })) => {
                    warn!(
                        plugin = %plugin_name,
                        error = %message,
                        "Plugin error (non-fatal) — passing through unchanged"
                    );
                    // Continue to next plugin with unchanged message
                }
                Err(panic_info) => {
                    let panic_msg = panic_info
                        .downcast_ref::<String>()
                        .map(|s| s.as_str())
                        .or_else(|| panic_info.downcast_ref::<&str>().copied())
                        .unwrap_or("unknown panic");
                    error!(
                        plugin = %plugin_name,
                        panic = %panic_msg,
                        "Plugin panicked — skipping, passing through unchanged"
                    );
                    // Continue with unchanged message
                }
            }
        }

        // Append plugin's redactions to manager's audit log
        for entry in &msg.redaction_log {
            self.audit_log.push(entry.clone());
            // Trim if over limit
            while self.audit_log.len() > self.max_audit_entries {
                self.audit_log.remove(0);
            }
        }

        Ok(Some(msg))
    }

    /// Run the response pipeline (plugins transform LLM output before user sees it).
    pub fn run_response_pipeline(&self, msg: PluginMessage) -> PluginMessage {
        let mut msg = msg;

        for loaded in &self.plugins {
            let _ = panic::catch_unwind(AssertUnwindSafe(|| {
                loaded.plugin.transform_response(&mut msg)
            }));
        }

        msg
    }
}

impl Drop for PluginManager {
    fn drop(&mut self) {
        for loaded in &mut self.plugins {
            loaded.plugin.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_pipeline() {
        let mut mgr = PluginManager::new(
            PathBuf::from("/tmp/nonexistent"),
            PluginConfig::default(),
        );
        mgr.load_all();
        let msg = PluginMessage::new("hello".into(), "t1".into(), "telegram".into());
        let result = mgr.run_pipeline(msg).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().text, "hello");
    }

    #[test]
    fn test_plugin_config_parse() {
        let json = r#"{"plugins":[{"file":"test.so","enabled":true,"config":{"threshold":0.8}}]}"#;
        let config: PluginConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.plugins.len(), 1);
        assert!(config.plugins[0].enabled);
        assert_eq!(config.plugins[0].file, "test.so");
    }
}
