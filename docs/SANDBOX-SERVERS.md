# External Sandbox Servers — Isolated Compute

Provision external VMs on-demand for heavy workloads (Java/GraalVM compilation,
k3s clusters, testing). Complete isolation from main ohAgent infrastructure.

## Security Model (non-negotiable)

```
┌─────────────────────────────────────────────────────────────────┐
│                  ohAgent main server (51.159.106.193)            │
│                                                                  │
│  Agent says "/sandbox compile-java repo=..."                    │
│       │                                                          │
│       ▼                                                          │
│  ┌────────────────────────────┐                                  │
│  │   Sandbox Provisioner      │                                  │
│  │                            │                                  │
│  │  1. POST Hetzner API       │                                  │
│  │  2. cloud-init:            │                                  │
│  │     - ufw deny from 51.*   │ ◄── FIREWALL: blocks main server │
│  │     - install k3s/GraalVM  │                                  │
│  │     - POST result callback │                                  │
│  │  3. Wait for result        │                                  │
│  │  4. Destroy VM             │                                  │
│  └────────────────────────────┘                                  │
│       │                                                          │
│       │  cloud API (Hetzner/Scaleway)                            │
│       ▼                                                          │
│  ┌──────────────────────────────────────────────┐               │
│  │         Sandbox VM (external, isolated)       │               │
│  │                                              │               │
│  │  • ufw: DROP from 51.159.106.193            │               │
│  │  • No K8s access (no kubeconfig)             │               │
│  │  • Ephemeral: destroyed after TTL            │               │
│  │  • Runs: GraalVM, k3s, cargo, gcc, etc.     │               │
│  │  • Reports result via HTTP POST to agent     │               │
│  │    (agent polls or receives callback)        │               │
│  └──────────────────────────────────────────────┘               │
└─────────────────────────────────────────────────────────────────┘
```

### Why the sandbox VM cannot modify the main server

| Vector | Defense |
|---|---|
| **Network** | `ufw deny from 51.159.106.193` in cloud-init BEFORE any app starts |
| **DNS rebinding** | cloud-init writes `/etc/hosts` hardcoding main server IP |
| **K8s API** | No kubeconfig, no SA token, wrong network |
| **SSH** | Main server never exposes SSH to internet |
| **VPN/overlay** | No WireGuard keys, no Tailscale installed |
| **Reverse shell** | Main server doesn't listen on any port the VM can reach |
| **Cloud metadata** | VM is in different project/account |

### One-way communication

The VM CAN reach the internet (for apt, git, maven). It CANNOT reach the main server.
Agent communicates with the VM via cloud-init logs and a simple HTTP callback:

```
VM cloud-init → POST https://agent-callback.orangehat.eu/result/{job_id}
```

The callback endpoint is on the main K8s cluster but only accepts POST with a per-job
token — not a general management API.

## Cost Model

| Provider | Type | vCPU | RAM | Price/hr | Best for |
|---|---|---|---|---|---|
| **Hetzner CPX41** | Cloud VM | 8 | 16 GB | €0.022 | GraalVM, general |
| **Hetzner CPX51** | Cloud VM | 16 | 32 GB | €0.048 | Heavy compilation |
| **Scaleway DEV1-L** | Dev Instance | 4 | 8 GB | €0.016 | Light tasks |
| **Scaleway GP1-L** | General Purpose | 4 | 16 GB | €0.029 | GraalVM |
| **OVH VPS-4** | VPS | 8 | 24 GB | €0.032 | Heavy compilation |

**Hourly billing:** pay only for usage. 10-minute GraalVM compile = €0.004 (CPX41).
Monthly dedicated VPS = €15-23 only if used 24/7.

### Recommended: Hetzner CPX41

- €0.022/hr — 8 vCPU, 16 GB RAM
- Ubuntu 24.04 + cloud-init
- 20 TB traffic included
- Locations: Nuremberg, Falkenstein, Helsinki (low latency to Paris)

Hetzner CPX51 (32 GB) if GraalVM needs more RAM.

## Implementation Plan

### Phase 1: Core Sandbox Provisioner

```rust
// crates/ohagent-sandbox/src/lib.rs

pub struct SandboxConfig {
    /// Cloud provider: "hetzner", "scaleway"
    pub provider: String,
    /// API token from env
    pub api_token: String,
    /// Default TTL (seconds)
    pub default_ttl_secs: u64,
    /// Max TTL (cap)
    pub max_ttl_secs: u64,
    /// Server type (CPX41, DEV1-L, etc.)
    pub server_type: String,
    /// Image (ubuntu-24.04)
    pub image: String,
    /// Location (nbg1, par1, hel1)
    pub location: String,
}

pub struct SandboxProvisioner {
    config: SandboxConfig,
    client: reqwest::Client,
    active: DashMap<String, ActiveSandbox>,  // job_id → VM
}

struct ActiveSandbox {
    server_id: String,
    ip: String,
    created_at: Instant,
    ttl: Duration,
    job_id: String,
}

impl SandboxProvisioner {
    /// Create a new sandbox VM with cloud-init
    pub async fn create(&self, job: SandboxJob) -> Result<ActiveSandbox>;
    
    /// Poll for job completion
    pub async fn check_status(&self, job_id: &str) -> Result<SandboxStatus>;
    
    /// Destroy sandbox (or let TTL expire)
    pub async fn destroy(&self, job_id: &str) -> Result<()>;
    
    /// List active sandboxes with costs
    pub fn list_active(&self) -> Vec<SandboxInfo>;
}
```

### Phase 2: cloud-init Templates

```yaml
#cloud-config
hostname: ohagent-sandbox-{JOB_ID}

# ── Block main server BEFORE any app starts ──
runcmd:
  - ufw default deny incoming
  - ufw allow ssh
  - ufw allow 80/tcp
  - ufw allow 443/tcp
  - ufw deny from 51.159.106.193
  - ufw --force enable
  
  # Hard-code main server IP to prevent DNS rebinding
  - echo "0.0.0.0 ohagent.orangehat.eu" >> /etc/hosts
  - echo "0.0.0.0 agent.orangehat.eu" >> /etc/hosts

  # ── Install tooling ──
  - apt-get update
  - apt-get install -y openjdk-21-jdk git curl build-essential
  - |
    # Install GraalVM (for native-image)
    curl -sL https://download.oracle.com/graalvm/23/latest/graalvm-jdk-23_linux-x64_bin.tar.gz | tar xz -C /opt
    echo 'export GRAALVM_HOME=/opt/graalvm-jdk-23' >> /etc/profile.d/graalvm.sh
    echo 'export JAVA_HOME=$GRAALVM_HOME' >> /etc/profile.d/graalvm.sh
    echo 'export PATH=$GRAALVM_HOME/bin:$PATH' >> /etc/profile.d/graalvm.sh
    /opt/graalvm-jdk-23/bin/gu install native-image

  # ── Install k3s if needed ──
  - |
    if [ "{INSTALL_K3S}" = "true" ]; then
      curl -sfL https://get.k3s.io | sh -
    fi

  # ── Run the job ──
  - |
    export GRAALVM_HOME=/opt/graalvm-jdk-23
    export JAVA_HOME=$GRAALVM_HOME
    export PATH=$GRAALVM_HOME/bin:$PATH
    
    git clone {REPO_URL} /tmp/work
    cd /tmp/work
    {COMPILE_COMMAND} 2>&1 | tee /var/log/ohagent-job.log
    
    # Report result via callback
    RESULT=$(tail -1 /var/log/ohagent-job.log | head -c 5000)
    curl -X POST "https://agent.orangehat.eu/sandbox/callback/{JOB_ID}" \
      -H "Authorization: Bearer {JOB_TOKEN}" \
      -d "{\"status\":\"done\",\"result\":\"$RESULT\"}"

  # ── Self-destruct after TTL ──
  - shutdown -h +{TTL_MINUTES} "ohAgent sandbox TTL expired"
```

### Phase 3: Integration with Security Guard

The `security_guard.rs` already blocks kubectl mutations. The sandbox provisioner
is additional: it uses cloud APIs (Hetzner/Scaleway REST), not K8s APIs.

Agent can say: "/sandbox compile-java repo=https://github.com/user/project"
→ Provisioner creates VM, job runs, VM self-destructs.

## Usage Examples

```text
# Compile Java to native-image
User: /sandbox compile-java repo=https://github.com/user/app type=hetzner-cpx41

# Test k3s deployment
User: /sandbox k3s-test repo=https://github.com/user/charts type=hetzner-cpx51

# General-purpose compile box
User: /sandbox run repo=https://github.com/user/rust-project cmd="cargo build --release"
```

### Agent command format

```
/sandbox <workload> repo=<url> [type=<vm-type>] [ttl=<duration>]

workloads:
  compile-java     → GraalVM native-image
  k3s-test         → Single-node k3s
  rust-build       → cargo build
  general          → Any shell command
```

## API

```
POST /api/sandbox/create
  {"workload":"compile-java","repo":"https://...","type":"hetzner-cpx41","ttl":"30m"}
  → {"job_id":"...", "ip":"...", "estimated_cost":"€0.011"}

GET /api/sandbox/{job_id}
  → {"status":"running","ip":"...","elapsed":"5m","cost":"€0.002"}

DELETE /api/sandbox/{job_id}
  → {"status":"destroyed"}

GET /api/sandbox
  → {"active":[...], "total_cost_this_month":"€0.37"}
```
