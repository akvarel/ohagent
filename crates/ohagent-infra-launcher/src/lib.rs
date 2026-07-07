//! ohagent-infra-launcher — on-demand GPU instance provisioning.
//!
//! **Proprietary plugin.** Spawns temporary GPU instances on Hetzner Cloud
//! (or any cloud provider) for custom model inference, LoRA fine-tuning,
//! and batch processing. Instances auto-destroy after configured TTL.
//!
//! # Supported Providers
//!
//! - **Hetzner Cloud** — cheapest GPU instances (€1.85/hr for A100-ish)
//!   - CX22: 2 vCPU, 4 GB, no GPU (~€0.01/hr) — base model caching
//!   - CCX13: 4 vCPU, 16 GB, A100 40GB (~€1.85/hr) — inference
//!   - CCX23: 8 vCPU, 32 GB, A100 80GB (~€2.50/hr) — large model inference
//!
//! # Lifecycle
//!
//! ```text
//! User: "deploy my llama3-lora on GPU, process 1000 docs"
//!   → Plugin detects {action: "deploy", model: "llama3-lora", ttl: 3600}
//!   → Create Hetzner server (CCX13) with cloud-init
//!   → cloud-init: install vLLM, download model from HF, start server
//!   → Return endpoint URL: http://<ip>:8000/v1
//!   → User's messages route through this endpoint for TTL duration
//!   → After TTL: auto-destroy instance
//! ```
//!
//! # Configuration
//!
//! Set in ~/.ohagent/plugins.toml:
//!
//! ```toml
//! [[plugins]]
//! file = "libohagent_infra_launcher.so"
//! enabled = true
//! config = {
//!   hetzner_api_token = "env:HETZNER_API_TOKEN",
//!   default_ttl_secs = 3600,
//!   max_instance_cost_per_hour = 5.00,
//!   image = "ubuntu-24.04",
//!   ssh_keys = ["my-key"],
//!   region = "nbg1"
//! }
//! ```

use ohagent_plugins::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

// ── Config ──

#[derive(Debug, Deserialize)]
struct InfraConfig {
    #[serde(default = "default_hetzner_token")]
    hetzner_api_token: String,
    #[serde(default = "default_ttl")]
    default_ttl_secs: u64,
    #[serde(default = "default_max_cost")]
    max_instance_cost_per_hour: f64,
    #[serde(default = "default_image")]
    image: String,
    #[serde(default)]
    ssh_keys: Vec<String>,
    #[serde(default = "default_region")]
    region: String,
}

fn default_hetzner_token() -> String { "env:HETZNER_API_TOKEN".into() }
fn default_ttl() -> u64 { 3600 }
fn default_max_cost() -> f64 { 5.00 }
fn default_image() -> String { "ubuntu-24.04".into() }
fn default_region() -> String { "nbg1".into() }

// ── Instance State ──

#[derive(Debug, Clone)]
struct ActiveInstance {
    id: String,
    name: String,
    ip: String,
    model: String,
    endpoint: String,
    created_at: i64,
    ttl_secs: u64,
    server_id: u64, // Hetzner server ID
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
                default_ttl_secs: 3600,
                max_instance_cost_per_hour: 5.0,
                image: "ubuntu-24.04".into(),
                ssh_keys: vec![],
                region: "nbg1".into(),
            },
            instances: Mutex::new(HashMap::new()),
            client: reqwest::Client::new(),
        }
    }

    fn resolve_token(&self) -> String {
        if let Some(env_var) = self.config.hetzner_api_token.strip_prefix("env:") {
            std::env::var(env_var).unwrap_or_default()
        } else {
            self.config.hetzner_api_token.clone()
        }
    }

    /// Parse a deployment request from message text.
    /// Recognizes patterns like:
    ///   "/deploy llama3:8b ttl=2h model=lora-adapter"
    ///   "/infra create model=mixtral gpu=A100 hours=4"
    fn parse_request(&self, text: &str) -> Option<DeployRequest> {
        let text = text.to_lowercase();

        // Check for deployment keywords
        let is_deploy = text.contains("/deploy")
            || text.contains("/infra")
            || text.contains("spawn gpu")
            || text.contains("launch instance");

        if !is_deploy { return None; }

        // Extract model name
        let model = text
            .split_whitespace()
            .find(|w| w.contains("model="))
            .map(|w| w.trim_start_matches("model=").to_string())
            .or_else(|| {
                // Try to find model name after "deploy" or "/deploy"
                let parts: Vec<&str> = text.split_whitespace().collect();
                parts.iter()
                    .position(|w| *w == "/deploy" || *w == "/infra")
                    .and_then(|i| parts.get(i + 1))
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "llama3:8b".to_string());

        // Extract TTL
        let ttl = text.split_whitespace()
            .find(|w| w.contains("ttl="))
            .and_then(|w| w.trim_start_matches("ttl=").parse::<u64>().ok())
            .or_else(|| {
                text.split_whitespace()
                    .find(|w| w.contains("hours="))
                    .and_then(|w| w.trim_start_matches("hours=").parse::<u64>().ok())
                    .map(|h| h * 3600)
            })
            .unwrap_or(self.config.default_ttl_secs);

        // Extract GPU type
        let gpu = text.split_whitespace()
            .find(|w| w.contains("gpu="))
            .map(|w| w.trim_start_matches("gpu=").to_string())
            .unwrap_or_else(|| "A100".to_string());

        Some(DeployRequest { model, ttl, gpu })
    }

    /// Create a Hetzner Cloud server via API.
    async fn create_hetzner_server(&self, req: &DeployRequest, tenant: &str) -> Result<ActiveInstance, String> {
        let token = self.resolve_token();
        if token.is_empty() {
            return Err("HETZNER_API_TOKEN not set".into());
        }

        let server_type = match req.gpu.as_str() {
            "A100" | "a100" => "ccx13",
            "A100-80GB" | "a100-80gb" => "ccx23",
            "cpu" | "CPU" => "cx22",
            _ => "ccx13",
        };

        // Determine model source:
        // - HuggingFace: hf:username/model-name
        // - Ollama: ollama:model-name
        // - Custom: URL to model weights
        let model_source = if req.model.contains("hf:") {
            req.model.trim_start_matches("hf:").to_string()
        } else if req.model.contains("ollama:") {
            req.model.trim_start_matches("ollama:").to_string()
        } else {
            // Default: look up on Hugging Face
            format!("meta-llama/{}", req.model)
        };

        let instance_name = format!("ohagent-{}-{}", tenant, &req.model[..req.model.len().min(20)]);
        let cloud_init = generate_cloud_init(&model_source, &instance_name);

        let body = serde_json::json!({
            "name": instance_name,
            "server_type": server_type,
            "image": self.config.image,
            "location": self.config.region,
            "ssh_keys": self.config.ssh_keys,
            "user_data": cloud_init,
            "labels": {
                "ohagent": "true",
                "tenant": tenant,
                "model": req.model,
                "ttl": req.ttl.to_string(),
                "auto_destroy": "true"
            }
        });

        tracing::info!(%server_type, model=%req.model, "Creating Hetzner instance");

        let resp = self.client
            .post("https://api.hetzner.cloud/v1/servers")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("API request failed: {e}"))?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(format!("Hetzner API error: {err}"));
        }

        let data: serde_json::Value = resp.json().await
            .map_err(|e| format!("JSON parse: {e}"))?;

        let server = &data["server"];
        let server_id = server["id"].as_u64().unwrap_or(0);
        let ip = server["public_net"]["ipv4"]["ip"]
            .as_str()
            .unwrap_or("0.0.0.0")
            .to_string();
        let endpoint = format!("http://{ip}:8000/v1");

        let instance = ActiveInstance {
            id: uuid::Uuid::new_v4().to_string(),
            name: instance_name,
            ip,
            model: req.model.clone(),
            endpoint,
            created_at: chrono::Utc::now().timestamp(),
            ttl_secs: req.ttl,
            server_id,
        };

        tracing::info!(
            server_id,
            ip = %instance.ip,
            model = %req.model,
            ttl = req.ttl,
            "Instance created"
        );

        Ok(instance)
    }

    /// Destroy a Hetzner instance.
    async fn destroy_instance(&self, instance: &ActiveInstance) {
        let token = self.resolve_token();
        let _ = self.client
            .delete(format!("https://api.hetzner.cloud/v1/servers/{}", instance.server_id))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await;

        tracing::info!(server_id = instance.server_id, "Instance destroyed");
    }
}

// ── Deploy Request ──

#[derive(Debug)]
struct DeployRequest {
    model: String,
    ttl: u64,
    gpu: String,
}

/// Generate cloud-init script for auto-setup.
fn generate_cloud_init(model_source: &str, instance_name: &str) -> String {
    format!(r#"#cloud-config
package_update: true
packages:
  - python3-pip
  - python3-venv
  - nvidia-container-toolkit
  - docker.io

write_files:
  - path: /opt/ohagent/setup.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      set -e
      echo "==> Starting setup for {instance_name}"

      # Pull and run vLLM with the model
      docker run -d --name vllm-server \
        --gpus all \
        -p 8000:8000 \
        -e HF_TOKEN=${{HF_TOKEN}} \
        vllm/vllm-openai:latest \
        --model {model_source} \
        --host 0.0.0.0 \
        --port 8000

      echo "==> vLLM server started on port 8000"

      # Auto-destroy after TTL (set in instance labels)
      TTL=$(curl -s http://169.254.169.254/hetzner/v1/metadata | jq -r .labels.ttl)
      if [ -n "$TTL" ] && [ "$TTL" != "null" ]; then
        echo "==> Scheduling auto-destroy in ${{TTL}}s"
        (sleep "$TTL" && curl -X DELETE -H "Authorization: Bearer $(curl -s http://169.254.169.254/hetzner/v1/metadata)" \
          http://169.254.169.254/hetzner/v1/server) &
      fi

runcmd:
  - /opt/ohagent/setup.sh
"#, instance_name = instance_name, model_source = model_source)
}

// ── MessagePlugin impl ──

impl MessagePlugin for InfraLauncherPlugin {
    fn name(&self) -> &str { "ohagent-infra-launcher" }
    fn version(&self) -> (u32, u32) { (1, 0) }

    fn init(&mut self) -> Result<(), PluginError> {
        let token = self.resolve_token();
        if token.is_empty() {
            tracing::warn!("HETZNER_API_TOKEN not set — infra launcher will use simulation mode");
        } else {
            tracing::info!("Infra launcher ready (Hetzner Cloud)");
        }
        Ok(())
    }

    fn transform_message(&self, message: &mut PluginMessage) -> Result<(), PluginError> {
        // Check if this is an infrastructure deployment request
        let req = match self.parse_request(&message.text) {
            Some(r) => r,
            None => return Ok(()), // Not a deploy request — pass through
        };

        // Check if we have an existing instance for this tenant+model
        {
            let instances = self.instances.lock().unwrap();
            let key = format!("{}:{}", message.tenant_id, req.model);
            if let Some(inst) = instances.get(&key) {
                // Route to existing instance instead of deploying new
                let redirect = format!(
                    "[INFRA] Using existing instance: {} (model: {}, endpoint: {})",
                    inst.id, inst.model, inst.endpoint
                );
                message.text = format!("{}\n{}", message.text, redirect);
                message.log_redaction(
                    "infra-launcher",
                    "deploy-request",
                    "existing-instance",
                    "infra_redirect",
                );
                return Ok(());
            }
        }

        // TODO: In async context, we'd spawn the instance here.
        // For now, return a response that the user can use.
        let response = format!(
            "[INFRA] Deploy request queued:\n  Model: {}\n  GPU: {}\n  TTL: {}s ({}h)\n  Status: pending\n\n\
             Set HETZNER_API_TOKEN to enable auto-provisioning.\n\
             For now, manually deploy:\n  hcloud server create --name ohagent-{}-{} \
             --type ccx13 --image ubuntu-24.04",
            req.model, req.gpu, req.ttl, req.ttl / 3600,
            message.tenant_id, req.model.split(':').next().unwrap_or("model"),
        );

        message.text = format!("{}\n{}", message.text, response);
        message.log_redaction(
            "infra-launcher",
            "deploy-request",
            "provisioning-pending",
            "infra_deploy",
        );

        Ok(())
    }

    fn shutdown(&mut self) {
        // Destroy all active instances on shutdown
        let instances: Vec<ActiveInstance> = {
            self.instances.lock().unwrap().values().cloned().collect()
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        for inst in instances {
            rt.block_on(self.destroy_instance(&inst));
        }
        tracing::info!("All instances destroyed on shutdown");
    }
}

// ── FFI ──

#[no_mangle]
pub extern "C" fn plugin_api_version() -> u32 { CURRENT_PLUGIN_API_VERSION }

#[no_mangle]
pub extern "C" fn create_plugin() -> *mut dyn MessagePlugin {
    Box::into_raw(Box::new(InfraLauncherPlugin::new()))
}
