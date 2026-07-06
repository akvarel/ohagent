//! Token/cost budget tracker with β parameterization.
//!
//! Tracks budget across providers and models, enforces limits,
//! and provides β-based scheduling for the CMC controller.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

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

/// Per-provider pricing (USD per 1M tokens).
#[derive(Debug, Clone)]
pub struct ProviderPricing {
    pub prompt_price_per_m: f64,
    pub completion_price_per_m: f64,
}

impl ProviderPricing {
    pub fn deepseek() -> Self {
        Self { prompt_price_per_m: 0.14, completion_price_per_m: 0.28 }
    }
    pub fn anthropic_sonnet() -> Self {
        Self { prompt_price_per_m: 3.0, completion_price_per_m: 15.0 }
    }
    pub fn anthropic_opus() -> Self {
        Self { prompt_price_per_m: 15.0, completion_price_per_m: 75.0 }
    }
    pub fn openai_gpt4o() -> Self {
        Self { prompt_price_per_m: 2.5, completion_price_per_m: 10.0 }
    }
}

/// Budget tracker — shared between CMC controller and ModelRouter.
#[derive(Debug)]
pub struct BudgetTracker {
    config: BudgetConfig,
    tokens_used: u64,
    cost_cents_used: u64,
    started_at: Instant,
    /// Per-model token counts
    model_tokens: HashMap<String, u64>,
    /// Pricing table
    pricing: HashMap<String, ProviderPricing>,
}

impl BudgetTracker {
    pub fn new(config: BudgetConfig) -> Self {
        let mut pricing = HashMap::new();
        pricing.insert("deepseek-chat".into(), ProviderPricing::deepseek());
        pricing.insert("deepseek-reasoner".into(), ProviderPricing::deepseek());
        pricing.insert("claude-sonnet-4-6".into(), ProviderPricing::anthropic_sonnet());
        pricing.insert("claude-opus-4-6".into(), ProviderPricing::anthropic_opus());
        pricing.insert("gpt-4o".into(), ProviderPricing::openai_gpt4o());

        Self {
            config,
            tokens_used: 0,
            cost_cents_used: 0,
            started_at: Instant::now(),
            model_tokens: HashMap::new(),
            pricing,
        }
    }

    /// Record token usage.
    pub fn record(&mut self, model: &str, prompt_tokens: u64, completion_tokens: u64) {
        let total = prompt_tokens + completion_tokens;
        self.tokens_used += total;
        *self.model_tokens.entry(model.to_string()).or_default() += total;

        // Calculate cost
        if let Some(pricing) = self.pricing.get(model) {
            let prompt_cost = (prompt_tokens as f64 / 1_000_000.0) * pricing.prompt_price_per_m;
            let completion_cost = (completion_tokens as f64 / 1_000_000.0) * pricing.completion_price_per_m;
            let total_cost = (prompt_cost + completion_cost) * 100.0; // to cents
            self.cost_cents_used += total_cost as u64;
        }
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
    ///
    /// When budget is full → β=1.0 (thorough)
    /// When budget is half → β=0.5 (balanced)
    /// When budget is low → β=0.1 (cheap)
    pub fn budget_to_beta(&self) -> f64 {
        let frac = self.remaining_fraction();
        // Smooth mapping: frac → beta
        // 1.0 → 1.0, 0.5 → 0.5, 0.0 → 0.1
        frac * 0.9 + 0.1
    }

    /// Get recommended CMC config based on current budget.
    pub fn recommended_cmc_config(&self) -> crate::cmc::CmcConfig {
        crate::cmc::CmcConfig::new(self.budget_to_beta())
    }

    // ── Accessors ──

    pub fn tokens_used(&self) -> u64 { self.tokens_used }
    pub fn cost_cents_used(&self) -> u64 { self.cost_cents_used }
    pub fn elapsed(&self) -> Duration { self.started_at.elapsed() }
    pub fn max_tokens(&self) -> u64 { self.config.max_tokens }

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
        let mut bt = BudgetTracker::new(BudgetConfig::default());
        bt.record("deepseek-chat", 1000, 500);
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
        let mut bt = BudgetTracker::new(config);
        bt.record("deepseek-chat", 200, 100);
        assert!(bt.is_exceeded());
    }

    #[test]
    fn test_budget_to_beta() {
        let config = BudgetConfig {
            max_tokens: 1000,
            max_cost_cents: 1000,
            enforce: true,
        };
        let bt = BudgetTracker::new(config);
        assert!(bt.budget_to_beta() > 0.9); // Full budget → high beta
    }
}
