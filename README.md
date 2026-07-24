# ohAgent — 24/7 Personal AI Super-Agent

**ohAgent** is a self-hosted, always-on AI agent built on Jcode v0.56.0.
It lives in your Telegram, remembers everything, learns from your tasks,
processes receipts with sub-cent OCR, and scales to thousands of users.

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.82%2B-orange" alt="Rust 1.82+">
  <img src="https://img.shields.io/badge/jcode-v0.56.0-blue" alt="Jcode v0.56.0">
  <img src="https://img.shields.io/badge/status-active%20development-brightgreen" alt="Status">
  <img src="https://img.shields.io/badge/platform-Telegram-blue" alt="Telegram">
  <img src="https://img.shields.io/badge/OCR-Gemini%20Free-4285F4" alt="Gemini OCR">
</p>

---

## Why ohAgent?

Most AI tools are **reactive**: open, ask, close. No memory. No initiative.
ohAgent is always on, remembers everything, and gets smarter.

| | ohAgent | ChatGPT | Claude | Copilot |
|---|---|---|---|---|
| **24/7 daemon** | ✅ | ❌ | ❌ | ❌ |
| **Telegram native** | ✅ | ❌ | ❌ | ❌ |
| **Multi-provider routing** | ✅ 8 providers | ❌ | ❌ | ❌ |
| **Receipt OCR (free)** | ✅ Gemini Flash | ❌ | ❌ | ❌ |
| **People recognition** | ✅ | ❌ | ❌ | ❌ |
| **Web dashboard** | ✅ React | ❌ | ❌ | ❌ |
| **Deep memory** | ✅ semantic | ❌ | ❌ | ❌ |
| **Self-learns skills** | ✅ | ❌ | ❌ | ❌ |
| **Multi-tenant** | ✅ day one | ❌ | ❌ | ❌ |
| **i18n (EN/LV/RU)** | ✅ | ✅ | ❌ | ❌ |
| **Self-hosted** | ✅ | ❌ | ❌ | ✅ |
| **Open source** | ✅ MIT | ❌ | ❌ | ❌ |

---

## Quick Start

### Docker (recommended)

```bash
# Clone and start
git clone --recurse-submodules https://github.com/akvarel/ohAgent.git
cd ohAgent

# Set your keys
export DEEPSEEK_API_KEY="sk-..." TELEGRAM_BOT_TOKEN="123:abc"
export GOOGLE_API_KEY="..."  # optional — for receipt OCR
docker compose up -d

# Done. Bot is live at https://t.me/your_bot
# Dashboard: http://localhost:9090
```

### Bare metal

```bash
git clone --recurse-submodules https://github.com/orangehat/ohAgent.git
cd ohAgent

# Setup keys interactively
./scripts/setup-keys.sh

# Build and run
cargo build --release -p ohagent-daemon
cargo run --release -p ohagent-daemon
```

### Kubernetes

```bash
kubectl apply -k k8s/overlays/prod
# 2 replicas, HPA 2-5, Prometheus, Scaleway SSD 50Gi
```

---

## What ohAgent Can Do

| Command | What it does |
|---|---|
| `/ocr` + photo | Extract all receipts from photo → structured JSON. Free via Gemini Flash. |
| Any message | Full Jcode agent session — coding, research, file ops |
| `/model` | Show and set model preferences per task type |
| `/skills` | List self-learned skills with quality scores |
| `/remember` / `/recall` | Persistent memory across sessions |
| `/new` | Start fresh conversation (preserves memory) |
| `/lang` | Toggle EN → LV → RU |
| `/help` | Show all commands |

### REST API

```
GET  /health                  — Health check
GET  /metrics                 — Prometheus metrics
GET  /api/status              — Uptime, provider, skills/memory counts
GET  /api/keys                — List configured API keys (masked)
PUT  /api/keys                — Update keys
GET  /api/skills              — List skills (?tenant_id=&status=)
GET  /api/skills/:id          — Skill detail
POST /api/skills/:id/record   — Record usage
GET  /api/memory              — Search memories
GET  /api/memory/:id          — Memory entry
GET  /api/usage/stats         — Token usage per tenant
GET  /api/usage/recent        — Recent API calls
GET  /api/vault/health        — Vault status
GET  /api/sessions            — Active sessions
POST /api/push                — Send push notification
POST /api/remind              — Schedule reminder
POST /v1/chat/completions     — OpenAI-compatible endpoint
GET  /v1/ws/chat              — WebSocket streaming
```

---

## Architecture

```
Telegram/WhatsApp/Slack
        │
   ohAgent Daemon (Rust, 24/7)
        │
   ┌────┼────────────────────┐
   │    │                    │
   ▼    ▼                    ▼
Jcode   Memory Engine    Skills Engine
v0.56  SQLite+vector    Auto-learns
        │
   ┌────┼────────────┬──────────────┐
   │    │            │              │
DeepSeek Gemini    Scaleway    SiliconFlow
V4-Flash  Flash    EU/GDPR     200+ models
          (OCR free)
        │
   Receipt Pipeline
   ┌─────────────────┐
   │ Gemini → Arbiter│ 4s, FREE, 4/4
   └─────────────────┘

Background:
  Version checker (daily, broadcast)
  Skills cron (eval:5min, create:10min)
  Cron scheduler (SQLite jobs)
  Prometheus (:9090/metrics)
```

---

## Production (K8s)

```yaml
# 2 replicas, auto-scales to 5
resources:
  requests:  {cpu: 250m, memory: 256Mi}
  limits:    {cpu: 2000m, memory: 2Gi}

# HPA: CPU >70% or RAM >80%
# Prometheus ServiceMonitor every 30s
# Alerts: Down, Error Rate, Memory, LLM Cost Spike
# Zero-downtime: RollingUpdate maxUnavailable=0
```

No GPU. We proxy, not compute.

See [docs/INFRASTRUCTURE.md](docs/INFRASTRUCTURE.md) for full cost breakdown.

---

## Documentation

| Doc | Contents |
|---|---|
| [MANUAL.md](docs/MANUAL.md) | Full manual: install, config, commands, troubleshooting |
| [MODEL-GUIDE.md](docs/MODEL-GUIDE.md) | Model capabilities: OCR tier list, pricing, anti-patterns |
| [PRICING.md](docs/PRICING.md) | Provider costs, speed benchmarks, pipeline costs |
| [PLUGINS.md](docs/PLUGINS.md) | Plugin SDK — build your own .so plugin |
| [INFRASTRUCTURE.md](docs/INFRASTRUCTURE.md) | Server sizing, sandbox costs, aggregator DB |
| [SANDBOX.md](docs/SANDBOX.md) | Per-tenant isolated execution pods |
| [AGGREGATOR-DB.md](docs/AGGREGATOR-DB.md) | PostgreSQL schema for multi-tenant billing |
| [USAGE.md](docs/USAGE.md) | All usage scenarios: plugins, MCP, routing, deployment |

### Skills

| Skill | Description |
|---|---|
| [receipt-ocr](skills/receipt-ocr.md) | Extract receipts from photos via Gemini Flash (free) |
| [people-recognition](skills/people-recognition.md) | Detect people, demographics, nudity from photos |

---

## Providers (8 configured, 43 models tracked)

**Primary**: DeepSeek V4-Flash (€0.14/M), Gemini Flash-Lite (free OCR)
**EU/GDPR**: Scaleway Mistral-small (€0.15/M)  
**Budget**: SiliconFlow Tencent Hy3 ($0.066/M)  
**Code**: SF Qwen3-Coder ($0.07/M)  
**Vision**: GLM-4.6V, GLM-OCR, Gemini Flash  

Full comparison: [MODEL-GUIDE.md](docs/MODEL-GUIDE.md)

---

## License

MIT — [OrangeHat.AI](https://github.com/akvarel)
