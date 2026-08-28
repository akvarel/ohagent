//! ohagent-pii-redactor — on-device PII/secret detection and redaction.
//!
//! **Proprietary plugin.** Detects and redacts sensitive data before it reaches
//! the LLM provider. All processing is local; no data leaves the machine.
//!
//! # License Validation
//!
//! Production builds require a valid license key, validated via HMAC-SHA256:
//! ```
//! ohagent-pii-redactor --generate-license "tenant-123:2026-12-31"
//! → license key: TENANT-XXXX-YYYY-ZZZZ
//! ```
//!
//! Set via environment: `OHAGENT_PII_LICENSE=TENANT-XXXX-YYYY-ZZZZ`
//!
//! # Detection Categories (15 total)
//!
//! | Category | Examples | Replacement |
//! |---|---|---|
//! | API Keys | sk-*, sk-ant-*, xoxb-*, ghp_*, hf_*, JWT | `[REDACTED:api_key_*]` |
//! | AWS Keys | AKIA*, ASIA*, session tokens | `[REDACTED:aws_*]` |
//! | Private Keys | PEM, SSH | `[REDACTED:private_key_*]` |
//! | Connection Strings | postgres://, mysql://, mongodb:// | `[REDACTED:conn_string]` |
//! | PII | emails, phones, SSN | `[REDACTED:email/phone/ssn]` |
//! | Financial | credit cards, IBAN | `[REDACTED:credit_card/iban]` |
//! | IP Addresses | IPv4, IPv6 | `[REDACTED:ip_v4/ip_v6]` |
//! | Secrets in Code | password=, secret=, token= | `[REDACTED:secret_assignment]` |

use ohagent_plugins::*;
use regex::Regex;
use std::time::{SystemTime, UNIX_EPOCH};

// ── License validation ──

/// Secret key embedded at build time (set via build.rs or env var).
/// In CI: `OHAGENT_PII_SECRET=... cargo build --release`
const LICENSE_SECRET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/license_secret.bin"));

/// Validate a license key of the form `TENANT-<b64>`
fn validate_license(tenant_id: &str) -> Result<(), PluginError> {
    let license = match std::env::var("OHAGENT_PII_LICENSE") {
        Ok(l) => l,
        Err(_) => {
            return Err(PluginError::fatal(
                "OHAGENT_PII_LICENSE not set. Get a license: https://ohagent.dev/licenses",
            ))
        }
    };

    // Format: TENANT-<base64(tenant_id:expiry:hmac)>
    let payload = match license.strip_prefix("TENANT-") {
        Some(p) => p,
        None => {
            return Err(PluginError::fatal(
                "Invalid license format — must start with TENANT-",
            ))
        }
    };

    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| PluginError::fatal("Invalid license: base64 decode failed"))?;

    let parts: Vec<&[u8]> = decoded.split(|b| *b == b':').collect();
    if parts.len() != 3 {
        return Err(PluginError::fatal("Invalid license: wrong format"));
    }

    let lic_tenant = std::str::from_utf8(parts[0])
        .map_err(|_| PluginError::fatal("Invalid license: bad tenant"))?;
    let expiry_str = std::str::from_utf8(parts[1])
        .map_err(|_| PluginError::fatal("Invalid license: bad expiry"))?;
    let sig = parts[2];

    // Verify tenant_id matches
    if lic_tenant != tenant_id && lic_tenant != "*" {
        return Err(PluginError::fatal(&format!(
            "License is for tenant '{lic_tenant}', but running as '{tenant_id}'"
        )));
    }

    // Verify expiry
    let expiry: u64 = expiry_str
        .parse()
        .map_err(|_| PluginError::fatal("Invalid license: bad expiry timestamp"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now > expiry {
        let days = (now - expiry) / 86400;
        return Err(PluginError::fatal(&format!(
            "License expired {days} days ago. Renew at https://ohagent.dev/licenses"
        )));
    }

    // Verify HMAC signature
    let message: Vec<u8> = decoded[..decoded.len() - sig.len() - 1].to_vec(); // data before last ':'
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_from_slice(LICENSE_SECRET)
        .map_err(|_| PluginError::fatal("License validation internal error"))?;
    mac.update(&message);
    mac.verify_slice(sig)
        .map_err(|_| PluginError::fatal("Invalid license: signature mismatch"))?;

    Ok(())
}

// ── Plugin implementation ──

pub struct PiiRedactorPlugin {
    patterns: Vec<RedactionPattern>,
    license_validated: bool,
}

struct RedactionPattern {
    name: &'static str,
    regex: Regex,
}

impl PiiRedactorPlugin {
    pub fn new() -> Self {
        let patterns = vec![
            pattern(
                "api_key_openai",
                r"sk-(?:proj-|org-)?[A-Za-z0-9]{20,}[^\s]*",
            ),
            pattern("api_key_openai_proj", r"sk-proj-[A-Za-z0-9_-]{32,}"),
            pattern(
                "api_key_anthropic",
                r"sk-ant-[a-z]{3,5}\d{2}-[A-Za-z0-9_-]{32,}",
            ),
            pattern(
                "api_key_slack",
                r"xox[bpras]-[0-9]{9,12}-[0-9]{9,12}-[A-Za-z0-9]{24,32}",
            ),
            pattern("api_key_github", r"gh[pousr]_[A-Za-z0-9]{36,40}"),
            pattern("api_key_huggingface", r"hf_[A-Za-z0-9]{34}"),
            pattern(
                "jwt_token",
                r"eyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}",
            ),
            pattern("aws_access_key", r"AKIA[0-9A-Z]{16}"),
            pattern("aws_secret_key", r"ASIA[0-9A-Z]{16}"),
            pattern("aws_session_token", r"IQoJb3JpZ2luX2Vj[A-Za-z0-9/+=]{200,}"),
            pattern(
                "private_key_pem",
                r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----[A-Za-z0-9+/=\s\n\r]+-----END (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
            ),
            pattern(
                "ssh_private_key",
                r"-----BEGIN OPENSSH PRIVATE KEY-----[A-Za-z0-9+/=\s\n\r]+-----END OPENSSH PRIVATE KEY-----",
            ),
            pattern(
                "conn_string",
                r"(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|rediss)://[^\s]+:[^\s@]+@[^\s]+",
            ),
            pattern("email", r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}"),
            pattern(
                "phone_international",
                r"\+[1-9]\d{1,3}[-.\s]?\(?\d{1,4}\)?[-.\s]?\d{1,4}[-.\s]?\d{1,9}",
            ),
            pattern("ssn", r"\b\d{3}-\d{2}-\d{4}\b"),
            pattern(
                "credit_card",
                r"\b(?:4[0-9]{12}(?:[0-9]{3})?|5[1-5][0-9]{14}|3[47][0-9]{13}|6(?:011|5[0-9]{2})[0-9]{12})\b",
            ),
            pattern("iban", r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b"),
            pattern(
                "ip_v4",
                r"\b(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b",
            ),
            pattern("ip_v6", r"\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b"),
            pattern(
                "secret_assignment",
                r#"(?i)(?:password|passwd|pwd|secret|api[_-]?key|apikey|token|auth[_-]?token|access[_-]?key)\s*[:=]\s*["']?([^\s"']{8,})"#,
            ),
        ];

        Self {
            patterns,
            license_validated: false,
        }
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
        // Validate license in production builds
        if LICENSE_SECRET.len() > 1 {
            let tenant = std::env::var("OHAGENT_TENANT_ID").unwrap_or_else(|_| "default".into());
            validate_license(&tenant)?;
            self.license_validated = true;
            tracing::info!("PII redactor license validated for tenant '{tenant}'");
        } else {
            // Dev build — no license required
            tracing::warn!("PII redactor running WITHOUT license validation (dev build)");
        }

        tracing::info!(patterns = self.patterns.len(), "PII redactor initialized");
        Ok(())
    }

    fn transform_message(&self, message: &mut PluginMessage) -> Result<(), PluginError> {
        let original_len = message.text.len();
        let mut redactions = 0u32;

        for pat in &self.patterns {
            let hits: Vec<(usize, usize)> = pat
                .regex
                .find_iter(&message.text)
                .map(|m| (m.start(), m.end()))
                .collect();

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
                "PII redacted"
            );
        }
        Ok(())
    }
}

// ── FFI exports ──

#[no_mangle]
pub extern "C" fn plugin_api_version() -> u32 {
    CURRENT_PLUGIN_API_VERSION
}

#[no_mangle]
pub extern "C" fn create_plugin() -> PluginBox {
    PluginBox::from_box(Box::new(PiiRedactorPlugin::new()))
}
