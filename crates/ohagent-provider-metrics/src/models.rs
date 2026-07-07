//! Data models: price records, speed benchmarks, routing decisions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A price record scraped from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceRecord {
    pub id: String,
    pub provider: String,
    pub model_id: String,
    pub input_price_per_mtok: f64,
    pub output_price_per_mtok: f64,
    pub currency: String,         // "USD", "EUR", "CNY"
    pub cached_input_price: Option<f64>, // for providers with cache discounts
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub capabilities: Vec<String>, // ["chat", "code", "vision", "audio"]
    pub scraped_at: DateTime<Utc>,
    pub source_url: String,
}

/// A speed benchmark result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedRecord {
    pub id: String,
    pub provider: String,
    pub model_id: String,
    /// Median time to first token (ms)
    pub ttf_ms: u64,
    /// Median total latency (ms) for a ~500 token completion
    pub total_latency_ms: u64,
    /// Tokens per second (output)
    pub tokens_per_second: f64,
    /// P95 latency (ms)
    pub p95_latency_ms: u64,
    /// Prompt tokens used in benchmark
    pub prompt_tokens: u32,
    /// Completion tokens received
    pub completion_tokens: u32,
    /// Number of samples
    pub samples: u32,
    /// When the benchmark ran
    pub measured_at: DateTime<Utc>,
    /// Error message if benchmark failed
    pub error: Option<String>,
}

/// A routing decision: which provider+model to use for a task.
#[derive(Debug, Clone, Serialize)]
pub struct RoutingDecision {
    pub provider: String,
    pub model_id: String,
    pub estimated_cost_eur: f64,
    pub estimated_latency_ms: u64,
    pub tokens_per_second: f64,
    pub reason: String,
    pub alternatives: Vec<RoutingAlternative>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoutingAlternative {
    pub provider: String,
    pub model_id: String,
    pub cost_eur: f64,
    pub latency_ms: u64,
    pub tps: f64,
}

/// Quality tiers for routing preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualityTier {
    Budget,      // Cheapest, decent quality
    Balanced,    // Good balance (default)
    Performance, // Best speed
    Quality,     // Best output quality
}

/// Router configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    pub quality_tier: QualityTier,
    pub max_budget_eur_per_1k: Option<f64>,
    pub prefer_eu: bool,
    pub prefer_open_source: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            quality_tier: QualityTier::Balanced,
            max_budget_eur_per_1k: None,
            prefer_eu: false,
            prefer_open_source: false,
        }
    }
}

/// Provider info with known base URLs and pricing pages.
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub name: String,
    pub api_base_url: Option<String>,
    pub pricing_url: Option<String>,
    pub currency: String,
    pub is_eu: bool,
}

/// Known providers with their metadata.
pub fn known_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo { name: "siliconflow".into(),    api_base_url: Some("https://api.siliconflow.cn/v1".into()),   pricing_url: Some("https://siliconflow.com/models".into()),             currency: "USD".into(), is_eu: false },
        ProviderInfo { name: "zai".into(),            api_base_url: Some("https://open.bigmodel.cn/api/paas/v4".into()), pricing_url: Some("https://open.bigmodel.cn/pricing".into()), currency: "CNY".into(), is_eu: false },
        ProviderInfo { name: "scaleway".into(),       api_base_url: Some("https://api.scaleway.com/generative/v1".into()), pricing_url: Some("https://www.scaleway.com/en/pricing/model-as-a-service/".into()), currency: "EUR".into(), is_eu: true },
        ProviderInfo { name: "deepseek".into(),       api_base_url: Some("https://api.deepseek.com/v1".into()),      pricing_url: Some("https://api-docs.deepseek.com/quick_start/pricing".into()), currency: "EUR".into(), is_eu: false },
        ProviderInfo { name: "openai".into(),         api_base_url: Some("https://api.openai.com/v1".into()),   pricing_url: Some("https://openai.com/api/pricing/".into()),          currency: "USD".into(), is_eu: false },
        ProviderInfo { name: "anthropic".into(),      api_base_url: Some("https://api.anthropic.com/v1".into()), pricing_url: Some("https://www.anthropic.com/pricing".into()),         currency: "USD".into(), is_eu: false },
        ProviderInfo { name: "hetzner".into(),        api_base_url: None,                                            pricing_url: Some("https://www.hetzner.com/cloud".into()),           currency: "EUR".into(), is_eu: true },
        ProviderInfo { name: "groq".into(),           api_base_url: Some("https://api.groq.com/openai/v1".into()), pricing_url: Some("https://groq.com/pricing/".into()),                currency: "USD".into(), is_eu: false },
    ]
}
