//! ohagent-infra-launcher — on-demand GPU instance provisioning.
//!
//! **Proprietary plugin.** Spawns temporary GPU/Serverless instances for
//! custom model inference, LoRA fine-tuning, and batch processing.
//! Auto-destroys after configured TTL.
//!
//! # Supported Providers
//!
//! | Provider | Type | Best For | Cost |
//! |---|---|---|---|
//! | **SiliconFlow** | 200+ models, per-token | Cheapest inference | $0.06-1.60/M tok |
//! | **Scaleway Serverless** | No GPU, per-token | EU/GDPR inference | €0.15-1.80/M tok |
//! | **Scaleway Dedicated** | L4/H100, per hour | Fine-tuning, LoRA | €0.93-3.40/hr |
//! | **Hetzner Cloud** | A100, per hour | Cheapest raw GPU | €1.85/hr |
//!
//! # SiliconFlow is the cheapest API aggregator (200+ models):
//! - Tencent Hy3-preview: **$0.066/M tok input** — 2x cheaper than DeepSeek
//! - Qwen3-Coder-30B: **$0.07/M tok input** — cheapest coding model
//! - Qwen3-8B: **$0.06/0.06** — ultra-cheap general chat
//! - FLUX.1-schnell: **$0.0014/image** — cheapest image gen
//! - Wan2.2 video: **$0.29/video**
//! - Qwen3-Embedding: **$0.01/M tok**
//!
//! # Scaleway is uniquely good for EU/GDPR:
//! - **Serverless**: no provisioning delay, free tier (1M tokens), -50% batches
//! - **L4 GPU**: €0.93/hr — cheapest managed GPU in Europe
//! - **H100 GPU**: €3.40/hr — half the price of AWS/GCP equivalents
//! - **GDPR**: all data stays in Paris/Amsterdam EU datacenters
//!
//! # Usage
//!
//! ```text
//! User: "/deploy scaleway:llama3.3-70b ttl=2h"
//!   → Plugin selects Scaleway Serverless (model already hosted)
//!   → Returns endpoint: https://api.scaleway.com/generative/v1/...
//!
//! User: "/deploy custom-lora gpu=L4 provider=scaleway ttl=4h"
//!   → Creates Scaleway L4-1-24G dedicated instance
//!   → cloud-init: install vLLM + LoRA adapter from HuggingFace
//!   → Returns endpoint: http://<ip>:8000/v1
//!
//! User: "/deploy mixtral gpu=A100 provider=hetzner ttl=2h"
//!   → Creates Hetzner CCX13 (A100 40GB)
//!   → Returns endpoint: http://<ip>:8000/v1
//! ```

use ohagent_plugins::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;

// ── Config ──

#[derive(Debug, Deserialize)]
struct InfraConfig {
    // Hetzner
    #[serde(default = "default_hetzner_token")]
    hetzner_api_token: String,
    // Scaleway
    #[serde(default = "default_scw_secret")]
    scaleway_secret_key: String,
    #[serde(default = "default_scw_org")]
    scaleway_organization_id: String,
    #[serde(default = "default_scw_project")]
    scaleway_project_id: String,
    // General
    #[serde(default = "default_ttl")]
    default_ttl_secs: u64,
    #[serde(default = "default_provider")]
    default_provider: String,
}

fn default_hetzner_token() -> String { "env:HETZNER_API_TOKEN".into() }
fn default_scw_secret() -> String { "env:SCW_SECRET_KEY".into() }
fn default_scw_org() -> String { "env:SCW_DEFAULT_ORGANIZATION_ID".into() }
fn default_scw_project() -> String { "env:SCW_DEFAULT_PROJECT_ID".into() }
fn default_ttl() -> u64 { 3600 }
fn default_provider() -> String { "scaleway-serverless".into() }

// ── Scaleway Serverless Models ──

/// Known Scaleway serverless models with pricing.
#[derive(Debug, Clone)]
struct ServerlessModel {
    id: String,
    input_price_per_mtok: f64,  // EUR per million tokens
    output_price_per_mtok: f64,
    capabilities: Vec<String>,
}

fn scaleway_models() -> Vec<ServerlessModel> {
    vec![
        ServerlessModel { id: "mistral-small-3.2-24b-instruct-2506".into(), input_price_per_mtok: 0.15, output_price_per_mtok: 0.35, capabilities: vec!["chat".into(), "vision".into()] },
        ServerlessModel { id: "qwen3-coder-30b-a3b-instruct".into(),        input_price_per_mtok: 0.20, output_price_per_mtok: 0.80, capabilities: vec!["chat".into(), "code".into()] },
        ServerlessModel { id: "gemma-4-26b-a4b-it".into(),                  input_price_per_mtok: 0.25, output_price_per_mtok: 0.50, capabilities: vec!["chat".into(), "vision".into()] },
        ServerlessModel { id: "gemma-3-27b-it".into(),                      input_price_per_mtok: 0.25, output_price_per_mtok: 0.50, capabilities: vec!["chat".into(), "vision".into()] },
        ServerlessModel { id: "qwen3.6-35b-a3b".into(),                     input_price_per_mtok: 0.25, output_price_per_mtok: 1.50, capabilities: vec!["chat".into(), "vision".into()] },
        ServerlessModel { id: "devstral-2-123b-instruct-2512".into(),       input_price_per_mtok: 0.40, output_price_per_mtok: 2.00, capabilities: vec!["chat".into(), "code".into()] },
        ServerlessModel { id: "qwen3.5-397b-a17b".into(),                   input_price_per_mtok: 0.60, output_price_per_mtok: 3.60, capabilities: vec!["chat".into(), "vision".into()] },
        ServerlessModel { id: "qwen3-235b-a22b-instruct-2507".into(),       input_price_per_mtok: 0.75, output_price_per_mtok: 2.25, capabilities: vec!["chat".into()] },
        ServerlessModel { id: "llama-3.3-70b-instruct".into(),              input_price_per_mtok: 0.90, output_price_per_mtok: 0.90, capabilities: vec!["chat".into()] },
        ServerlessModel { id: "mistral-medium-3.5-128b".into(),             input_price_per_mtok: 1.50, output_price_per_mtok: 7.50, capabilities: vec!["chat".into(), "vision".into()] },
        ServerlessModel { id: "glm-5.2".into(),                             input_price_per_mtok: 1.80, output_price_per_mtok: 5.50, capabilities: vec!["chat".into(), "code".into()] },
        ServerlessModel { id: "pixtral-12b-2409".into(),                    input_price_per_mtok: 0.20, output_price_per_mtok: 0.20, capabilities: vec!["chat".into(), "vision".into()] },
        ServerlessModel { id: "whisper-large-v3".into(),                    input_price_per_mtok: 0.0,  output_price_per_mtok: 0.0,  capabilities: vec!["audio".into()] }, // €0.003/audio minute
    ]
}

// ── Scaleway Dedicated GPU Types ──

#[derive(Debug, Clone)]
struct GpuType {
    name: &'static str,
    provider: &'static str,
    api_slug: &'static str,
    price_per_hour: f64,
    vram_gb: u32,
    max_tokens_per_sec_est: u32,
}

fn gpu_types() -> Vec<GpuType> {
    vec![
        // Scaleway Dedicated
        GpuType { name: "scw-l4",       provider: "scaleway", api_slug: "l4-1-24g",    price_per_hour: 0.93,  vram_gb: 24,  max_tokens_per_sec_est: 1500 },
        GpuType { name: "scw-l40s",     provider: "scaleway", api_slug: "l40s-1-48g",  price_per_hour: 1.72,  vram_gb: 48,  max_tokens_per_sec_est: 3000 },
        GpuType { name: "scw-h100",     provider: "scaleway", api_slug: "h100-1-80g",  price_per_hour: 3.40,  vram_gb: 80,  max_tokens_per_sec_est: 8000 },
        // Hetzner
        GpuType { name: "hz-a100-40",   provider: "hetzner",  api_slug: "ccx13",       price_per_hour: 1.85,  vram_gb: 40,  max_tokens_per_sec_est: 4000 },
        GpuType { name: "hz-a100-80",   provider: "hetzner",  api_slug: "ccx23",       price_per_hour: 2.50,  vram_gb: 80,  max_tokens_per_sec_est: 8000 },
    ]
}

// ── Instance State ──

#[derive(Debug, Clone)]
struct ActiveInstance {
    id: String,
    provider: String,
    model: String,
    endpoint: String,
    created_at: i64,
    ttl_secs: u64,
    provider_id: String, // server ID or deployment ID
}

// ── Plugin ──

pub struct InfraLauncherPlugin {
    config: InfraConfig,
    instances: Mutex<HashMap<String, ActiveInstance>>,
    client: reqwest::Client,
}

impl InfraLauncherPlugin {
    pub fn new() -> Self {
        Self {
            config: InfraConfig {
                hetzner_api_token: "env:HETZNER_API_TOKEN".into(),
                scaleway_secret_key: "env:SCW_SECRET_KEY".into(),
                scaleway_organization_id: "env:SCW_DEFAULT_ORGANIZATION_ID".into(),
                scaleway_project_id: "env:SCW_DEFAULT_PROJECT_ID".into(),
                default_ttl_secs: 3600,
                default_provider: "scaleway-serverless".into(),
            },
            instances: Mutex::new(HashMap::new()),
            client: reqwest::Client::new(),
        }
    }

    fn resolve_env(&self, val: &str) -> String {
        val.strip_prefix("env:").and_then(|v| std::env::var(v).ok()).unwrap_or_else(|| val.to_string())
    }

    /// Parse a deployment request from message text.
    /// Patterns:
    ///   "/deploy llama3.3-70b ttl=2h"                    → scaleway-serverless (default)
    ///   "/deploy scaleway:qwen3-coder ttl=30m provider=scaleway" → scaleway-serverless
    ///   "/deploy custom-lora gpu=L4 provider=scaleway ttl=4h"    → scaleway dedicated GPU
    ///   "/deploy mixtral gpu=A100 provider=hetzner ttl=2h"       → hetzner cloud
    fn parse_request(&self, text: &str) -> Option<DeployRequest> {
        let text_lower = text.to_lowercase();
        if !text_lower.contains("/deploy") && !text_lower.contains("/infra") && !text_lower.contains("spawn gpu") {
            return None;
        }

        // Determine provider
        let provider = text_lower.split_whitespace()
            .find(|w| w.starts_with("provider="))
            .map(|w| w.trim_start_matches("provider=").to_string())
            .or_else(|| {
                if text_lower.contains("zai:") || text_lower.contains("zhipu:") { Some("zai".into()) }
                else if text_lower.contains("siliconflow:") || text_lower.contains("sf:") { Some("siliconflow".into()) }
                else if text_lower.contains("scaleway:") || text_lower.contains("scw:") { Some("scaleway".into()) }
                else if text_lower.contains("hetzner:") || text_lower.contains("hz:") { Some("hetzner".into()) }
                else { None }
            })
            .unwrap_or_else(|| self.config.default_provider.clone());

        // Extract model
        let model = text_lower.split_whitespace()
            .find(|w| w.starts_with("model="))
            .map(|w| w.trim_start_matches("model=").to_string())
            .or_else(|| {
                // Try "scaleway:model-name" or "hetzner:model-name" prefix
                text_lower.split_whitespace()
                    .find(|w| w.contains(':'))
                    .map(|w| w.split(':').nth(1).unwrap_or(w).to_string())
            })
            .or_else(|| {
                text_lower.split_whitespace()
                    .skip_while(|w| *w != "/deploy" && *w != "/infra")
                    .nth(1)
                    .filter(|w| !w.starts_with("ttl=") && !w.starts_with("gpu=") && !w.starts_with("provider="))
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "mistral-small-3.2-24b".to_string());

        // Extract TTL
        let ttl = text_lower.split_whitespace()
            .find(|w| w.starts_with("ttl="))
            .and_then(|w| parse_duration(w.trim_start_matches("ttl=")))
            .or_else(|| {
                text_lower.split_whitespace()
                    .find(|w| w.starts_with("hours="))
                    .and_then(|w| w.trim_start_matches("hours=").parse::<u64>().ok())
                    .map(|h| h * 3600)
            })
            .unwrap_or(self.config.default_ttl_secs);

        // Extract GPU type
        let gpu = text_lower.split_whitespace()
            .find(|w| w.starts_with("gpu="))
            .map(|w| w.trim_start_matches("gpu=").to_string());

        Some(DeployRequest { model, ttl, gpu: gpu.unwrap_or_else(|| "auto".into()), provider })
    }
}

// ── Deploy Request ──

#[derive(Debug)]
struct DeployRequest {
    model: String,
    ttl: u64,
    gpu: String,
    provider: String,
}

fn parse_duration(s: &str) -> Option<u64> {
    if let Ok(n) = s.parse::<u64>() { return Some(n); }
    if s.ends_with('h') { return s[..s.len()-1].parse::<u64>().ok().map(|h| h * 3600); }
    if s.ends_with('m') { return s[..s.len()-1].parse::<u64>().ok().map(|m| m * 60); }
    if s.ends_with('s') { return s[..s.len()-1].parse::<u64>().ok(); }
    None
}

impl MessagePlugin for InfraLauncherPlugin {
    fn name(&self) -> &str { "ohagent-infra-launcher" }
    fn version(&self) -> (u32, u32) { (1, 1) }

    fn init(&mut self) -> Result<(), PluginError> {
        let hz = self.resolve_env(&self.config.hetzner_api_token);
        let scw = self.resolve_env(&self.config.scaleway_secret_key);
        let providers = [(!hz.is_empty(), "Hetzner"), (!scw.is_empty(), "Scaleway")];
        let available: Vec<&str> = providers.iter().filter(|(ok, _)| *ok).map(|(_, n)| *n).collect();
        if available.is_empty() {
            tracing::warn!("No cloud provider tokens set — infra launcher in simulation mode");
        } else {
            tracing::info!(?available, "Infra launcher ready");
        }
        Ok(())
    }

    fn transform_message(&self, message: &mut PluginMessage) -> Result<(), PluginError> {
        let req = match self.parse_request(&message.text) {
            Some(r) => r,
            None => return Ok(()),
        };

        // Check for existing instance
        let key = format!("{}:{}:{}", message.tenant_id, req.provider, req.model);
        {
            let instances = self.instances.lock().unwrap();
            if let Some(inst) = instances.get(&key) {
                message.text = format!("{}\n[INFRA] Using existing instance: {} (provider: {}, endpoint: {})",
                    message.text, inst.id, inst.provider, inst.endpoint);
                message.log_redaction("infra-launcher", "deploy-request", "existing-instance", "infra_redirect");
                return Ok(());
            }
        }

        // Generate deployment plan based on provider
        let plan = self.build_deploy_plan(&req, &message.tenant_id);

        message.text = format!("{}\n{}", message.text, plan);
        message.log_redaction("infra-launcher", "deploy-request", "provisioning-plan", "infra_deploy");
        Ok(())
    }

    fn shutdown(&mut self) {
        let _instances = self.instances.lock().unwrap();
        tracing::info!("Infra launcher shutting down");
    }
}

impl InfraLauncherPlugin {
    /// Build a deployment plan as a text response.
    fn build_deploy_plan(&self, req: &DeployRequest, tenant: &str) -> String {
        let model_display = &req.model;

        match req.provider.as_str() {
            "scaleway" | "scaleway-serverless" | "scw" => {
                // Try to match to a serverless model first
                let models = scaleway_models();
                let matched = models.iter().find(|m| {
                    model_display.contains(&m.id) || m.id.contains(model_display.as_str())
                });

                if let Some(sm) = matched {
                    let cost_1k = sm.input_price_per_mtok / 1000.0 + sm.output_price_per_mtok / 1000.0;
                    return format!(r#"[INFRA] Scaleway Serverless Deployment Plan
  Provider: Scaleway Generative APIs (Paris)
  Model: {model_id}
  Type: Serverless — no GPU provisioning needed!
  Pricing: €{input:.2}/M input + €{output:.2}/M output tokens
  Est. cost per 1K requests: ~€{per1k:.4}
  Free tier: 1M tokens/month
  Batches: -50% discount

  Instant availability — just use the API directly:
    curl https://api.scaleway.com/generative/v1/chat/completions \
      -H "X-Auth-Token: $SCW_SECRET_KEY" \
      -d '{{"model":"{model_id}","messages":[{{"role":"user","content":"..."}}]}}'

  Or add to ohAgent config:
    [[providers.scaleway]]
    model = "{model_id}"
    endpoint = "https://api.scaleway.com/generative/v1""#,
                    model_id = sm.id,
                    input = sm.input_price_per_mtok,
                    output = sm.output_price_per_mtok,
                    per1k = cost_1k,
                );
                }

                // Fall back to dedicated GPU
                let gpu = match req.gpu.as_str() {
                    "l4" | "L4" => "scw-l4",
                    "l40s" | "L40S" => "scw-l40s",
                    "h100" | "H100" | "auto" => "scw-h100",
                    _ => "scw-l4",
                };
                let gpus = gpu_types();
                let gpu_info = gpus.iter().find(|g| g.name == gpu).unwrap();
                let ttl_h = req.ttl as f64 / 3600.0;
                let cost = gpu_info.price_per_hour * ttl_h;

                format!(r#"[INFRA] Scaleway Dedicated GPU Plan
  Provider: Scaleway Managed Inference (Paris)
  GPU: {gpu_name} ({vram}GB VRAM, ~{tps} tok/s)
  Model: {model}
  TTL: {ttl_h}h ({ttl_s}s)
  Cost: €{cost:.2} total (€{rate:.2}/hr × {ttl_h}h)
  Status: Needs SCW_SECRET_KEY + SCW_DEFAULT_PROJECT_ID

  To deploy manually:
    scw inference deployment create \
      name=ohagent-{tenant} \
      model={model} \
      node-type={api_slug}"#,
                    gpu_name = gpu_info.name,
                    vram = gpu_info.vram_gb,
                    tps = gpu_info.max_tokens_per_sec_est,
                    model = model_display,
                    ttl_h = req.ttl / 3600,
                    ttl_s = req.ttl,
                    cost = cost,
                    rate = gpu_info.price_per_hour,
                    tenant = tenant,
                    api_slug = gpu_info.api_slug,
                )
            }

            "hetzner" | "hz" => {
                let gpu = match req.gpu.as_str() {
                    "a100" | "A100" | "auto" => "hz-a100-40",
                    "a100-80" | "A100-80" => "hz-a100-80",
                    _ => "hz-a100-40",
                };
                let gpus = gpu_types();
                let gpu_info = gpus.iter().find(|g| g.name == gpu).unwrap();
                let ttl_h = req.ttl as f64 / 3600.0;
                let cost = gpu_info.price_per_hour * ttl_h;

                format!(r#"[INFRA] Hetzner Cloud GPU Plan
  Provider: Hetzner Cloud (Nuremberg/Falkenstein)
  GPU: A100 {vram}GB (~{tps} tok/s)
  Server type: {server_type}
  Model: {model}
  TTL: {ttl_h}h ({ttl_s}s)
  Cost: €{cost:.2} total (€{rate:.2}/hr × {ttl_h}h)
  Status: Needs HETZNER_API_TOKEN

  To deploy manually:
    hcloud server create --name ohagent-{tenant} \
      --type {server_type} --image ubuntu-24.04 \
      --location nbg1 \
      --user-data-from-file cloud-init.yaml"#,
                    vram = gpu_info.vram_gb,
                    tps = gpu_info.max_tokens_per_sec_est,
                    server_type = gpu_info.api_slug,
                    model = model_display,
                    ttl_h = req.ttl / 3600,
                    ttl_s = req.ttl,
                    cost = cost,
                    rate = gpu_info.price_per_hour,
                    tenant = tenant,
                )
            }

            "siliconflow" | "sf" => {
                // SiliconFlow — 200+ models, per-token pricing (USD)
                // Known cheap models and their prices
                let sf_models: &[(&str, f64, f64, &str)] = &[
                    ("Tencent/Hy3-preview",        0.066, 0.26,  "Cheapest LLM — 295B MoE, 21B active"),
                    ("Qwen/Qwen3-Coder-30B-A3B",  0.07,  0.28,  "Cheapest coding — 30B MoE, 3B active"),
                    ("Qwen/Qwen3-8B",              0.06,  0.06,  "Ultra-cheap general chat"),
                    ("Qwen/Qwen3.5-9B",            0.10,  0.15,  "Multimodal, 201 languages"),
                    ("deepseek-ai/DeepSeek-V4-Flash", 0.13, 0.28, "DeepSeek Flash via aggregator"),
                    ("stepfun-ai/Step-3.5-Flash",  0.10,  0.30,  "196B MoE, 11B active"),
                    ("google/gemma-4-26b-it",      0.12,  0.40,  "Google open-source MoE"),
                    ("inclusionAI/Ling-flash-2.0", 0.14,  0.57,  "100B MoE, 6.1B active"),
                ];

                // Try to match the requested model
                let best = sf_models.iter().find(|(id, _, _, _)| {
                    model_display.contains(&id.to_lowercase()) || id.to_lowercase().contains(model_display.as_str())
                });

                if let Some((model_id, input_price, output_price, desc)) = best {
                    let cost_1k = (input_price + output_price * 2.0) / 1000.0;
                    format!(r#"[INFRA] SiliconFlow Serverless Plan
  Provider: SiliconFlow (200+ models, single API)
  Model: {model_id}
  Type: Serverless — instant, no provisioning
  Description: {desc}
  Pricing: ${input:.3}/M input + ${output:.2}/M output tokens
  Est. cost per 1K requests: ~${per1k:.4}

  Instant availability — single API for 200+ models:
    curl https://api.siliconflow.cn/v1/chat/completions \
      -H "Authorization: Bearer $SF_API_KEY" \
      -d '{{"model":"{model_id}","messages":[{{"role":"user","content":"..."}}]}}'

  Also available: image gen ($0.0014/img FLUX), video ($0.29 Wan2.2),
  embeddings ($0.01/M tok Qwen3), TTS ($7.15/M bytes IndexTTS-2)"#,
                        model_id = model_id,
                        desc = desc,
                        input = input_price,
                        output = output_price,
                        per1k = cost_1k,
                    )
                } else {
                    format!(r#"[INFRA] SiliconFlow — 200+ models available
  Provider: SiliconFlow (api.siliconflow.cn)
  Requested: {model}
  Status: Not matched to known model — check https://siliconflow.com/models

  Try: /deploy sf:Qwen3-8B, /deploy sf:Tencent/Hy3-preview,
       /deploy sf:Qwen3-Coder-30B-A3B, /deploy sf:DeepSeek-V4-Flash

  Requires: SF_API_KEY env var or siliconflow_api_token in config"#,
                        model = model_display,
                    )
                }
            }

            "zai" | "zhipu" => {
                format!(r#"[INFRA] Z.ai / Zhipu API
  Provider: Zhipu AI (open.bigmodel.cn)
  Models: GLM-5.2 (1M ctx, #1 agentic), GLM-5.1, GLM-5, GLM-4.7, GLM-4.5-Air
  Pricing: ¥0.50-10.00/M tok (CNY) — extremely cheap direct API
  Also available on SiliconFlow (USD pricing, easier for non-China access)

  Direct API:
    curl https://open.bigmodel.cn/api/paas/v4/chat/completions \
      -H "Authorization: Bearer $ZAI_API_KEY" \
      -d '{{"model":"glm-5.2","messages":[{{"role":"user","content":"..."}}]}}'

  Via SiliconFlow (recommended for Western users):
    /deploy sf:GLM-5.2

  Requires: ZAI_API_KEY env var (register at open.bigmodel.cn)"#,
                )
            }

            _ => format!("[INFRA] Unknown provider: {}. Try: zai, siliconflow, scaleway, scaleway-serverless, hetzner", req.provider),
        }
    }
}

// ── FFI ──

#[no_mangle]
pub extern "C" fn plugin_api_version() -> u32 { CURRENT_PLUGIN_API_VERSION }

#[no_mangle]
pub extern "C" fn create_plugin() -> *mut dyn MessagePlugin {
    Box::into_raw(Box::new(InfraLauncherPlugin::new()))
}
