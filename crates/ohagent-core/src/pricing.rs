//! Pricing registry — single source of truth for model prices.
//!
//! Loaded from the model catalog (models.toml) and kept in sync.
//! BudgetTracker reads from here instead of hardcoded values.
//! Prices can be refreshed at runtime via the API.

use chrono::Timelike;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

use crate::model_router::ModelEntry;
use ohagent_reasoning::budget::PricingProvider;

/// Price per 1M tokens for a model.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
}

/// Time-window for off-peak pricing.
#[derive(Debug, Clone)]
struct OffPeakWindow {
    discount: f64, // multiplier: 0.50 = 50% off
    start_h: u32,
    start_m: u32,
    end_h: u32,
    end_m: u32,
}

impl OffPeakWindow {
    fn is_active(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        let current_minutes = now.hour() * 60 + now.minute();
        let start_min = self.start_h * 60 + self.start_m;
        let end_min = self.end_h * 60 + self.end_m;

        if start_min <= end_min {
            // Same day window, e.g. 02:00–06:00
            current_minutes >= start_min && current_minutes < end_min
        } else {
            // Overnight window, e.g. 16:30–00:30
            current_minutes >= start_min || current_minutes < end_min
        }
    }
}

impl ModelPrice {
    /// Calculate cost in USD cents — base price.
    pub fn cost_cents(&self, prompt_tokens: u64, completion_tokens: u64) -> u64 {
        let prompt_cost = (prompt_tokens as f64 / 1_000_000.0) * self.input;
        let completion_cost = (completion_tokens as f64 / 1_000_000.0) * self.output;
        ((prompt_cost + completion_cost) * 100.0) as u64
    }
}

/// Registry of model prices, loaded from catalog.
#[derive(Debug, Clone)]
pub struct PricingRegistry {
    prices: HashMap<String, ModelPrice>,
    off_peak: HashMap<String, OffPeakWindow>,
    last_sync: chrono::DateTime<chrono::Utc>,
    source: String,
}

impl PricingRegistry {
    /// Create from model catalog entries.
    pub fn from_catalog(entries: &[ModelEntry]) -> Self {
        let mut prices = HashMap::new();
        let mut off_peak = HashMap::new();

        for entry in entries {
            if entry.id.contains("image") || entry.id.contains("video") || entry.id.contains("dall-e") || entry.id.contains("kling") || entry.id.contains("flux") {
                continue;
            }

            match (entry.input_price, entry.output_price) {
                (Some(input), Some(output)) => {
                    if input > 0.0 || output > 0.0 {
                        prices.insert(entry.id.clone(), ModelPrice { input, output });
                        debug!(model = %entry.id, input, output, "Registered model price");
                    }
                }
                (Some(_), None) | (None, Some(_)) => {
                    warn!(model = %entry.id, "Partial pricing — skipping");
                }
                (None, None) => {
                    warn!(model = %entry.id, "No pricing — using 0.0");
                    prices.insert(entry.id.clone(), ModelPrice { input: 0.0, output: 0.0 });
                }
            }

            // Register off-peak window
            if let (Some(discount), Some(ref start), Some(ref end)) =
                (entry.off_peak_discount, &entry.off_peak_start_utc, &entry.off_peak_end_utc)
            {
                if let (Ok(sh), Ok(sm), Ok(eh), Ok(em)) = (
                    start[..2].parse::<u32>(), start[3..].parse::<u32>(),
                    end[..2].parse::<u32>(), end[3..].parse::<u32>(),
                ) {
                    off_peak.insert(entry.id.clone(), OffPeakWindow {
                        discount,
                        start_h: sh, start_m: sm,
                        end_h: eh, end_m: em,
                    });
                    info!(
                        model = %entry.id, discount, start, end,
                        "Off-peak pricing registered"
                    );
                }
            }
        }

        info!(models = prices.len(), "Pricing registry loaded");

        Self {
            prices,
            off_peak,
            last_sync: chrono::Utc::now(),
            source: "catalog".to_string(),
        }
    }

    /// Get base price for a model.
    pub fn get(&self, model_id: &str) -> Option<ModelPrice> {
        self.prices.get(model_id).copied()
    }

    /// Get effective price — applies off-peak discount if current time is in window.
    pub fn get_or_estimate(&self, model_id: &str) -> ModelPrice {
        let base = self.get(model_id).unwrap_or_else(|| {
            let (input, output) = if model_id.contains("deepseek") {
                (0.14, 0.28)
            } else if model_id.contains("claude") || model_id.contains("sonnet") {
                (3.0, 15.0)
            } else if model_id.contains("gpt") || model_id.contains("openai") {
                (2.5, 10.0)
            } else if model_id.contains("glm") || model_id.contains("zhipu") {
                (1.40, 4.40)
            } else {
                (1.0, 4.0)
            };
            ModelPrice { input, output }
        });

        // Apply off-peak discount if active
        if let Some(window) = self.off_peak.get(model_id) {
            let now = chrono::Utc::now();
            if window.is_active(now) {
                let d = window.discount;
                let effective = ModelPrice {
                    input: base.input * (1.0 - d),
                    output: base.output * (1.0 - d),
                };
                debug!(
                    model = %model_id,
                    base_input = base.input, base_output = base.output,
                    discount = d,
                    effective_input = effective.input, effective_output = effective.output,
                    "Off-peak pricing applied"
                );
                return effective;
            }
        }

        base
    }

    pub fn update(&mut self, model_id: &str, input: f64, output: f64) {
        self.prices.insert(model_id.to_string(), ModelPrice { input, output });
        self.last_sync = chrono::Utc::now();
        info!(%model_id, input, output, "Price updated via API");
    }

    pub fn reload(&mut self, entries: &[ModelEntry]) {
        *self = Self::from_catalog(entries);
    }

    pub fn len(&self) -> usize { self.prices.len() }
    pub fn is_empty(&self) -> bool { self.prices.is_empty() }
    pub fn last_sync(&self) -> chrono::DateTime<chrono::Utc> { self.last_sync }
    pub fn source(&self) -> &str { &self.source }
    pub fn all_prices(&self) -> HashMap<String, ModelPrice> { self.prices.clone() }
}

impl PricingProvider for PricingRegistry {
    fn price(&self, model_id: &str) -> (f64, f64) {
        let p = self.get_or_estimate(model_id);
        (p.input, p.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<ModelEntry> {
        vec![
            ModelEntry {
                id: "deepseek-v4-flash".into(), provider: "deepseek".into(),
                api_key_env: "K".into(), display: "DS".into(),
                capabilities: vec!["coding".into()], cost_tier: "low".into(),
                context: 1_000_000, input_price: Some(0.14), output_price: Some(0.28),
                off_peak_discount: Some(0.5),
                off_peak_start_utc: Some("16:30".into()),
                off_peak_end_utc: Some("00:30".into()),
                enabled: true,
                serverless: None, serverless_lora: None, fine_tuning: None,
                embeddings: None, rerankers: None, vision: None,
                json_mode: None, structured_outputs: None, tools: None,
                fim_completion: None, chat_prefix: None,
                base_model: None, lora_id: None,
            },
            ModelEntry {
                id: "dall-e-3".into(), provider: "openai-image".into(),
                api_key_env: "K".into(), display: "DALL-E".into(),
                capabilities: vec!["image_gen".into()], cost_tier: "medium".into(),
                context: 0, input_price: None, output_price: None,
                off_peak_discount: None,
                off_peak_start_utc: None,
                off_peak_end_utc: None,
                enabled: true,
                serverless: None, serverless_lora: None, fine_tuning: None,
                embeddings: None, rerankers: None, vision: None,
                json_mode: None, structured_outputs: None, tools: None,
                fim_completion: None, chat_prefix: None,
                base_model: None, lora_id: None,
            },
        ]
    }

    #[test]
    fn test_from_catalog() {
        let reg = PricingRegistry::from_catalog(&sample());
        assert!(reg.get("deepseek-v4-flash").is_some());
        assert!(reg.get("dall-e-3").is_none());
    }

    #[test]
    fn test_cost_calculation() {
        let reg = PricingRegistry::from_catalog(&sample());
        let price = reg.get("deepseek-v4-flash").unwrap();
        let cost = price.cost_cents(1_000_000, 500_000);
        assert!(cost >= 27 && cost <= 29);
    }

    #[test]
    fn test_fallback() {
        let reg = PricingRegistry::from_catalog(&[]);
        let price = reg.get_or_estimate("deepseek-unknown");
        assert_eq!(price.input, 0.14);
    }
}
