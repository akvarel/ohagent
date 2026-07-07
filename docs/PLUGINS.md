# ohAgent Plugin Development Guide

Build third-party plugins for ohAgent — message filtering, custom model deployment,
audit logging, and more. The plugin system uses `.so` dynamic libraries loaded at runtime.

---

## 1. Architecture

```text
┌─────────────────────────────────────────────────────┐
│  ohAgent Daemon                                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────┐  │
│  │Telegram  │  │ REST API │  │ Cron Scheduler   │  │
│  └────┬─────┘  └────┬─────┘  └────────┬─────────┘  │
│       │              │               │              │
│       └──────────────┼───────────────┘              │
│                      ▼                              │
│             ┌────────────────┐                      │
│             │ PluginManager  │                      │
│             │ dlopen .so     │                      │
│             └───────┬────────┘                      │
│                     │                               │
│       ┌─────────────┼─────────────┐                │
│       ▼             ▼             ▼                │
│  ┌─────────┐  ┌──────────┐  ┌──────────┐          │
│  │Plugin₁  │  │Plugin₂   │  │Pluginₙ   │          │
│  │.so      │  │.so       │  │.so       │          │
│  └─────────┘  └──────────┘  └──────────┘          │
│                                                     │
│  message → plugin₁ → plugin₂ → ... → LLM Provider  │
└─────────────────────────────────────────────────────┘
```

## 2. Quick Start

### Create a plugin crate

```bash
cargo new --lib my-ohagent-plugin
cd my-ohagent-plugin
```

### Cargo.toml

```toml
[package]
name = "ohagent-my-plugin"
version = "0.1.0"
edition = "2021"
license = "Proprietary"  # or Apache-2.0 for open-source

[lib]
crate-type = ["cdylib"]  # MUST be cdylib — produces .so file

[dependencies]
ohagent-plugins = "1"    # The plugin SDK
serde_json = "1"          # For custom config
tracing = "0.1"           # Logging (optional)
```

### src/lib.rs (skeleton)

```rust
use ohagent_plugins::*;

pub struct MyPlugin {
    // Your state: compiled regexes, database connections, etc.
}

impl MyPlugin {
    pub fn new() -> Self {
        Self {}
    }
}

impl MessagePlugin for MyPlugin {
    fn name(&self) -> &str {
        "ohagent-my-plugin"
    }

    fn version(&self) -> (u32, u32) {
        (1, 0)
    }

    fn init(&mut self) -> Result<(), PluginError> {
        // Called once at load time. Compile regexes, connect to local DB, etc.
        tracing::info!("MyPlugin initialized");
        Ok(())
    }

    fn transform_message(&self, message: &mut PluginMessage) -> Result<(), PluginError> {
        // Modify message.text in-place
        // Add entries to message.redaction_log for audit
        //
        // Return Err(PluginError::fatal("reason")) to BLOCK the message
        // Return Err(PluginError::new("reason")) to pass through unchanged
        // Return Ok(()) to pass the modified message forward
        Ok(())
    }

    fn transform_response(&self, message: &mut PluginMessage) -> Result<(), PluginError> {
        // Optional: transform LLM output before user sees it
        Ok(())
    }

    fn shutdown(&mut self) {
        // Cleanup: close connections, flush buffers
    }
}

// ── FFI exports (required) ──

#[no_mangle]
pub extern "C" fn plugin_api_version() -> u32 {
    CURRENT_PLUGIN_API_VERSION
}

#[no_mangle]
pub extern "C" fn create_plugin() -> *mut dyn MessagePlugin {
    Box::into_raw(Box::new(MyPlugin::new()))
}
```

### Build and deploy

```bash
cargo build --release
cp target/release/libmy_ohagent_plugin.so ~/.ohagent/plugins/
```

### Enable in config

`~/.ohagent/plugins.toml`:

```toml
[[plugins]]
file = "libmy_ohagent_plugin.so"
enabled = true
config = {}  # Plugin-specific settings, accessible via PluginEntry.config
```

---

## 3. The MessagePlugin Trait

```rust
pub trait MessagePlugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> (u32, u32) { (1, 0) }
    fn init(&mut self) -> Result<(), PluginError> { Ok(()) }
    fn transform_message(&self, message: &mut PluginMessage) -> Result<(), PluginError>;
    fn transform_response(&self, message: &mut PluginMessage) -> Result<(), PluginError> { Ok(()) }
    fn shutdown(&mut self) {}
}
```

### Lifecycle

1. `create_plugin()` — called by `dlopen`, creates your struct
2. `name()` / `version()` — metadata
3. `init()` — initialization (validate license, compile regex, connect to DB)
4. `transform_message()` — called on EVERY message before it reaches the LLM
5. `transform_response()` — called on EVERY LLM response before it reaches the user
6. `shutdown()` — cleanup when daemon stops

### Safety guarantees

- **catch_unwind**: Every `transform_*` call is wrapped in `std::panic::catch_unwind`. Your plugin panic will NOT crash the daemon.
- **Version check**: `plugin_api_version()` is validated before ANY function is called.
- **Thread safety**: Plugins may be called from multiple threads. Use `Mutex`/`RwLock` for interior state.
- **Local processing**: Plugins run on-device. Data does NOT leave the machine to plugin vendors.

---

## 4. PluginMessage

```rust
pub struct PluginMessage {
    pub text: String,                    // Message content (mutable by plugins)
    pub tenant_id: String,               // Tenant identifier
    pub platform: String,                // "telegram", "slack", "openai-api", "cron"
    pub attachment_paths: Vec<String>,   // Local file paths (plugins inspect names only)
    pub redaction_log: Vec<RedactionEntry>, // Audit log entries
}

pub struct RedactionEntry {
    pub plugin: String,          // Plugin name
    pub field: String,           // What was redacted: "email", "api_key", "ssn"
    pub original_length: u32,   // Size of original text (content NOT stored)
    pub replacement_length: u32, // Size of replacement text
    pub timestamp: i64,          // Unix timestamp
}
```

### Logging redactions

```rust
fn transform_message(&self, msg: &mut PluginMessage) -> Result<(), PluginError> {
    // Replace text, log what we did
    msg.text = msg.text.replace("sk-my-key-abc123", "[REDACTED:api_key]");
    msg.log_redaction(
        "my-plugin",       // plugin name
        "sk-my-key-abc123",  // original (stored only as length in audit log)
        "[REDACTED:api_key]", // replacement
        "openai_api_key",   // field type
    );
    Ok(())
}
```

---

## 5. PluginError

```rust
pub struct PluginError {
    pub message: String,
    pub fatal: bool,
}
```

- `PluginError::new("reason")` → non-fatal: message passes through unchanged, logged at WARN
- `PluginError::fatal("reason")` → BLOCK the message entirely. User sees "Message blocked by security policy."

---

## 6. Plugin Config

Plugins receive config from `~/.ohagent/plugins.toml`:

```toml
[[plugins]]
file = "libcustom.so"
enabled = true
config = { threshold = 0.75, mode = "strict", allowed_domains = ["example.com"] }
```

Config is available as `serde_json::Value` in `PluginEntry.config`. Your plugin should deserialize it:

```rust
#[derive(Deserialize)]
struct MyConfig {
    threshold: f64,
    mode: String,
    allowed_domains: Vec<String>,
}

fn init(&mut self) -> Result<(), PluginError> {
    let cfg: MyConfig = serde_json::from_value(config).unwrap_or_default();
    // ...
}
```

---

## 7. Audit API

All `RedactionEntry` events are collected and available via REST:

```bash
# View recent redactions
curl -H "X-API-Key: $OHAGENT_API_KEY" http://localhost:9090/api/plugins/audit

# Response:
{
  "total": 42,
  "entries": [
    {
      "plugin": "pii-redactor/email",
      "field": "email",
      "original_bytes": 25,
      "replacement_bytes": 18,
      "timestamp": 1783411200
    }
  ]
}

# Clear audit log
curl -X DELETE -H "X-API-Key: $OHAGENT_API_KEY" http://localhost:9090/api/plugins/audit
```

---

## 8. Plugin Ideas

| Plugin Type | Use Case | Example |
|---|---|---|
| **Data Loss Prevention** | Redact PII, secrets, credentials | `ohagent-pii-redactor` |
| **Infrastructure** | Spawn GPU instances, deploy custom models | `ohagent-infra-launcher` |
| **Audit** | Log every message to SIEM/Splunk/Datadog | `ohagent-audit-logger` |
| **Custom Router** | Route messages to specific models per tenant/channel | `ohagent-custom-router` |
| **Content Moderation** | Block toxic/harmful prompts, enforce usage policies | `ohagent-moderation` |
| **Rate Limiting** | Per-tenant/per-user rate limits beyond built-in | `ohagent-rate-limiter` |
| **Translation** | Auto-translate messages to/from target language | `ohagent-translator` |
| **Context Injection** | Add system context, company policies, RAG data | `ohagent-context-builder` |
| **Multi-Provider** | Route to cheapest available provider in real-time | `ohagent-cost-optimizer` |
| **Data Sovereignty** | Block messages containing country-specific data | `ohagent-data-sovereignty` |

---

## 9. Commercial Plugin Distribution

### Licensing model

1. Build plugin `.so` with license validation in `init()`
2. Generate license keys: `TENANT-<base64(tenant:expiry:hmac)>`
3. Set via env: `OHAGENT_MYPLUGIN_LICENSE=TENANT-XXXX...`
4. Distribute `.so` as binary (no source code)
5. Sell per-seat licenses: $X/month per user

### Multi-platform distribution

```bash
# Cross-compile for all targets
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin

# Package
tar czf my-plugin-v1.0.tar.gz target/*/release/libmy_plugin*.so
```

### Docker distribution

```dockerfile
FROM ghcr.io/ohagent/ohagent:latest
COPY libmy_plugin.so /opt/ohagent/plugins/
COPY plugins.toml /etc/ohagent/plugins.toml
```

---

## 10. Testing Your Plugin

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redacts_emails() {
        let plugin = MyPlugin::new();
        let mut msg = PluginMessage::new(
            "Contact: john@example.com".into(),
            "test-tenant".into(),
            "test".into(),
        );

        plugin.transform_message(&mut msg).unwrap();

        assert!(!msg.text.contains("john@example.com"));
        assert!(msg.text.contains("[REDACTED")); 
        assert_eq!(msg.redaction_log.len(), 1);
    }
}
```

---

## 11. References

- [ohAgent Plugins crate](https://github.com/orangehat/ohAgent/tree/master/crates/ohagent-plugins) — trait definitions & manager
- [PII Redactor plugin](https://github.com/orangehat/ohAgent/tree/master/crates/ohagent-pii-redactor) — complete example
- [Example config](https://github.com/orangehat/ohAgent/blob/master/config/plugins.example.toml)

---

**API Version:** 1  
**Last updated:** July 2026
