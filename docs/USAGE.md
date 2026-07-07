# ohAgent Usage Guide — All Scenarios

Comprehensive guide to using ohAgent across all features: chat, plugins, MCP,
infrastructure, dynamic routing, desktop control, and more.

---

## Table of Contents

1. [Quick Start](#1-quick-start)
2. [Basic Chat (Telegram + API)](#2-basic-chat)
3. [PII Redaction Plugin](#3-pii-redaction-plugin)
4. [Desktop Control (MCP)](#4-desktop-control-mcp)
5. [Infrastructure Launcher](#5-infrastructure-launcher)
6. [Dynamic Provider Routing](#6-dynamic-provider-routing)
7. [Custom Model Deployment](#7-custom-model-deployment)
8. [Plugin Development](#8-plugin-development)
9. [Enterprise Deployment](#9-enterprise-deployment)
10. [Troubleshooting](#10-troubleshooting)

---

## 1. Quick Start

### Start the daemon

```bash
# Minimal — Telegram only
export DEEPSEEK_API_KEY="sk-your-key"
export TELEGRAM_BOT_TOKEN="123456:ABC-DEF"
cargo run --release -p ohagent-daemon

# Full — all features
export DEEPSEEK_API_KEY="sk-..."
export TELEGRAM_BOT_TOKEN="123:..."
export HETZNER_API_TOKEN="..."    # for GPU provisioning
export SCW_SECRET_KEY="..."       # for Scaleway serverless/GPU
export SF_API_KEY="..."           # for SiliconFlow
OHAGENT_CMC_ENABLED=1 cargo run --release -p ohagent-daemon
```

### Verify health

```bash
curl http://localhost:9090/health
# {"status":"ok","service":"ohagent","version":"0.1.0"}
```

---

## 2. Basic Chat

### Telegram Bot Commands

| Command | Description | Example |
|---|---|---|
| `/pair` | Generate pairing code | `/pair` → ABC123 |
| `/confirm <code>` | Confirm pairing | `/confirm ABC123` |
| `/new` | Start fresh conversation | `/new` |
| `/stop` | Interrupt current task | `/stop` |
| `/lang` | Toggle EN→LV→RU | `/lang` |
| `/model` | Show model preferences | `/model` |
| `/model set coding deepseek-chat` | Set model for coding | |
| `/remember <text>` | Save to memory | `/remember My API key is sk-abc` |
| `/recall <query>` | Search memories | `/recall deployment` |
| `/forget <id>` | Delete memory | `/forget abc-123-def` |
| `/skills` | List learned skills | `/skills` |
| `/skill <name>` | Skill details | `/skill deployment` |
| `/skilluse <name>` | Record skill usage | `/skilluse deployment` |

### OpenAI-Compatible API

```bash
# Non-streaming
curl -X POST http://localhost:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"Hello!"}]}'

# Streaming (SSE)
curl -X POST http://localhost:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"Write a poem"}],"stream":true}'

# WebSocket streaming
wscat -c ws://localhost:9090/v1/ws/chat
> {"type":"chat","model":"deepseek-chat","messages":[{"role":"user","content":"Hi"}]}
```

### Model Routing

ohAgent automatically routes to the best model based on task type:
- **General chat** → DeepSeek V4-Flash (€0.14/M tok)
- **Code generation** → DeepSeek Chat (€0.27/M tok)
- **Complex reasoning** → DeepSeek Reasoner (€0.55/M tok)
- **Top quality** → Anthropic Claude (€3.00/M tok)

Override per-task: `/model set coding claude-sonnet-4`

### Agent Tools in Chat

When tools are registered, the agent can execute commands and modify files:

```text
User: "Create a Python script that analyzes a CSV file"
Agent: [calls bash: ls *.csv] → [calls write: analyze.py] → [calls bash: python analyze.py]
```

Available built-in tools: `bash`, `write`, `edit`, `read`, `ls`

---

## 3. PII Redaction Plugin

### Scenario: Enterprise Data Loss Prevention

A company deploys ohAgent and wants to prevent employees from accidentally
sending API keys, customer emails, or source code to external LLM providers.

### Setup

```bash
# 1. Build the plugin
cd crates/ohagent-pii-redactor && cargo build --release

# 2. Install
mkdir -p ~/.ohagent/plugins
cp target/release/libohagent_pii_redactor.so ~/.ohagent/plugins/

# 3. Configure
cat > ~/.ohagent/plugins.toml <<EOF
[[plugins]]
file = "libohagent_pii_redactor.so"
enabled = true
config = {}
EOF

# 4. Restart daemon
```

### What it detects (15 categories)

| Category | Examples | Replacement |
|---|---|---|
| OpenAI keys | `sk-proj-abc123...` | `[REDACTED:api_key_openai]` |
| Anthropic keys | `sk-ant-api03-xxx...` | `[REDACTED:api_key_anthropic]` |
| GitHub tokens | `ghp_xxxxxxxxxxxx` | `[REDACTED:api_key_github]` |
| Slack tokens | `xoxb-123-456-abc` | `[REDACTED:api_key_slack]` |
| JWT tokens | `eyJhbGciOi...` | `[REDACTED:jwt_token]` |
| AWS keys | `AKIA...`, `ASIA...` | `[REDACTED:aws_*]` |
| Private keys | `-----BEGIN PRIVATE KEY-----` | `[REDACTED:private_key_pem]` |
| Connection strings | `postgres://user:pass@host` | `[REDACTED:conn_string]` |
| Emails | `john@example.com` | `[REDACTED:email]` |
| Phones | `+371 12345678` | `[REDACTED:phone_international]` |
| SSN | `123-45-6789` | `[REDACTED:ssn]` |
| Credit cards | `4111-1111-1111-1111` | `[REDACTED:credit_card]` |
| IBAN | `DE89 3704 0044...` | `[REDACTED:iban]` |
| IP addresses | `192.168.1.1`, `::1` | `[REDACTED:ip_v4/v6]` |
| Secrets in code | `password=`, `secret=`, `token=` | `[REDACTED:secret_assignment]` |

### Before/After Example

```
Before:
  "Fix this bug: my API key is sk-proj-abc123def456, connect to
   postgres://admin:pass123@db.internal:5432/prod"

After:
  "Fix this bug: my API key is [REDACTED:api_key_openai_proj], connect to
   [REDACTED:conn_string]"
```

The original content is NEVER stored. Only lengths are logged for audit.

### Audit Log

```bash
# View recent redactions
curl -H "X-API-Key: $OHAGENT_API_KEY" http://localhost:9090/api/plugins/audit

# Response:
{
  "total": 42,
  "entries": [
    {
      "plugin": "pii-redactor/api_key_openai_proj",
      "field": "api_key_openai_proj",
      "original_bytes": 29,
      "replacement_bytes": 32,
      "timestamp": 1783411200
    }
  ]
}
```

### Commercial License

Production builds require a license key:

```bash
# Set license (obtained from ohagent.dev/licenses)
export OHAGENT_PII_LICENSE="TENANT-<base64>"

# Dev builds skip validation automatically
```

### Multi-platform Build

```bash
./scripts/build-pii.sh 1.0.0
# Output: dist/pii-redactor/1.0.0/
#   libohagent_pii_redactor-linux-x86_64.so
#   libohagent_pii_redactor-linux-aarch64.so
#   libohagent_pii_redactor-darwin-x86_64.dylib
#   libohagent_pii_redactor-darwin-aarch64.dylib
```

---

## 4. Desktop Control (MCP)

### Scenario: Automating GUI Applications

A user wants their AI agent to interact with desktop applications — fill forms,
click buttons, read screens, type text.

### Setup

```bash
# 1. Build the desktop MCP server
cargo build --release -p ohagent-desktop-mcp

# 2. Register in MCP config
# Already done automatically: ~/.jcode/mcp.json
```

### Available Tools (10 total)

| Tool | Description | Example |
|---|---|---|
| `screenshot` | Capture screen, optional crop region | No args = full screen |
| `mouse_move` | Move cursor to (x,y) | `{"x":500,"y":300}` |
| `mouse_click` | Click button at coordinates | `{"button":"left","x":500,"y":300}` |
| `mouse_drag` | Drag from (x1,y1) to (x2,y2) | `{"x1":100,"y1":100,"x2":400,"y2":400}` |
| `keyboard_type` | Type text | `{"text":"Hello world"}` |
| `keyboard_press` | Key combination | `{"keys":["ctrl","c"]}` |
| `accessibility_tree` | AT-SPI tree dump (Linux) | `{"max_depth":3,"app_name":"firefox"}` |
| `window_list` | List open windows | No args |
| `window_focus` | Focus window by title | `{"title":"Firefox"}` |
| `get_screen_size` | Monitor info | No args |

### Scenario: Automate Browser Login

```text
User: "Log into https://admin.example.com with username admin"
Agent:
  1. [calls screenshot] → sees login page
  2. [calls mouse_click {"x":400,"y":300}] → click username field
  3. [calls keyboard_type {"text":"admin"}] → type username
  4. [calls keyboard_press {"keys":["tab"]}] → tab to password
  5. [calls keyboard_type {"text":"..."}] → type password
  6. [calls keyboard_press {"keys":["return"]}] → submit
  7. [calls screenshot] → verify logged in
```

### Installation Requirements

| OS | Screenshot | Mouse/Keyboard | Accessibility |
|---|---|---|---|
| **Linux** | `imagemagick` or `maim` | `enigo` (x11rb) | `python3-pyatspi` |
| **macOS** | `screencapture` (built-in) | `enigo` | Not available |
| **Windows** | PowerShell | `enigo` | Not available |

---

## 5. Infrastructure Launcher

### Scenario: Deploy a Custom Model on GPU

You have a LoRA adapter fine-tuned on your company's documentation. You want
to deploy it on a GPU for a 2-hour batch processing job, then tear it down.

### Commands

```text
# Scaleway Serverless (instant, no GPU, per-token pricing)
/deploy scaleway:mistral-small-3.2

# Scaleway Dedicated GPU — cheap L4
/deploy custom-lora gpu=L4 provider=scaleway ttl=4h

# Scaleway Dedicated GPU — H100 for large models
/deploy llama-70b-lora gpu=H100 provider=scaleway ttl=2h

# Hetzner — cheapest raw GPU
/deploy mixtral-lora gpu=A100 provider=hetzner ttl=2h

# SiliconFlow — ultra-cheap serverless
/deploy sf:Qwen3-Coder-30B-A3B

# Shortcuts
/deploy sf:Qwen3-8B              # SiliconFlow cheapest
/deploy scw:mistral-small         # Scaleway serverless
/deploy hz:llama3 gpu=A100       # Hetzner
```

### Cost Comparison: 2-hour LoRA Session

| Provider | GPU | Cost | Model Size |
|---|---|---|---|
| Scaleway | L4 24GB | **€1.86** | Up to 8B params |
| Hetzner | A100-40 | **€3.70** | Up to 14B params |
| Scaleway | H100 80GB | **€6.80** | Up to 72B params |

### Auto-Destroy

All instances have a TTL. After the configured time, the instance destroys
itself via cloud-init auto-shutdown scripts.

---

## 6. Dynamic Provider Routing

### Scenario: Automatic Provider Selection

ohAgent can automatically select the best provider+model for each task based
on price, speed, and quality requirements.

### Routing Tiers

```bash
# Budget — pick cheapest that works (SiliconFlow/DeepSeek)
cargo run -p ohagent-provider-metrics -- route \
  --capabilities chat --prompt-tokens 1000 --output-tokens 2000 --tier budget

# Balanced — best price/quality ratio (DeepSeek/Scaleway)
cargo run -p ohagent-provider-metrics -- route \
  --capabilities chat,code --prompt-tokens 3000 --output-tokens 8000 --tier balanced

# Performance — fastest possible (Groq/SiliconFlow Qwen3-8B)
cargo run -p ohagent-provider-metrics -- route \
  --capabilities chat --tier performance

# Quality — best output (Anthropic/OpenAI)
cargo run -p ohagent-provider-metrics -- route \
  --capabilities chat,code --tier quality

# EU-only — GDPR compliance
cargo run -p ohagent-provider-metrics -- route \
  --capabilities chat --tier balanced --prefer-eu true
```

### Example Routing Decision

```json
{
  "provider": "siliconflow",
  "model_id": "Qwen/Qwen3.5-9B",
  "estimated_cost_eur": 0.00037,
  "estimated_latency_ms": 900,
  "tokens_per_second": 100.0,
  "reason": "budget routing: 0.70 x price + 0.20 x speed + 0.10 x quality = 0.847",
  "alternatives": [
    {
      "provider": "deepseek",
      "model_id": "deepseek-v4-flash",
      "cost_eur": 0.00056,
      "latency_ms": 1000,
      "tps": 80.0
    }
  ]
}
```

### Daily Price Update

```bash
# Scrape latest prices from all providers
cargo run -p ohagent-provider-metrics -- scrape

# This populates ~/.ohagent/metrics.db with 37 models across 5 providers
```

### Speed Benchmarks

```bash
# Benchmark a specific provider+model
cargo run -p ohagent-provider-metrics -- benchmark \
  --provider deepseek --model deepseek-v4-flash \
  --api-key $DEEPSEEK_KEY --api-base https://api.deepseek.com/v1 --samples 5

# Results:
# TTF: 200ms, Total: 1000ms, TPS: 80.0, P95: 1200ms
```

### Speed Comparison (estimated)

```bash
cargo run -p ohagent-provider-metrics -- speed-compare
```

| Provider | Model | TTF ms | Total ms | tok/s | Price |
|---|---|---|---|---|---|
| Groq | Llama-3.3-70B | 100 | 500 | 250 | $$$ |
| SiliconFlow | Qwen3-8B | 150 | 800 | 120 | $ |
| OpenAI | GPT-4o-mini | 200 | 900 | 100 | $$ |
| DeepSeek | V4-Flash | 200 | 1000 | 80 | $ |
| Anthropic | Claude-Sonnet-4 | 1000 | 4000 | 25 | $$$$$ |

---

## 7. Custom Model Deployment

### Scenario: Deploy Fine-Tuned Model for Specific Domain

A legal firm fine-tuned Mistral on their contract database. They want to
deploy it for batch contract review.

### Step-by-Step

```bash
# 1. Prepare LoRA adapter (done externally, e.g. via Unsloth/Axolotl)
#    Output: lora-adapter/ folder with adapter_config.json + adapter_model.safetensors

# 2. Upload to HuggingFace (or S3)
huggingface-cli upload my-org/legal-mistral-lora ./lora-adapter

# 3. Deploy via ohAgent
/deploy my-org/legal-mistral-lora gpu=H100 provider=scaleway ttl=8h

# Response:
# [INFRA] Scaleway Dedicated GPU Plan
#   Provider: Scaleway Managed Inference (Paris)
#   GPU: scw-h100 (80GB VRAM)
#   Cost: €27.20 total (€3.40/hr × 8h)

# 4. Route batch requests to the endpoint
curl http://<instance-ip>:8000/v1/chat/completions \
  -d '{"model":"legal-mistral","messages":[{"role":"user","content":"Review contract..."}]}'
```

### SiliconFlow — No Deployment Needed

For models already hosted on SiliconFlow, just use the API:

```bash
curl https://api.siliconflow.cn/v1/chat/completions \
  -H "Authorization: Bearer $SF_API_KEY" \
  -d '{"model":"deepseek-ai/DeepSeek-V4-Flash","messages":[...]}'
```

Cost: $0.13/M input + $0.28/M output — no GPU provisioning needed.

---

## 8. Plugin Development

### Scenario: Build a Custom Moderation Plugin

```rust
// ohagent-custom-moderation/src/lib.rs
use ohagent_plugins::*;

pub struct ModerationPlugin;

impl MessagePlugin for ModerationPlugin {
    fn name(&self) -> &str { "custom-moderation" }

    fn transform_message(&self, msg: &mut PluginMessage) -> Result<(), PluginError> {
        // Block toxic content
        if msg.text.contains("forbidden phrase") {
            return Err(PluginError::fatal("Content policy violation"));
        }
        // Redact competitor names
        msg.text = msg.text.replace("COMPETITOR_NAME", "[REDACTED]");
        msg.log_redaction("moderation", "COMPETITOR_NAME", "[REDACTED]", "competitor");
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn plugin_api_version() -> u32 { CURRENT_PLUGIN_API_VERSION }

#[no_mangle]
pub extern "C" fn create_plugin() -> *mut dyn MessagePlugin {
    Box::into_raw(Box::new(ModerationPlugin))
}
```

Build and deploy:

```bash
cargo build --release
cp target/release/libcustom_moderation.so ~/.ohagent/plugins/
# Add to ~/.ohagent/plugins.toml:
# [[plugins]]
# file = "libcustom_moderation.so"
# enabled = true
```

### Plugin Ideas

| Plugin Type | Use Case |
|---|---|
| Data Loss Prevention | Redact PII/secrets before LLM (see PII plugin) |
| Content Moderation | Block toxic/harmful prompts |
| Audit Logger | Log every message to SIEM/Splunk |
| Custom Router | Route messages to specific models per channel |
| Rate Limiter | Per-tenant rate limits beyond built-in |
| Translation | Auto-translate messages |
| Context Injector | Add company policies, RAG data |

Full guide: [docs/PLUGINS.md](PLUGINS.md)

---

## 9. Enterprise Deployment

### Scenario: GDPR-Compliant AI for European Bank

Requirements:
- All data must stay in EU
- PII must be redacted before leaving the machine
- Audit trail required
- Model must be served from EU datacenter

### Architecture

```text
┌─────────────────────────────────────────────────────┐
│ Enterprise Network                                   │
│  ┌──────────┐  ┌──────────────┐  ┌───────────────┐ │
│  │Employee  │→│ ohAgent       │→│ Scaleway       │ │
│  │Telegram  │  │ Daemon        │  │ Serverless     │ │
│  │          │  │               │  │ (Paris)        │ │
│  └──────────┘  │ ┌───────────┐ │  └───────────────┘ │
│                │ │PII Plugin │ │                     │
│                │ │(redacts)  │ │  Data flow:         │
│                │ └───────────┘ │  1. Employee types  │
│                │ ┌───────────┐ │  2. PII Plugin      │
│                │ │Audit Log  │ │     redacts on-dev  │
│                │ │(SQLite)   │ │  3. Only clean text │
│                │ └───────────┘ │     goes to Scaleway│
│                └──────────────┘                     │
└─────────────────────────────────────────────────────┘
```

### Configuration

```bash
# ~/.ohagent/plugins.toml
[[plugins]]
file = "libohagent_pii_redactor.so"
enabled = true
config = {}

# Environment
export DEEPSEEK_API_KEY="sk-..."        # Fallback provider
export SCW_SECRET_KEY="..."             # Scaleway (EU)
export OHAGENT_PII_LICENSE="TENANT-..." # PII license
export OHAGENT_API_KEY="secure-key"     # Audit API auth
```

### Docker Deployment

```dockerfile
FROM ghcr.io/ohagent/ohagent:latest

# Install PII plugin
COPY libohagent_pii_redactor.so /opt/ohagent/plugins/
COPY plugins.toml /etc/ohagent/plugins.toml

# Production license
ENV OHAGENT_PII_LICENSE=${PII_LICENSE}

ENTRYPOINT ["ohagent-daemon"]
```

### Kubernetes

```bash
kubectl apply -k k8s/overlays/prod
```

Includes:
- Vault sidecar for secret injection
- HPA 1-5 replicas
- PVC 10Gi for audit logs
- Prometheus metrics

### Cost Estimate (Enterprise, 100 users)

| Item | Monthly Cost |
|---|---|
| ohAgent daemon (2 replicas) | €50 (Scaleway PRO2 instances) |
| PII Plugin license | €500 (100 seats × €5) |
| LLM API (Scaleway serverless) | €500 (100K req × 100 users) |
| Audit log storage | €10 (10GB Object Storage) |
| **Total** | **~€1,060/month** |

Per-user cost: ~€10.60/month

---

## 10. Multi-Document Vision / Receipt OCR Pipeline

### Scenario: Bookkeeper Photographs 4 Receipts

A bookkeeper takes one photo of 4 receipts on a table and wants them
extracted into structured accounting data.

### Pipeline Flow

```text
📸 Photo of 4 receipts (960×1280)
       ↓
Step 0: PreClassifier — "How many documents?"
        Scaleway Mistral-small, 0.7s, €0.00017
        → "4"
       ↓
Step 1: BBox Detection — GLM-4.6V, 6s, €0.0011
        → [{bbox:[147,0,764,215], hint:"Kurs"}, ...]
       ↓
Step 2: PIL Crop + Enhance (local)
        → 4 individual receipts at full resolution
       ↓
Step 3: OCR each receipt — Scaleway Mistral-small
        → 4 JSONs with raw_text_dump for verification
```

### Running the Pipeline

```bash
# Set keys
export ZAI_API_KEY="..."       # for GLM-4.6V bbox detection
export SCW_SECRET_KEY="..."    # for pre-classifier + OCR
export SCW_PROJECT_ID="..."

# Run
python3 scripts/receipt_bbox_pipeline.py
```

### Output Example

```json
{
  "store_name": "SIA Tirdzniecibas nams \"Kurs\"",
  "address": "Lubanas iela 103, Riga",
  "reg_nr": "4000399995",
  "vat_nr": "LV40003999995",
  "date": "20.06.2026", "time": "20:11",
  "items": [
    {"name": "Preces attiecinata (K) (10%)", "quantity": 1, "total_price": 1.36},
    {"name": "Preces attiecinata (K) (13.6%)", "quantity": 1, "total_price": 1.65}
  ],
  "subtotal": 2.01, "vat_amount": 0.24, "total": 2.25,
  "payment_method": "Cash", "payment_amount": 2.00, "change": 0.25,
  "raw_text_dump": "SIA Tirdzniecibas nams \"Kurs\" Lubanas iela 103..."
}
```

### Cost Breakdown

| Step | Cost | Time |
|---|---|---|
| Count documents | €0.00017 | 0.7s |
| Detect bboxes | €0.0011 | 6s |
| Crop | €0 | 0s |
| OCR ×4 receipts | €0.0024 | 41s |
| **Total — 4 receipts** | **€0.0034** | **~47s** |
| **Per receipt** | **€0.00085** | **~12s** |

### What NOT to Do

**Never ask any model to OCR multiple small documents in one go.**
All 5 tested models hallucinated completely (100% failure rate) when given
a photo with 4 small receipts and a structured JSON schema.

✅ **Correct approach**: count → bbox detect → crop → OCR individually
❌ **Wrong approach**: one JSON extraction call for all documents

### Viewing Results

```bash
# Full pipeline results with raw_text_dump for each receipt
cat scripts/ocr_results.json | jq '.ocr_step[] | {file, store: .data.store_name, total: .data.total}'

# Cropped receipt images
ls /tmp/receipts_cropped/
```

---

## 11. Troubleshooting

### Plugin not loading

```bash
# Check plugin directory
ls -la ~/.ohagent/plugins/

# Check config
cat ~/.ohagent/plugins.toml

# Run with debug logging
RUST_LOG=debug cargo run -p ohagent-daemon 2>&1 | grep -i plugin
```

### MCP tools not appearing

```bash
# Check MCP config
cat ~/.jcode/mcp.json

# Verify server starts manually
/path/to/mcp-server  # Should wait for stdin JSON-RPC

# Test: echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | /path/to/mcp-server
```

### Speed benchmark fails

```bash
# Test API key
curl -H "Authorization: Bearer $DEEPSEEK_API_KEY" https://api.deepseek.com/v1/models

# Run with single sample for debugging
cargo run -p ohagent-provider-metrics -- benchmark \
  --provider deepseek --model deepseek-v4-flash \
  --api-key $KEY --api-base $BASE --samples 1
```

### Route returns unexpected provider

```bash
# Check price database
sqlite3 ~/.ohagent/metrics.db "SELECT provider, model_id, input_price_per_mtok FROM prices ORDER BY provider"

# Re-scrape
cargo run -p ohagent-provider-metrics -- scrape
```

### Infrastructure deploy stuck

```bash
# Check provider API token
echo $HETZNER_API_TOKEN | cut -c1-10
echo $SCW_SECRET_KEY | cut -c1-10

# Verify provider API
curl -H "Authorization: Bearer $HETZNER_API_TOKEN" https://api.hetzner.cloud/v1/servers

# Run in simulation mode (no tokens set — shows plan without deploying)
```
