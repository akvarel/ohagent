//! Data models: price records, speed benchmarks, routing decisions, provider discovery.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};


// ── Pricing ──

/// How many documents were detected in an image by the pre-classifier.
/// Used to route: Single → cheapest vision model, Multiple → GLM-4.6V.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentCount {
    /// Not yet classified — router falls back to normal vision routing
    Unknown,
    /// Exactly 1 document/receipt/page — use cheapest model
    Single,
    /// N distinct documents (2+) — use multi_doc-capable model (GLM-4.6V)
    Multiple(u8),
}

impl DocumentCount {
    pub fn is_multi(&self) -> bool {
        matches!(self, DocumentCount::Multiple(_))
    }

    pub fn count(&self) -> u8 {
        match self {
            DocumentCount::Unknown => 0,
            DocumentCount::Single => 1,
            DocumentCount::Multiple(n) => *n,
        }
    }
}

impl std::fmt::Display for DocumentCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocumentCount::Unknown => write!(f, "unknown"),
            DocumentCount::Single => write!(f, "single"),
            DocumentCount::Multiple(n) => write!(f, "{} documents", n),
        }
    }
}
/// How the provider charges for this model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingModel {
    /// Per 1M tokens (chat, code models) — input + output prices
    PerMillionTokens,
    /// Per image generated (FLUX, Qwen-Image)
    PerImage,
    /// Per video generated (Wan2.2)
    PerVideo,
    /// Per audio minute (Whisper, TTS)
    PerAudioMinute,
    /// Per 1M UTF-8 bytes (IndexTTS, Fish-Speech)
    PerMillionBytes,
}

impl PricingModel {
    pub fn unit_label(&self) -> &'static str {
        match self {
            PricingModel::PerMillionTokens => "M tokens",
            PricingModel::PerImage => "image",
            PricingModel::PerVideo => "video",
            PricingModel::PerAudioMinute => "audio minute",
            PricingModel::PerMillionBytes => "M UTF-8 bytes",
        }
    }
}

/// A price record scraped from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceRecord {
    pub id: String,
    pub provider: String,
    pub model_id: String,
    pub pricing_model: PricingModel,
    /// Price per unit: per 1M tokens / per image / per video / per audio minute
    pub input_price_per_unit: f64,
    pub output_price_per_unit: f64,
    /// Cached input price (prompt caching discount)
    pub cached_input_price_per_unit: Option<f64>,
    pub currency: String,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub capabilities: Vec<String>,
    pub scraped_at: DateTime<Utc>,
    pub source_url: String,
}

impl PriceRecord {
    /// Estimated cost in EUR for an API call.
    pub fn estimated_cost_eur(&self, prompt_tokens: u64, completion_tokens: u64) -> f64 {
        let rate = match self.currency.as_str() {
            "USD" => 0.92, "CNY" => 0.13, _ => 1.0,
        };
        match self.pricing_model {
            PricingModel::PerMillionTokens => {
                let input_cost = (prompt_tokens as f64 / 1_000_000.0) * self.input_price_per_unit;
                let output_cost = (completion_tokens as f64 / 1_000_000.0) * self.output_price_per_unit;
                (input_cost + output_cost) * rate
            }
            // Non-token models: return input_price as flat per-unit cost
            PricingModel::PerImage | PricingModel::PerVideo | PricingModel::PerAudioMinute | PricingModel::PerMillionBytes => {
                self.input_price_per_unit * rate
            }
        }
    }
}

// ── Speed ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedRecord {
    pub id: String,
    pub provider: String,
    pub model_id: String,
    pub ttf_ms: u64,
    pub total_latency_ms: u64,
    pub tokens_per_second: f64,
    pub p95_latency_ms: u64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub samples: u32,
    pub measured_at: DateTime<Utc>,
    pub error: Option<String>,
}

// ── Routing ──

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualityTier { Budget, Balanced, Performance, Quality }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    pub quality_tier: QualityTier,
    pub max_budget_eur_per_1k: Option<f64>,
    pub prefer_eu: bool,
    pub prefer_open_source: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self { quality_tier: QualityTier::Balanced, max_budget_eur_per_1k: None, prefer_eu: false, prefer_open_source: false }
    }
}

// ── Provider Discovery ──

#[derive(Debug, Clone)]
pub struct ProviderInfo {
    pub name: String,
    /// Auto-discovered API base URL (set to None for manual/project-based URLs)
    pub api_base_url: Option<String>,
    pub pricing_url: Option<String>,
    pub currency: String,
    pub is_eu: bool,
}

/// Known providers with metadata — validated July 2026.
pub fn known_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo { name: "siliconflow".into(), api_base_url: Some("https://api.siliconflow.com/v1".into()), pricing_url: Some("https://siliconflow.com/models".into()), currency: "USD".into(), is_eu: false },
        ProviderInfo { name: "scaleway".into(),    api_base_url: None, /* per-project: https://api.scaleway.ai/<id>/v1 */ pricing_url: Some("https://www.scaleway.com/en/pricing/model-as-a-service/".into()), currency: "EUR".into(), is_eu: true },
        ProviderInfo { name: "deepseek".into(),    api_base_url: Some("https://api.deepseek.com/v1".into()), pricing_url: Some("https://api-docs.deepseek.com/quick_start/pricing".into()), currency: "EUR".into(), is_eu: false },
        ProviderInfo { name: "zai".into(),         api_base_url: Some("https://api.z.ai/api/paas/v4".into()), pricing_url: Some("https://docs.z.ai/api-reference/introduction".into()), currency: "CNY".into(), is_eu: false },
        // GLM-4.6V has its own provider alias — same API as Z.ai but dedicated to vision
        ProviderInfo { name: "glm4v".into(),       api_base_url: Some("https://api.z.ai/api/paas/v4".into()), pricing_url: Some("https://docs.z.ai/guides/vlm/glm-4.6v".into()), currency: "CNY".into(), is_eu: false },
        ProviderInfo { name: "openai".into(),      api_base_url: Some("https://api.openai.com/v1".into()), pricing_url: Some("https://openai.com/api/pricing/".into()), currency: "USD".into(), is_eu: false },
        ProviderInfo { name: "anthropic".into(),   api_base_url: Some("https://api.anthropic.com/v1".into()), pricing_url: Some("https://www.anthropic.com/pricing".into()), currency: "USD".into(), is_eu: false },
        ProviderInfo { name: "groq".into(),        api_base_url: Some("https://api.groq.com/openai/v1".into()), pricing_url: Some("https://groq.com/pricing/".into()), currency: "USD".into(), is_eu: false },
        ProviderInfo { name: "hetzner".into(),     api_base_url: None, /* GPU only, not a chat API */ pricing_url: Some("https://www.hetzner.com/cloud".into()), currency: "EUR".into(), is_eu: true },
    ]
}
