//! ohagent-aggregator-plugin — proprietary API aggregation with billing.
//!
//! **Closed source.** Intercepts messages, validates ohag API keys,
//! routes to cheapest provider via DynamicRouter, records billing with markup.
//!
//! # Message Convention
//!
//! Messages starting with `!ag <key> ` trigger the aggregator:
//! ```
//! !ag ohag-xxxx-xxxx-xxxx "Write a Python script..."
//! ```
//! The plugin strips the prefix, validates the key, and passes the message through
//! with billing metadata. Cost is recorded and the customer is charged at their tier markup.
//!
//! # Pricing (Proprietary)
//!
//! | Tier | Markup | Monthly | Tokens/day |
//! |---|---|---|---|
//! | Free | 0% | €0 | 1,000 |
//! | Starter | 20% | €19 | 100,000 |
//! | Pro | 30% | €99 | 1,000,000 |
//! | Enterprise | 15% | €499 | Unlimited |

use ohagent_aggregator_core::{AggregatorStore, ApiKeyManager, BillingTracker, MarkupTier};
use ohagent_plugins::*;

// ── License validation ──

const LICENSE_SECRET: &[u8] = &[0u8]; // Dev: no validation. Production: embedded via build.rs

fn validate_license() -> Result<(), PluginError> {
    if LICENSE_SECRET.len() <= 1 { return Ok(()); } // Dev mode
    let license = std::env::var("OHAGENT_AGGREGATOR_LICENSE")
        .map_err(|_| PluginError::fatal("OHAGENT_AGGREGATOR_LICENSE not set"))?;
    if !license.starts_with("OHAG-") {
        return Err(PluginError::fatal("Invalid license format"));
    }
    Ok(())
}

// ── Cost estimation (proprietary pricing tables) ──

fn estimate_cost_eur(provider: &str, _model: &str, prompt_tokens: u64, completion_tokens: u64) -> f64 {
    let (ip, op) = match provider {
        "deepseek" => (0.14, 0.28),
        "siliconflow" => (0.06, 0.15),
        "scaleway" => (0.15, 0.35),
        "openai" => (2.50, 10.0),
        "anthropic" => (3.00, 15.0),
        _ => (0.27, 1.10),
    };
    (prompt_tokens as f64 / 1_000_000.0) * ip + (completion_tokens as f64 / 1_000_000.0) * op
}

pub struct AggregatorPlugin {
    key_manager: ApiKeyManager,
    billing: BillingTracker,
}

impl AggregatorPlugin {
    pub fn new() -> Self {
        let path = shellexpand::tilde("~/.ohagent/aggregator.db");
        let store = AggregatorStore::open(path.as_ref())
            .expect("Failed to open aggregator store");
        let db = store.db();
        Self {
            key_manager: ApiKeyManager::new(db.clone()),
            billing: BillingTracker::new(db),
        }
    }
}

impl MessagePlugin for AggregatorPlugin {
    fn name(&self) -> &str { "ohagent-aggregator" }
    fn version(&self) -> (u32, u32) { (1, 0) }

    fn init(&mut self) -> Result<(), PluginError> {
        validate_license()?;
        tracing::info!("Aggregator plugin initialized");
        Ok(())
    }

    fn transform_message(&self, message: &mut PluginMessage) -> Result<(), PluginError> {
        // Check for aggregator prefix: "!ag <key> <actual message>"
        let rest = match message.text.strip_prefix("!ag ") {
            Some(r) => r,
            None => return Ok(()), // Not an aggregator request — pass through
        };

        // Parse: first token is API key, rest is message
        let (key_str, actual_msg) = match rest.split_once(' ') {
            Some((k, m)) => (k, m.to_string()),
            None => return Err(PluginError::fatal("Usage: !ag <api-key> <message>")),
        };

        // Validate key
        let key = self.key_manager.validate(key_str)
            .map_err(|e| PluginError::fatal(&format!("API key error: {e}")))?;

        // Check quota
        if !self.billing.check_quota(&key.id, key.monthly_token_limit)
            .unwrap_or(false)
        {
            return Err(PluginError::fatal("Daily token quota exceeded. Upgrade your plan at ohagent.dev"));
        }

        // Record estimated cost (rough estimate before LLM call)
        let prompt_est = (actual_msg.len() as u64) / 4;
        let completion_est = 500; // rough estimate
        let cost = estimate_cost_eur("deepseek", "v4-flash", prompt_est, completion_est);
        let customer_cost = self.billing.record(
            &key.id, &key.customer_id, "deepseek", "v4-flash",
            prompt_est, completion_est, cost, &key.tier,
        ).unwrap_or(0.0);

        // Replace message text with clean version
        let tier_markup = (key.tier.markup() - 1.0) * 100.0;
        message.text = format!(
            "{} [aggregator: tier={:?}, cost=€{:.6}]",
            actual_msg, key.tier, customer_cost
        );

        tracing::info!(
            customer = %key.customer_id,
            tier = ?key.tier,
            cost_e6 = (customer_cost * 1_000_000.0) as u64,
            "Aggregator request billed"
        );

        Ok(())
    }

    fn transform_response(&self, message: &mut PluginMessage) -> Result<(), PluginError> {
        // Strip billing metadata from response
        if let Some(idx) = message.text.find(" [aggregator:") {
            message.text.truncate(idx);
        }
        Ok(())
    }

    fn shutdown(&mut self) {
        tracing::info!("Aggregator plugin shutting down");
    }
}

// ── FFI ──

#[no_mangle]
pub extern "C" fn plugin_api_version() -> u32 { CURRENT_PLUGIN_API_VERSION }

#[no_mangle]
pub extern "C" fn create_plugin() -> PluginBox {
    PluginBox::from_box(Box::new(AggregatorPlugin::new()))
}
