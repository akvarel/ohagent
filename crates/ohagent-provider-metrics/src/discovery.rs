//! Provider self-discovery — probes URL patterns, validates models, finds pricing.

use crate::models::{PriceRecord, PricingModel};

/// Pattern sets for each provider: multiple base URLs to try, `/models` endpoint.
/// Ordered by likelihood — first match wins.
pub fn probe_patterns(provider: &str) -> Vec<String> {
    match provider {
        "siliconflow" => vec![
            "https://api.siliconflow.com/v1/models".into(),
            "https://api.siliconflow.cn/v1/models".into(),
        ],
        "zai" | "zhipu" => vec![
            "https://api.z.ai/api/paas/v4/models".into(),
            "https://open.bigmodel.cn/api/paas/v4/models".into(),
        ],
        "deepseek" => vec![
            "https://api.deepseek.com/v1/models".into(),
        ],
        "openai" => vec![
            "https://api.openai.com/v1/models".into(),
        ],
        "anthropic" => vec![
            "https://api.anthropic.com/v1/models".into(),
        ],
        "groq" => vec![
            "https://api.groq.com/openai/v1/models".into(),
        ],
        "scaleway" => vec![],
        _ => vec![],
    }
}

/// Discover the real model IDs and pricing from a provider's API.
/// Returns: Vec of (model_id, supports_chat, supports_vision, supports_audio)
pub async fn discover_models(
    provider: &str,
    api_base: &str,
    api_key: &str,
) -> Result<Vec<(String, Vec<String>)>, String> {
    let url = format!("{api_base}/models");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {} from {url}", resp.status()));
    }

    let body: serde_json::Value = resp.json().await.map_err(|e| format!("JSON: {e}"))?;

    // OpenAI-compatible format: {"data": [{"id": "..."}]}
    let models: Vec<(String, Vec<String>)> = body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    // Skip non-chat models
                    if id.contains("embedding") || id.contains("reranker") || id.contains("moderation") {
                        return None;
                    }
                    let mut caps = vec!["chat".to_string()]; // Assume all support chat
                    if id.to_lowercase().contains("vision") || id.to_lowercase().contains("vl") {
                        caps.push("vision".to_string());
                    }
                    if id.to_lowercase().contains("code") || id.to_lowercase().contains("coder") {
                        caps.push("code".to_string());
                    }
                    Some((id, caps))
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// Known correct pricing by provider+model — from our benchmark data + official pricing pages.
/// These are validated against provider docs (July 7, 2026).
pub fn known_prices() -> Vec<PriceRecord> {
    let now = chrono::Utc::now();
    vec![
        // ═══ DeepSeek (direct) ═══
        pr("deepseek", "deepseek-v4-flash",   PricingModel::PerMillionTokens, 0.14, 0.28, Some(0.028), "EUR", 1_049_000, 393_000, &["chat"], now),
        pr("deepseek", "deepseek-v4-pro",     PricingModel::PerMillionTokens, 1.60, 3.14, Some(0.135), "EUR", 1_049_000, 393_000, &["chat","code"], now),
        pr("deepseek", "deepseek-chat",       PricingModel::PerMillionTokens, 0.27, 1.10, Some(0.135), "EUR",   164_000, 164_000, &["chat","code"], now),
        pr("deepseek", "deepseek-reasoner",   PricingModel::PerMillionTokens, 0.55, 2.19, None,        "EUR",   164_000, 164_000, &["chat","code"], now),

        // ═══ Scaleway (EU serverless) ═══
        pr("scaleway", "mistral-small-3.2-24b-instruct-2506", PricingModel::PerMillionTokens, 0.15, 0.35, None, "EUR", 131_000, 131_000, &["chat","vision"], now),
        pr("scaleway", "qwen3-coder-30b-a3b-instruct",       PricingModel::PerMillionTokens, 0.20, 0.80, None, "EUR", 131_000, 131_000, &["chat","code"], now),
        pr("scaleway", "qwen3.6-35b-a3b",                    PricingModel::PerMillionTokens, 0.25, 1.50, None, "EUR", 131_000, 131_000, &["chat","vision"], now),
        pr("scaleway", "llama-3.3-70b-instruct",             PricingModel::PerMillionTokens, 0.90, 0.90, None, "EUR", 131_000, 131_000, &["chat"], now),
        pr("scaleway", "qwen3.5-397b-a17b",                  PricingModel::PerMillionTokens, 0.60, 3.60, None, "EUR", 131_000, 131_000, &["chat","vision"], now),
        pr("scaleway", "pixtral-12b-2409",                   PricingModel::PerMillionTokens, 0.20, 0.20, None, "EUR", 131_000, 131_000, &["chat","vision"], now),
        pr("scaleway", "gemma-4-26b-a4b-it",                 PricingModel::PerMillionTokens, 0.25, 0.50, None, "EUR", 131_000, 131_000, &["chat","vision"], now),
        pr("scaleway", "gemma-3-27b-it",                     PricingModel::PerMillionTokens, 0.25, 0.50, None, "EUR", 131_000, 131_000, &["chat","vision"], now),
        pr("scaleway", "mistral-medium-3.5-128b",            PricingModel::PerMillionTokens, 1.50, 7.50, None, "EUR", 131_000, 131_000, &["chat","vision"], now),
        pr("scaleway", "glm-5.2",                            PricingModel::PerMillionTokens, 1.30, 4.09, None, "EUR",1_049_000,262_000, &["chat","code"], now),
        // Audio (per-minute, not per-token)
        pr("scaleway", "whisper-large-v3",                   PricingModel::PerAudioMinute, 0.003, 0.0, None, "EUR", 0, 0, &["audio"], now),

        // ═══ SiliconFlow (CN aggregator, USD) ═══
        pr("siliconflow", "tencent/Hy3-preview",           PricingModel::PerMillionTokens, 0.066, 0.26, Some(0.029), "USD", 262_000, 262_000, &["chat","code"], now),
        pr("siliconflow", "Qwen/Qwen3-Coder-30B-A3B-Instruct", PricingModel::PerMillionTokens, 0.07, 0.28, None, "USD", 262_000, 262_000, &["chat","code"], now),
        pr("siliconflow", "Qwen/Qwen3-8B",                 PricingModel::PerMillionTokens, 0.06, 0.06, None, "USD", 131_000, 131_000, &["chat"], now),
        pr("siliconflow", "Qwen/Qwen3.5-9B",               PricingModel::PerMillionTokens, 0.10, 0.15, None, "USD", 262_000, 262_000, &["chat","vision"], now),
        pr("siliconflow", "deepseek-ai/DeepSeek-V4-Flash", PricingModel::PerMillionTokens, 0.13, 0.28, Some(0.028),"USD",1_049_000,393_000, &["chat"], now),
        pr("siliconflow", "inclusionAI/Ling-flash-2.0",    PricingModel::PerMillionTokens, 0.14, 0.57, None, "USD", 131_000, 131_000, &["chat"], now),
        pr("siliconflow", "zai-org/GLM-5.2",               PricingModel::PerMillionTokens, 1.30, 4.09, Some(0.26), "USD",1_049_000,262_000, &["chat","code"], now),
        pr("siliconflow", "google/gemma-4-26B-A4B-it",     PricingModel::PerMillionTokens, 0.12, 0.40, None, "USD", 262_000, 262_000, &["chat","vision"], now),
        pr("siliconflow", "MiniMaxAI/MiniMax-M3",          PricingModel::PerMillionTokens, 0.30, 1.20, Some(0.06), "USD",1_049_000,131_000, &["chat","code"], now),
        // Image (per-image, not per-token)
        pr("siliconflow", "black-forest-labs/FLUX.1-schnell", PricingModel::PerImage, 0.0014, 0.0, None, "USD", 0, 0, &["image"], now),
        pr("siliconflow", "black-forest-labs/FLUX.1-dev",     PricingModel::PerImage, 0.014, 0.0, None, "USD", 0, 0, &["image"], now),
        // Video (per-video)
        pr("siliconflow", "Wan-AI/Wan2.2-T2V-A14B",           PricingModel::PerVideo, 0.29, 0.0, None, "USD", 0, 0, &["video"], now),
        // TTS (per 1M bytes)
        pr("siliconflow", "IndexTeam/IndexTTS-2",              PricingModel::PerMillionBytes, 7.15, 0.0, None, "USD", 0, 0, &["audio"], now),

        // ═══ Z.ai / Zhipu (CNY, direct) ═══
        pr("zai", "glm-5.2",       PricingModel::PerMillionTokens, 1.30, 4.09, Some(0.26), "CNY",1_049_000,262_000, &["chat","code"], now),
        pr("zai", "glm-4.7",       PricingModel::PerMillionTokens, 0.39, 1.67, None,        "CNY",  205_000,205_000, &["chat","code"], now),
        pr("zai", "glm-4.5-air",   PricingModel::PerMillionTokens, 0.14, 0.86, None,        "CNY",  131_000,131_000, &["chat"], now),

        // ═══ OpenAI (USD) ═══
        pr("openai", "gpt-4o-mini",    PricingModel::PerMillionTokens, 0.15, 0.60, Some(0.075), "USD", 128_000, 16_384, &["chat","vision","code"], now),
        pr("openai", "gpt-4o",         PricingModel::PerMillionTokens, 2.50, 10.00,Some(1.25),  "USD", 128_000, 16_384, &["chat","vision","code"], now),

        // ═══ Anthropic (USD) ═══
        pr("anthropic", "claude-haiku-3-5",  PricingModel::PerMillionTokens, 1.00, 5.00,  None, "USD", 200_000, 8_192, &["chat"], now),
        pr("anthropic", "claude-sonnet-4",   PricingModel::PerMillionTokens, 3.00, 15.00, None, "USD", 200_000, 8_192, &["chat","code"], now),
        pr("anthropic", "claude-opus-4",     PricingModel::PerMillionTokens, 15.00,75.00, None, "USD", 200_000, 8_192, &["chat","code"], now),

        // ═══ Groq (USD) ═══
        pr("groq", "llama-3.3-70b-versatile", PricingModel::PerMillionTokens, 0.59, 0.79, None, "USD", 128_000, 32_768, &["chat"], now),
        pr("groq", "mixtral-8x7b-32768",       PricingModel::PerMillionTokens, 0.24, 0.24, None, "USD", 32_000, 32_768, &["chat"], now),
    ]
}

fn pr(
    provider: &str, model: &str, model_type: PricingModel,
    input: f64, output: f64, cached: Option<f64>,
    currency: &str, ctx: u64, max_out: u64, caps: &[&str],
    when: chrono::DateTime<chrono::Utc>,
) -> PriceRecord {
    PriceRecord {
        id: format!("known:{}:{}", provider, model.replace('/', "_").replace(':', "_")),
        provider: provider.to_string(),
        model_id: model.to_string(),
        pricing_model: model_type,
        input_price_per_unit: input,
        output_price_per_unit: output,
        cached_input_price_per_unit: cached,
        currency: currency.to_string(),
        context_window: Some(ctx),
        max_output_tokens: Some(max_out),
        capabilities: caps.iter().map(|s| s.to_string()).collect(),
        scraped_at: when,
        source_url: format!("https://docs.ohagent.dev/pricing#/{}", provider),
    }
}
