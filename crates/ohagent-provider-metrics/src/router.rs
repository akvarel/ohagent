//! Dynamic routing optimizer — selects the best provider+model for a task.
//!
//! Scoring formula:
//! ```
//! score = α * price_score + β * speed_score + γ * quality_score
//!
//! price_score  = 1 - (price / max_price)        // cheaper = better
//! speed_score  = tps / max_tps                    // faster = better
//! quality_score = capability_match * tier_bonus    // matches task
//! ```
//!
//! Weights (α, β, γ) depend on QualityTier:
//! - Budget:      α=0.7, β=0.2, γ=0.1
//! - Balanced:    α=0.4, β=0.3, γ=0.3
//! - Performance: α=0.2, β=0.6, γ=0.2
//! - Quality:     α=0.1, β=0.1, γ=0.8

use crate::models::{PriceRecord, QualityTier, RouterConfig, RoutingAlternative, RoutingDecision, DocumentCount};
use crate::store::MetricsStore;

pub struct DynamicRouter {
    store: MetricsStore,
}

impl DynamicRouter {
    pub fn new(store: MetricsStore) -> Self {
        Self { store }
    }

    /// Route a task to the best provider+model given constraints.
    ///
    /// If `doc_count` is Multiple(2+), only models with `multi_doc` capability
    /// are considered — this ensures multi-receipt images go to GLM-4.6V.
    /// If `doc_count` is Single, the `multi_doc` capability is NOT required,
    /// and the cheapest vision model wins.
    pub fn route(
        &self,
        task_capabilities: &[&str],
        estimated_prompt_tokens: u64,
        estimated_output_tokens: u64,
        config: &RouterConfig,
        doc_count: DocumentCount,
    ) -> Result<RoutingDecision, String> {
        let prices = self.store.get_all_latest_prices()?;

        // Filter: must have all required capabilities AND be token-based (for now)
        let candidates: Vec<&PriceRecord> = prices.iter().filter(|p| {
            if config.prefer_eu && p.provider != "scaleway" { return false; }
            // Only route token-based models — images/video/audio are special-purpose
            if p.pricing_model != crate::models::PricingModel::PerMillionTokens && p.pricing_model != crate::models::PricingModel::PerMillionBytes { return false; }
            // Multi-doc routing: if 2+ documents detected, model MUST have multi_doc capability
            if doc_count.is_multi() && !p.capabilities.iter().any(|c| c == "multi_doc") {
                return false;
            }
            task_capabilities.iter().all(|cap| p.capabilities.iter().any(|c| c == *cap))
        }).collect();

        if candidates.is_empty() {
            return Err("No provider matches required capabilities".into());
        }

        // Convert all prices to EUR for comparison
        let eur_rates: Vec<(&str, f64)> = vec![("USD", 0.92), ("EUR", 1.0), ("CNY", 0.13)];
        let to_eur = |currency: &str, price: f64| -> f64 {
            eur_rates.iter().find(|(c, _)| *c == currency).map(|(_, r)| price * r).unwrap_or(price)
        };

        // Compute scores
        struct CandidateScore<'a> {
            record: &'a PriceRecord,
            cost_eur: f64,
            price_score: f64,
            speed_score: f64,
            quality_score: f64,
            total_score: f64,
            latency_ms: u64,
            tps: f64,
        }

        let max_cost: f64 = candidates.iter()
            .map(|p| p.estimated_cost_eur(estimated_prompt_tokens, estimated_output_tokens))
            .fold(0.0, f64::max).max(0.001);

        let max_tps: f64 = 200.0;

        let mut scored: Vec<CandidateScore> = Vec::new();

        for record in &candidates {
            let cost = record.estimated_cost_eur(estimated_prompt_tokens, estimated_output_tokens);

            if let Some(max_budget) = config.max_budget_eur_per_1k {
                let cost_per_1k = cost * 1000.0 / (estimated_prompt_tokens + estimated_output_tokens) as f64;
                if cost_per_1k > max_budget { continue; }
            }

            let price_score = 1.0 - (cost / max_cost).min(1.0);

            let speeds = self.store.get_speeds(&record.provider, &record.model_id).unwrap_or_default();
            let (speed_score, latency_ms, tps) = if let Some(best) = speeds.first() {
                (best.tokens_per_second / max_tps, best.total_latency_ms, best.tokens_per_second)
            } else {
                let (est_tps, est_lat) = estimated_speed(&record.provider, &record.model_id);
                (est_tps / max_tps, est_lat, est_tps)
            };

            let quality_base = match record.provider.as_str() {
                "anthropic" => 0.95, "openai" => 0.90, "deepseek" => 0.85,
                "zai" => {
                    // GLM-4.6V is the KING for multi-document vision. GLM-5.2 for general chat.
                    if record.model_id.contains("glm-4.6v") { 0.95 }
                    else { 0.85 }
                },
                "siliconflow" => {
                    // GLM-5V-Turbo on SF gets vision quality bump
                    if record.model_id.contains("GLM-5V") { 0.90 }
                    else { 0.75 }
                },
                "scaleway" => 0.80,
                "google" => {
                    // Gemini 3.1 Flash-Lite = king of LV receipt OCR. 2.5 Pro for code.
                    if record.model_id.contains("pro") { 0.92 }
                    else if record.model_id.contains("flash-lite") { 0.87 }
                    else { 0.83 }
                },
                _ => 0.70,
            };
            let cap_match = task_capabilities.len() as f64 / record.capabilities.len().max(1) as f64;
            let quality_score = quality_base * (0.5 + 0.5 * cap_match);

            let (alpha, beta, gamma) = match config.quality_tier {
                QualityTier::Budget => (0.7, 0.2, 0.1),
                QualityTier::Balanced => (0.4, 0.3, 0.3),
                QualityTier::Performance => (0.2, 0.6, 0.2),
                QualityTier::Quality => (0.1, 0.1, 0.8),
            };

            let total_score = alpha * price_score + beta * speed_score + gamma * quality_score;
            scored.push(CandidateScore { record, cost_eur: cost, price_score, speed_score, quality_score, total_score, latency_ms, tps });
        }

        scored.sort_by(|a, b| b.total_score.partial_cmp(&a.total_score).unwrap_or(std::cmp::Ordering::Equal));

        let best = &scored[0];
        let alternatives: Vec<RoutingAlternative> = scored.iter().take(4).skip(1).map(|s| {
            RoutingAlternative {
                provider: s.record.provider.clone(),
                model_id: s.record.model_id.clone(),
                cost_eur: s.cost_eur,
                latency_ms: s.latency_ms,
                tps: s.tps,
            }
        }).collect();

        let tier_label = match config.quality_tier {
            QualityTier::Budget => "budget",
            QualityTier::Balanced => "balanced",
            QualityTier::Performance => "performance",
            QualityTier::Quality => "quality",
        };

        Ok(RoutingDecision {
            provider: best.record.provider.clone(),
            model_id: best.record.model_id.clone(),
            estimated_cost_eur: best.cost_eur,
            estimated_latency_ms: best.latency_ms,
            tokens_per_second: best.tps,
            reason: format!("{} routing: {:.2}x price + {:.2}x speed + {:.2}x quality = {:.3}",
                tier_label, best.price_score, best.speed_score, best.quality_score, best.total_score),
            alternatives,
        })
    }
}

fn estimated_speed(provider: &str, model_id: &str) -> (f64, u64) {
    match (provider, model_id) {
        ("deepseek", m) if m.contains("v4-flash") => (45.4, 2288),  // Real benchmark Jul 7
        ("deepseek", m) if m.contains("v4-pro") => (25.9, 4567),     // Real benchmark Jul 7
        ("deepseek", m) if m.contains("chat") => (48.5, 2041),        // Deprecated — migrating to V4-Flash
        ("deepseek", m) if m.contains("reasoner") => (50.0, 2235),    // Deprecated — includes thinking bloat
        ("siliconflow", m) if m.contains("8B") || m.contains("9B") => (120.0, 800),
        ("siliconflow", m) if m.contains("Hy3") => (90.0, 1200),
        ("siliconflow", m) if m.contains("GLM") => (58.0, 2000),
        ("zai", m) if m.contains("glm-5.2") => (7.7, 6799),             // Real benchmark Jul 7 via api.z.ai
        ("zai", m) if m.contains("glm-4.7") => (5.1, 15160),            // Real benchmark Jul 7
        ("zai", m) if m.contains("glm-4.5") => (37.3, 2802),            // Real benchmark Jul 7 via api.z.ai
        ("zai", m) if m.contains("glm-4.6v-flashx") => (25.0, 20000),   // Real benchmark Jul 7 — receipt OCR
        ("zai", m) if m.contains("glm-4.6v-flash") => (60.0, 3000),     // Free tier, fast but rate-limited
        ("zai", m) if m.contains("glm-4.6v") => (7.0, 28000),           // Real benchmark Jul 7 — flagship, 4 receipts
        ("zai", _) => (15.0, 5000),
        ("siliconflow", _) => (60.0, 1500),
        ("scaleway", m) if m.contains("qwen3-coder") => (169.4, 536),    // Real benchmark Jul 7
        ("scaleway", m) if m.contains("mistral-small") => (138.7, 844),    // Real benchmark Jul 7
        ("scaleway", m) if m.contains("llama") => (71.0, 943),             // Real benchmark Jul 7
        ("scaleway", m) if m.contains("mistral-medium") => (63.7, 2125),   // Real benchmark Jul 7
        ("scaleway", m) if m.contains("glm") => (22.2, 3030),              // Real benchmark Jul 7
        ("scaleway", m) if m.contains("gemma") => (29.9, 4794),            // Real benchmark Jul 7
        ("scaleway", _) => (50.0, 2000),
        ("openai", m) if m.contains("mini") => (54.0, 1939),          // Real benchmark Jul 7
        ("openai", _) => (30.0, 3000),
        ("anthropic", m) if m.contains("haiku") => (70.0, 1100),
        ("anthropic", _) => (25.0, 4000),
        ("google", m) if m.contains("flash-lite") => (250.0, 4000),  // Real benchmark Jul 8
        ("google", m) if m.contains("flash") => (100.0, 10000),       // Real benchmark Jul 8 (2.5 flash = 20s TTF)
        ("google", m) if m.contains("pro") => (50.0, 15000),
        ("google", _) => (100.0, 8000),
        _ => (50.0, 2000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PriceScraper;

    #[test]
    fn test_routing_budget_tier() {
        let store = MetricsStore::open("/tmp/ohagent_test_router_budget.db").unwrap();
        let scraper = PriceScraper::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(scraper.scrape_all(&store)).unwrap();
        let router = DynamicRouter::new(store);
        let config = RouterConfig { quality_tier: QualityTier::Budget, ..Default::default() };
        let decision = router.route(&["chat"], 1000, 2000, &config, DocumentCount::Unknown).unwrap();
        assert!(
            decision.provider == "siliconflow" || decision.provider == "deepseek" || decision.provider == "scaleway" || decision.provider == "zai" || decision.provider == "google",
            "Budget should pick cheapest, got {}", decision.provider
        );
    }

    #[test]
    fn test_routing_multi_doc_detected() {
        // When pre-classifier finds 4 documents, only multi_doc models are candidates.
        // GLM-4.6V should win because it has multi_doc capability with quality=0.95.
        let store = MetricsStore::open("/tmp/ohagent_test_router_multidoc.db").unwrap();
        let scraper = PriceScraper::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(scraper.scrape_all(&store)).unwrap();
        let router = DynamicRouter::new(store);
        let config = RouterConfig { quality_tier: QualityTier::Quality, ..Default::default() };

        // 4 documents detected → multi_doc routing
        let decision = router.route(&["vision"], 1000, 2000, &config, DocumentCount::Multiple(4)).unwrap();

        // Must be a multi_doc model — GLM-4.6V family
        assert!(
            decision.model_id.contains("glm-4.6v") || decision.model_id.contains("GLM-5V"),
            "Multi-doc should route to GLM-4.6V/GLM-5V, got {} from {}",
            decision.model_id, decision.provider
        );
        println!("Multi-doc (4 receipts) routed to: {}/{} — cost €{:.6}, latency {}ms",
            decision.provider, decision.model_id, decision.estimated_cost_eur, decision.estimated_latency_ms);
    }

    #[test]
    fn test_routing_single_doc_uses_cheapest() {
        // When pre-classifier finds 1 document, cheapest vision model wins (Scaleway Mistral-small).
        let store = MetricsStore::open("/tmp/ohagent_test_router_single.db").unwrap();
        let scraper = PriceScraper::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(scraper.scrape_all(&store)).unwrap();
        let router = DynamicRouter::new(store);
        let config = RouterConfig { quality_tier: QualityTier::Budget, ..Default::default() };

        let decision = router.route(&["vision"], 1000, 2000, &config, DocumentCount::Single).unwrap();

        println!("Single doc routed to: {}/{} — cost €{:.6}",
            decision.provider, decision.model_id, decision.estimated_cost_eur);

        // Single document → should be cheap, not GLM-4.6V full (that's expensive for single doc)
        assert!(
            !decision.model_id.contains("glm-4.6v") || decision.model_id.contains("flash"),
            "Single doc should NOT use expensive GLM-4.6V full, got {}",
            decision.model_id
        );
    }

    #[test]
    fn test_routing_eu_preference() {
        let store = MetricsStore::open("/tmp/ohagent_test_router_eu.db").unwrap();
        let scraper = PriceScraper::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(scraper.scrape_all(&store)).unwrap();
        let router = DynamicRouter::new(store);
        let config = RouterConfig { quality_tier: QualityTier::Balanced, prefer_eu: true, ..Default::default() };
        let decision = router.route(&["chat"], 1000, 2000, &config, DocumentCount::Unknown).unwrap();
        assert_eq!(decision.provider, "scaleway", "EU preference failed, got {}", decision.provider);
    }
}
