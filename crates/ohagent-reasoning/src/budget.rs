//! Token/cost budget tracker with β parameterization.
//!
//! Tracks budget across providers and models, enforces limits,
//! and provides β-based scheduling for the CMC controller.
//!
//! Prices are resolved from a `PricingProvider` trait, implemented
//! by `ohagent-core::pricing::PricingRegistry` at integration time.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Trait for providing model prices. Implemented by PricingRegistry.
pub trait PricingProvider: std::fmt::Debug {
    fn price(&self, model_id: &str) -> (f64, f64); // (input_per_1M, output_per_1M)
}

/// Simple inline pricing provider (for tests or when registry is unavailable).
#[derive(Debug, Clone)]
pub struct InlinePricing {
    input: f64,
    output: f64,
}

impl InlinePricing {
    pub fn new(input: f64, output: f64) -> Self {
        Self { input, output }
    }
}

impl PricingProvider for InlinePricing {
    fn price(&self, _model_id: &str) -> (f64, f64) {
        (self.input, self.output)
    }
}

/// Budget configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Total token budget across all models
    pub max_tokens: u64,
    /// Total cost budget in USD cents (100 = $1.00)
    pub max_cost_cents: u64,
    /// Whether to enforce budgets (false = advisory only)
    pub enforce: bool,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_tokens: 100_000,
            max_cost_cents: 100, // $1.00
            enforce: true,
        }
    }
}

/// Budget tracker — reads prices via PricingProvider trait.
#[derive(Debug)]
pub struct BudgetTracker<P: PricingProvider> {
    config: BudgetConfig,
    tokens_used: u64,
    cost_cents_used: u64,
    started_at: Instant,
    per_model_tokens: HashMap<String, u64>,
    pricing: P,
}

impl<P: PricingProvider> BudgetTracker<P> {
    /// Create with a pricing provider.
    pub fn new(config: BudgetConfig, pricing: P) -> Self {
        Self {
            config,
            tokens_used: 0,
            cost_cents_used: 0,
            started_at: Instant::now(),
            per_model_tokens: HashMap::new(),
            pricing,
        }
    }

    /// Record token usage — cost is calculated from PricingProvider.
    pub fn record(&mut self, model: &str, prompt_tokens: u64, completion_tokens: u64) {
        let total = prompt_tokens + completion_tokens;
        self.tokens_used += total;
        *self.per_model_tokens.entry(model.to_string()).or_default() += total;

        let (input_price, output_price) = self.pricing.price(model);
        let prompt_cost = (prompt_tokens as f64 / 1_000_000.0) * input_price;
        let completion_cost = (completion_tokens as f64 / 1_000_000.0) * output_price;
        let cost = ((prompt_cost + completion_cost) * 100.0) as u64;
        self.cost_cents_used += cost;
    }

    /// Check if budget is exceeded.
    pub fn is_exceeded(&self) -> bool {
        self.config.enforce
            && (self.tokens_used >= self.config.max_tokens
                || self.cost_cents_used >= self.config.max_cost_cents)
    }

    /// Calculate remaining budget as a fraction (1.0 = full, 0.0 = exhausted).
    pub fn remaining_fraction(&self) -> f64 {
        let token_frac = self.tokens_used as f64 / self.config.max_tokens as f64;
        let cost_frac = self.cost_cents_used as f64 / self.config.max_cost_cents as f64;
        let used = token_frac.max(cost_frac);
        (1.0 - used).max(0.0)
    }

    /// Map remaining budget fraction to CMC β.
    pub fn budget_to_beta(&self) -> f64 {
        let frac = self.remaining_fraction();
        frac * 0.9 + 0.1
    }

    /// Get recommended CMC config based on current budget.
    pub fn recommended_cmc_config(&self) -> crate::cmc::CmcConfig {
        crate::cmc::CmcConfig::new(self.budget_to_beta())
    }

    // ── Accessors ──

    pub fn tokens_used(&self) -> u64 {
        self.tokens_used
    }
    pub fn cost_cents_used(&self) -> u64 {
        self.cost_cents_used
    }
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
    pub fn max_tokens(&self) -> u64 {
        self.config.max_tokens
    }
    pub fn pricing(&self) -> &P {
        &self.pricing
    }

    /// Format budget as a human-readable string.
    pub fn status_line(&self) -> String {
        format!(
            "tokens: {}/{} ({}%) cost: ${:.4} elapsed: {:?}",
            self.tokens_used,
            self.config.max_tokens,
            (self.tokens_used as f64 / self.config.max_tokens as f64 * 100.0) as u32,
            self.cost_cents_used as f64 / 100.0,
            self.elapsed()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_tracking() {
        let mut bt = BudgetTracker::new(BudgetConfig::default(), InlinePricing::new(0.14, 0.28));
        bt.record("deepseek-v4-flash", 1000, 500);
        assert_eq!(bt.tokens_used(), 1500);
        assert!(!bt.is_exceeded());
    }

    #[test]
    fn test_budget_exceeded() {
        let config = BudgetConfig {
            max_tokens: 100,
            max_cost_cents: 1000,
            enforce: true,
        };
        let mut bt = BudgetTracker::new(config, InlinePricing::new(0.14, 0.28));
        bt.record("deepseek-v4-flash", 200, 100);
        assert!(bt.is_exceeded());
    }

    #[test]
    fn test_budget_to_beta() {
        let config = BudgetConfig {
            max_tokens: 1000,
            max_cost_cents: 1000,
            enforce: true,
        };
        let bt = BudgetTracker::new(config, InlinePricing::new(0.14, 0.28));
        assert!(bt.budget_to_beta() > 0.9);
    }

    #[test]
    fn test_cost_from_pricing() {
        let mut bt = BudgetTracker::new(
            BudgetConfig {
                max_tokens: 1_000_000,
                max_cost_cents: 10_000,
                enforce: false,
            },
            InlinePricing::new(0.14, 0.28),
        );
        bt.record("deepseek-v4-flash", 1_000_000, 500_000);
        // 1M * 0.14 + 0.5M * 0.28 = 0.14 + 0.14 = 0.28 USD = 28 cents
        assert!(bt.cost_cents_used >= 27 && bt.cost_cents_used <= 29);
    }
}
