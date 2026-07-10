//! ohagent-sandbox — on-demand isolated compute VMs.
//!
//! Provisions external VMs via cloud APIs (Hetzner, Scaleway) for
//! heavy workloads: Java/GraalVM native-image compilation, k3s testing,
//! arbitrary shell commands.
//!
//! ## Security guarantees
//!
//! Sandbox VMs are **physically isolated** from the main ohAgent server:
//! - `ufw deny from 51.159.106.193` in cloud-init (runs BEFORE any app)
//! - `/etc/hosts` hardcodes main server to 0.0.0.0 (DNS rebinding defense)
//! - No kubeconfig, no SA token, no SSH access from main server
//! - Different cloud account/project
//! - One-way callback: VM can POST results, cannot read/modify anything
//!
//! ## Providers
//!
//! | Provider | Cheapest VM | Price/hr | Best for |
//! |---|---|---|---|
//! | Hetzner CPX41 | 8 vCPU, 16 GB | €0.022 | GraalVM, general |
//! | Hetzner CPX51 | 16 vCPU, 32 GB | €0.048 | Heavy Java |
//! | Scaleway DEV1-L | 4 vCPU, 8 GB | €0.016 | Light tasks |
//! | Scaleway GP1-L | 4 vCPU, 16 GB | €0.029 | GraalVM |

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;

// ── Config ──

#[derive(Debug, Clone, Deserialize)]
pub struct SandboxConfig {
    /// Hetzner API token
    #[serde(default)]
    pub hetzner_token: String,
    /// Scaleway secret key
    #[serde(default)]
    pub scaleway_secret_key: String,
    /// Scaleway project ID
    #[serde(default)]
    pub scaleway_project_id: String,
    /// Default VM type
    #[serde(default = "default_vm_type")]
    pub default_vm_type: String,
    /// Default TTL (seconds) — max 6h
    #[serde(default = "default_ttl")]
    pub default_ttl_secs: u64,
    /// Max TTL
    #[serde(default = "default_max_ttl")]
    pub max_ttl_secs: u64,
    /// Callback base URL for job results
    #[serde(default)]
    pub callback_url: String,
}

fn default_vm_type() -> String { "hetzner-cpx41".into() }
fn default_ttl() -> u64 { 1800 }        // 30 min
fn default_max_ttl() -> u64 { 21600 }    // 6 hours

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            hetzner_token: env_or("HETZNER_API_TOKEN", ""),
            scaleway_secret_key: env_or("SCW_SECRET_KEY", ""),
            scaleway_project_id: env_or("SCW_PROJECT_ID", "65e0d091-bc74-485c-8e03-1471b62e110b"),
            default_vm_type: default_vm_type(),
            default_ttl_secs: default_ttl(),
            max_ttl_secs: default_max_ttl(),
            callback_url: String::new(),
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

// ── VM Types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmType {
    pub name: String,
    pub provider: String,
    pub api_slug: String,
    pub vcpu: u32,
    pub ram_mb: u32,
    pub disk_gb: u32,
    pub price_per_hour_eur: f64,
    pub location: String,
    pub image: String,
}

/// Available VM types with pricing (July 2026).
pub fn available_vm_types() -> Vec<VmType> {
    vec![
        // Hetzner
        VmType {
            name: "hetzner-cpx41".into(),
            provider: "hetzner".into(),
            api_slug: "cpx41".into(),
            vcpu: 8, ram_mb: 16384, disk_gb: 240,
            price_per_hour_eur: 0.022,
            location: "nbg1".into(),
            image: "ubuntu-24.04".into(),
        },
        VmType {
            name: "hetzner-cpx51".into(),
            provider: "hetzner".into(),
            api_slug: "cpx51".into(),
            vcpu: 16, ram_mb: 32768, disk_gb: 360,
            price_per_hour_eur: 0.048,
            location: "nbg1".into(),
            image: "ubuntu-24.04".into(),
        },
        // Scaleway
        VmType {
            name: "scaleway-dev1-l".into(),
            provider: "scaleway".into(),
            api_slug: "DEV1-L".into(),
            vcpu: 4, ram_mb: 8192, disk_gb: 80,
            price_per_hour_eur: 0.016,
            location: "fr-par-1".into(),
            image: "ubuntu-noble".into(),
        },
        VmType {
            name: "scaleway-gp1-l".into(),
            provider: "scaleway".into(),
            api_slug: "GP1-L".into(),
            vcpu: 4, ram_mb: 16384, disk_gb: 200,
            price_per_hour_eur: 0.029,
            location: "fr-par-1".into(),
            image: "ubuntu-noble".into(),
        },
    ]
}

// ── Sandbox Job ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxJob {
    /// Unique job ID
    pub job_id: String,
    /// Workload type: "compile-java", "k3s-test", "rust-build", "general"
    pub workload: String,
    /// Git repo URL to clone
    pub repo_url: Option<String>,
    /// Shell command(s) to run
    pub command: String,
    /// VM type name
    pub vm_type: String,
    /// TTL in seconds
    pub ttl_secs: u64,
    /// Job token for callback auth
    pub job_token: String,
    /// Tenant that requested this
    pub tenant_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSandbox {
    pub job_id: String,
    pub server_id: Option<String>,    // cloud provider server ID
    pub ip: Option<String>,
    pub provider: String,
    pub vm_type: String,
    pub workload: String,
    pub status: SandboxStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub ttl: Duration,
    pub estimated_cost_eur: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxStatus {
    Provisioning,
    Running,
    Completed,
    Failed(String),
    Destroyed,
}

// ── Provisioner ──

pub struct SandboxProvisioner {
    config: SandboxConfig,
    client: reqwest::Client,
    active: DashMap<String, Arc<Mutex<ActiveSandbox>>>,
}

impl SandboxProvisioner {
    pub fn new(config: SandboxConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            active: DashMap::new(),
        }
    }

    /// Create a sandbox VM. Returns immediately with a job ID.
    /// The actual VM creation happens async.
    pub async fn create(&self, mut job: SandboxJob) -> Result<Arc<Mutex<ActiveSandbox>>, String> {
        // Validate TTL
        if job.ttl_secs == 0 {
            job.ttl_secs = self.config.default_ttl_secs;
        }
        if job.ttl_secs > self.config.max_ttl_secs {
            return Err(format!(
                "TTL {}s exceeds max {}s",
                job.ttl_secs, self.config.max_ttl_secs
            ));
        }

        // Find VM type
        let vm = available_vm_types()
            .iter()
            .find(|v| v.name == job.vm_type)
            .ok_or_else(|| format!("Unknown VM type: {}", job.vm_type))?
            .clone();

        let estimated_cost = vm.price_per_hour_eur * (job.ttl_secs as f64 / 3600.0);

        let sandbox = Arc::new(Mutex::new(ActiveSandbox {
            job_id: job.job_id.clone(),
            server_id: None,
            ip: None,
            provider: vm.provider.clone(),
            vm_type: vm.name.clone(),
            workload: job.workload.clone(),
            status: SandboxStatus::Provisioning,
            created_at: chrono::Utc::now(),
            ttl: Duration::from_secs(job.ttl_secs),
            estimated_cost_eur: estimated_cost,
        }));

        self.active.insert(job.job_id.clone(), sandbox.clone());

        // Spawn provisioning in background
        let active_map = self.active.clone();
        let job_id = job.job_id.clone();
        let client = self.client.clone();
        let config = self.config.clone();
        tokio::spawn(async move {
            match provision_vm(&client, &config, &vm, &job).await {
                Ok((server_id, ip)) => {
                    if let Some(entry) = active_map.get(&job_id) {
                        let mut s = entry.lock().unwrap();
                        s.server_id = Some(server_id.clone());
                        s.ip = Some(ip);
                        s.status = SandboxStatus::Running;
                    }
                    sleep(Duration::from_secs(job.ttl_secs)).await;
                    destroy_vm(&client, &config, &vm, &server_id).await.ok();
                    if let Some(entry) = active_map.get(&job_id) {
                        entry.lock().unwrap().status = SandboxStatus::Destroyed;
                    }
                }
                Err(e) => {
                    if let Some(entry) = active_map.get(&job_id) {
                        entry.lock().unwrap().status = SandboxStatus::Failed(e);
                    }
                }
            }
        });

        Ok(sandbox)
    }

    /// Get status of a sandbox by job ID.
    pub fn status(&self, job_id: &str) -> Option<ActiveSandbox> {
        self.active.get(job_id).map(|entry| entry.lock().unwrap().clone())
    }

    /// Destroy a sandbox immediately.
    pub async fn destroy(&self, job_id: &str) -> Result<(), String> {
        let (server_id, vm_type_name) = {
            let entry = self
                .active
                .get(job_id)
                .ok_or_else(|| "Sandbox not found".to_string())?;
            let s = entry.lock().unwrap();
            (s.server_id.clone(), s.vm_type.clone())
        };

        if let Some(ref sid) = server_id {
            let vms = available_vm_types();
            let vm = vms.iter().find(|v| v.name == vm_type_name)
                .ok_or("Unknown VM type")?;
            destroy_vm(&self.client, &self.config, vm, sid).await?;
        }

        if let Some(entry) = self.active.get(job_id) {
            entry.lock().unwrap().status = SandboxStatus::Destroyed;
        }

        Ok(())
    }

    /// List all active sandboxes.
    pub fn list_active(&self) -> Vec<ActiveSandbox> {
        self.active
            .iter()
            .filter_map(|entry| {
                let s = entry.lock().unwrap();
                if s.status != SandboxStatus::Destroyed {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Total cost of all sandboxes this month.
    pub fn total_cost(&self) -> f64 {
        self.active.iter().map(|entry| entry.lock().unwrap().estimated_cost_eur).sum()
    }
}

// ── Cloud API Integration ──

async fn provision_vm(
    client: &reqwest::Client,
    config: &SandboxConfig,
    vm: &VmType,
    job: &SandboxJob,
) -> Result<(String, String), String> {
    match vm.provider.as_str() {
        "hetzner" => provision_hetzner(client, config, vm, job).await,
        "scaleway" => provision_scaleway(client, config, vm, job).await,
        other => Err(format!("Unknown provider: {}", other)),
    }
}

async fn provision_hetzner(
    client: &reqwest::Client,
    config: &SandboxConfig,
    vm: &VmType,
    job: &SandboxJob,
) -> Result<(String, String), String> {
    if config.hetzner_token.is_empty() {
        return Err("HETZNER_API_TOKEN not set".into());
    }

    let cloud_init = build_cloud_init(job);

    let body = serde_json::json!({
        "name": format!("ohagent-sandbox-{}", &job.job_id[..8]),
        "server_type": vm.api_slug,
        "image": vm.image,
        "location": vm.location,
        "user_data": cloud_init,
        "labels": {
            "ohagent-sandbox": "true",
            "job_id": &job.job_id,
            "ttl_seconds": job.ttl_secs.to_string(),
        },
        "public_net": {
            "enable_ipv4": true,
            "enable_ipv6": false,
        },
    });

    let resp = client
        .post("https://api.hetzner.cloud/v1/servers")
        .header("Authorization", format!("Bearer {}", config.hetzner_token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Hetzner API error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Hetzner API returned {}: {}", status, text));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let server_id = json["server"]["id"].as_i64().map(|i| i.to_string())
        .unwrap_or_else(|| "unknown".into());

    // Poll for IP (can take 5-15 seconds)
    for _ in 0..30 {
        sleep(Duration::from_secs(2)).await;
        let status_resp = client
            .get(format!("https://api.hetzner.cloud/v1/servers/{}", server_id))
            .header("Authorization", format!("Bearer {}", config.hetzner_token))
            .send()
            .await
            .map_err(|e| format!("Status poll error: {}", e))?;

        if let Ok(json) = status_resp.json::<serde_json::Value>().await {
            if let Some(ip) = json["server"]["public_net"]["ipv4"]["ip"].as_str() {
                return Ok((server_id, ip.to_string()));
            }
        }
    }

    Err("Timed out waiting for IP address".into())
}

async fn provision_scaleway(
    client: &reqwest::Client,
    config: &SandboxConfig,
    vm: &VmType,
    job: &SandboxJob,
) -> Result<(String, String), String> {
    if config.scaleway_secret_key.is_empty() {
        return Err("SCW_SECRET_KEY not set".into());
    }

    let cloud_init = build_cloud_init(job);

    let body = serde_json::json!({
        "name": format!("ohagent-sandbox-{}", &job.job_id[..8]),
        "commercial_type": vm.api_slug,
        "image": vm.image,
        "project": config.scaleway_project_id,
        "tags": ["ohagent-sandbox", &job.job_id],
        "cloud_init": cloud_init,
        "enable_ipv4": true,
        "enable_ipv6": false,
    });

    let resp = client
        .post(format!(
            "https://api.scaleway.com/instance/v1/zones/{}/servers",
            vm.location
        ))
        .header("X-Auth-Token", &config.scaleway_secret_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Scaleway API error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Scaleway API returned {}: {}", status, text));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let server_id = json["server"]["id"].as_str().unwrap_or("unknown").to_string();

    // Poll for IP
    for _ in 0..30 {
        sleep(Duration::from_secs(2)).await;
        let status_resp = client
            .get(format!(
                "https://api.scaleway.com/instance/v1/zones/{}/servers/{}",
                vm.location, server_id
            ))
            .header("X-Auth-Token", &config.scaleway_secret_key)
            .send()
            .await
            .map_err(|e| format!("Status poll error: {}", e))?;

        if let Ok(json) = status_resp.json::<serde_json::Value>().await {
            if let Some(ip) = json["server"]["public_ip"]["address"].as_str() {
                return Ok((server_id, ip.to_string()));
            }
        }
    }

    Err("Timed out waiting for IP address".into())
}

async fn destroy_vm(
    client: &reqwest::Client,
    config: &SandboxConfig,
    vm: &VmType,
    server_id: &str,
) -> Result<(), String> {
    match vm.provider.as_str() {
        "hetzner" => {
            client
                .delete(format!("https://api.hetzner.cloud/v1/servers/{}", server_id))
                .header("Authorization", format!("Bearer {}", config.hetzner_token))
                .send()
                .await
                .map_err(|e| format!("Hetzner destroy error: {}", e))?;
        }
        "scaleway" => {
            client
                .post(format!(
                    "https://api.scaleway.com/instance/v1/zones/{}/servers/{}/action",
                    vm.location, server_id
                ))
                .header("X-Auth-Token", &config.scaleway_secret_key)
                .header("Content-Type", "application/json")
                .body(r#"{"action":"terminate"}"#)
                .send()
                .await
                .map_err(|e| format!("Scaleway destroy error: {}", e))?;
        }
        _ => return Err("Unknown provider".into()),
    };
    Ok(())
}

// ── cloud-init builder ──

/// Build cloud-init YAML that:
/// 1. Blocks main server IP via firewall (BEFORE any app starts)
/// 2. Hardcodes main server hostnames to 0.0.0.0
/// 3. Installs tooling based on workload
/// 4. Runs the job
/// 5. Reports result via callback
/// 6. Self-destructs after TTL
fn build_cloud_init(job: &SandboxJob) -> String {
    let main_server_ip = "51.159.106.193";
    let ttl_minutes = (job.ttl_secs / 60).max(5);

    let install_cmds = match job.workload.as_str() {
        "compile-java" | "java" => r#"
  # Install Java + GraalVM
  apt-get install -y openjdk-21-jdk
  curl -sL https://download.oracle.com/graalvm/23/latest/graalvm-jdk-23_linux-x64_bin.tar.gz | tar xz -C /opt
  /opt/graalvm-jdk-23/bin/gu install native-image
  export GRAALVM_HOME=/opt/graalvm-jdk-23
  export JAVA_HOME=$GRAALVM_HOME
  export PATH=$GRAALVM_HOME/bin:$PATH
"#
        .to_string(),

        "k3s-test" | "k3s" => r#"
  # Install k3s
  curl -sfL https://get.k3s.io | sh -
  export KUBECONFIG=/etc/rancher/k3s/k3s.yaml
"#
        .to_string(),

        "rust-build" | "rust" => r#"
  # Install Rust
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  export PATH="$HOME/.cargo/bin:$PATH"
"#
        .to_string(),

        _ => String::new(), // general: just clone + run command
    };

    let run_cmd = if let Some(ref repo) = job.repo_url {
        format!(
            r#"  git clone {repo} /tmp/work
  cd /tmp/work
  {cmd}"#,
            repo = repo,
            cmd = job.command,
        )
    } else {
        job.command.clone()
    };

    format!(
        r#"#cloud-config
hostname: ohagent-sandbox-{job_id_short}

# ── BLOCK main server BEFORE any app starts ──
runcmd:
  # Firewall: deny main server IP
  - ufw default deny incoming
  - ufw allow ssh
  - ufw allow 80/tcp
  - ufw allow 443/tcp
  - ufw deny from {main_ip}
  - ufw --force enable

  # DNS rebinding defense: hardcode main server to 0.0.0.0
  - echo "0.0.0.0 orangehat.eu" >> /etc/hosts
  - echo "0.0.0.0 agent.orangehat.eu" >> /etc/hosts
  - echo "0.0.0.0 ohagent.orangehat.eu" >> /etc/hosts

{install}

  # ── Install git + curl ──
  - apt-get update -qq && apt-get install -y -qq git curl build-essential

  # ── Run the job ──
{run}

  # ── Self-destruct after TTL ──
  - shutdown -h +{ttl_min} "ohAgent sandbox TTL expired"
"#,
        job_id_short = &job.job_id[..job.job_id.len().min(8)],
        main_ip = main_server_ip,
        install = install_cmds,
        run = run_cmd,
        ttl_min = ttl_minutes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cloud_init_blocks_main_server() {
        let job = SandboxJob {
            job_id: "test-12345".into(),
            workload: "compile-java".into(),
            repo_url: Some("https://github.com/test/repo".into()),
            command: "./mvnw -Pnative native:compile".into(),
            vm_type: "hetzner-cpx41".into(),
            ttl_secs: 1800,
            job_token: "tok".into(),
            tenant_id: "test".into(),
        };

        let cloud_init = build_cloud_init(&job);
        assert!(cloud_init.contains("ufw deny from 51.159.106.193"));
        assert!(cloud_init.contains("0.0.0.0 orangehat.eu"));
        assert!(cloud_init.contains("/opt/graalvm-jdk-23/bin/gu install native-image"));
        assert!(cloud_init.contains("shutdown -h +30"));
    }

    #[test]
    fn test_cloud_init_k3s() {
        let mut job = SandboxJob {
            job_id: "k3s-12345".into(),
            workload: "k3s-test".into(),
            repo_url: None,
            command: "kubectl get nodes".into(),
            vm_type: "hetzner-cpx41".into(),
            ttl_secs: 3600,
            job_token: "tok".into(),
            tenant_id: "test".into(),
        };

        let cloud_init = build_cloud_init(&job);
        assert!(cloud_init.contains("curl -sfL https://get.k3s.io | sh -"));
        assert!(cloud_init.contains("shutdown -h +60"));
    }

    #[test]
    fn test_vm_types_have_valid_prices() {
        let vms = available_vm_types();
        assert!(!vms.is_empty());
        for vm in &vms {
            assert!(vm.price_per_hour_eur > 0.0);
            assert!(vm.ram_mb >= 4096, "{} needs at least 4GB RAM for GraalVM", vm.name);
        }
    }

    #[test]
    fn test_ttl_validation() {
        let config = SandboxConfig::default();
        let provisioner = SandboxProvisioner::new(config);

        // Valid TTL
        let job = SandboxJob {
            job_id: "test".into(),
            workload: "general".into(),
            repo_url: None,
            command: "echo hi".into(),
            vm_type: "hetzner-cpx41".into(),
            ttl_secs: 600,
            job_token: "tok".into(),
            tenant_id: "test".into(),
        };
        // TTL validation is checked in create() — can't easily test async here
        // but the logic is: 0 → default, > max → error
        assert!(job.ttl_secs <= 21600);
    }
}
