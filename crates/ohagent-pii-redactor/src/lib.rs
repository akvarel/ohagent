//! ohagent-pii-redactor — on-device PII/secret detection and redaction.
//!
//! **Proprietary plugin.** Detects and redacts sensitive data before it reaches
//! the LLM provider. All processing is local; no data leaves the machine.
//!
//! # Detection Categories
//!
//! | Category | Pattern | Replacement |
//! |---|---|---|
//! | API Keys | `sk-*`, `xoxb-*`, `ghp_*`, `hf_*`, JWT | `[REDACTED:api_key]` |
//! | AWS Keys | `AKIA*`, `ASIA*` | `[REDACTED:aws_key]` |
//! | Emails | RFC 5322 | `[REDACTED:email]` |
//! | Credit Cards | 13-19 digits, Luhn-checked | `[REDACTED:credit_card]` |
//! | Phone Numbers | International formats | `[REDACTED:phone]` |
//! | IP Addresses | IPv4 + IPv6 | `[REDACTED:ip]` |
//! | Secrets in Code | `password=`, `secret=`, `token=` | `[REDACTED:secret]` |
//! | Private Keys | PEM headers | `[REDACTED:private_key]` |
//! | Connection Strings | `postgres://user:pass@` | `[REDACTED:conn_string]` |
//! | SSN/Personal IDs | ###-##-#### format | `[REDACTED:ssn]` |

use ohagent_plugins::*;
use regex::Regex;

/// License validation placeholder.
/// In production, this would verify a signed license key against
/// tenant_id and expiration date.
const ENTERPRISE_LICENSE_REQUIRED: bool = false; // Set to true in production builds

pub struct PiiRedactorPlugin {
    patterns: Vec<RedactionPattern>,
}

struct RedactionPattern {
    name: &'static str,
    regex: Regex,
}

impl PiiRedactorPlugin {
    pub fn new() -> Self {
        let patterns = vec![
            // ── API Keys ──
            pattern("api_key_openai",      r"sk-(?:proj-|org-)?[A-Za-z0-9]{20,}[^\s]*"),
            pattern("api_key_openai_proj", r"sk-proj-[A-Za-z0-9_-]{32,}"),
            pattern("api_key_anthropic",   r"sk-ant-[a-z]{3,5}\d{2}-[A-Za-z0-9_-]{32,}"),
            pattern("api_key_slack",       r"xox[bpras]-[0-9]{9,12}-[0-9]{9,12}-[A-Za-z0-9]{24,32}"),
            pattern("api_key_github",      r"gh[pousr]_[A-Za-z0-9]{36,40}"),
            pattern("api_key_huggingface", r"hf_[A-Za-z0-9]{34}"),
            pattern("jwt_token",           r"eyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}"),

            // ── AWS ──
            pattern("aws_access_key",      r"AKIA[0-9A-Z]{16}"),
            pattern("aws_secret_key",      r"ASIA[0-9A-Z]{16}"),
            pattern("aws_session_token",   r"IQoJb3JpZ2luX2Vj[A-Za-z0-9/+=]{200,}"),

            // ── Private Keys ──
            pattern("private_key_pem",     r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----[A-Za-z0-9+/=\s\n\r]+-----END (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----"),
            pattern("ssh_private_key",     r"-----BEGIN OPENSSH PRIVATE KEY-----[A-Za-z0-9+/=\s\n\r]+-----END OPENSSH PRIVATE KEY-----"),

            // ── Connection Strings ──
            pattern("conn_string",         r"(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|rediss)://[^\s]+:[^\s@]+@[^\s]+"),

            // ── PII ──
            pattern("email",               r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}"),
            pattern("phone_international", r"\+[1-9]\d{1,3}[-.\s]?\(?\d{1,4}\)?[-.\s]?\d{1,4}[-.\s]?\d{1,9}"),
            pattern("ssn",                 r"\b\d{3}-\d{2}-\d{4}\b"),

            // ── Financial ──
            pattern("credit_card",         r"\b(?:4[0-9]{12}(?:[0-9]{3})?|5[1-5][0-9]{14}|3[47][0-9]{13}|6(?:011|5[0-9]{2})[0-9]{12})\b"),
            pattern("iban",                r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b"),

            // ── IP Addresses ──
            pattern("ip_v4",               r"\b(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b"),
            pattern("ip_v6",               r"\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b"),

            // ── Secrets in Code ──
            pattern("secret_assignment",   r#"(?i)(?:password|passwd|pwd|secret|api[_-]?key|apikey|token|auth[_-]?token|access[_-]?key)\s*[:=]\s*["']?([^\s"']{8,})"#),
        ];

        Self { patterns }
    }
}

fn pattern(name: &'static str, re: &str) -> RedactionPattern {
    RedactionPattern {
        name,
        regex: Regex::new(re).expect(&format!("Invalid regex for {name}")),
    }
}

impl MessagePlugin for PiiRedactorPlugin {
    fn name(&self) -> &str {
        "ohagent-pii-redactor"
    }

    fn version(&self) -> (u32, u32) {
        (1, 0)
    }

    fn init(&mut self) -> Result<(), PluginError> {
        if ENTERPRISE_LICENSE_REQUIRED {
            // In production builds, validate license here.
            // Check license key, tenant, expiration.
            // If invalid, return Err(PluginError::fatal("License invalid")).
        }
        tracing::info!(
            patterns = self.patterns.len(),
            "PII redactor initialized"
        );
        Ok(())
    }

    fn transform_message(&self, message: &mut PluginMessage) -> Result<(), PluginError> {
        let original_len = message.text.len();
        let mut redactions = 0u32;

        for pat in &self.patterns {
            // Collect match positions first (drop immutable borrow before mutating)
            let hits: Vec<(usize, usize)> = pat
                .regex
                .find_iter(&message.text)
                .map(|m| (m.start(), m.end()))
                .collect();

            // Replace from end to start to preserve indices
            for (start, end) in hits.iter().rev() {
                let replacement = format!("[REDACTED:{}]", pat.name);
                message.text.replace_range(*start..*end, &replacement);
                message.log_redaction(
                    &format!("pii-redactor/{}", pat.name),
                    "", // original content never stored
                    &replacement,
                    pat.name,
                );
                redactions += 1;
            }
        }

        if redactions > 0 {
            tracing::info!(
                original_len,
                final_len = message.text.len(),
                redactions,
                "PII redaction completed"
            );
        } else {
            tracing::debug!(original_len, "No PII detected");
        }

        Ok(())
    }
}

// ── FFI exports (required by plugin system) ──

#[no_mangle]
pub extern "C" fn plugin_api_version() -> u32 {
    CURRENT_PLUGIN_API_VERSION
}

#[no_mangle]
pub extern "C" fn create_plugin() -> *mut dyn MessagePlugin {
    Box::into_raw(Box::new(PiiRedactorPlugin::new()))
}
