# ohAgent Manual

## 1. Installation

### Prerequisites

- **Rust** 1.82+ (stable toolchain)
- **Git** for submodule checkout
- **HashiCorp Vault** or Vault Agent (for secrets)
- **DeepSeek API key** (primary provider) or **Anthropic/OpenAI API key**
- **Telegram Bot Token** (for Telegram gateway)
- Systemd (Linux) or launchd (macOS) for daemon persistence

### Step-by-step Setup

```bash
# Clone with submodules
git clone --recurse-submodules https://github.com/orangehat/ohAgent.git
cd ohAgent

# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build
cargo build --release -p ohagent-daemon
```

### Vault Setup

ohAgent reads secrets from environment variables. In production, use Vault Agent to inject them:

```bash
# Store secrets in Vault
vault kv put secret/ohagent/deepseek api_key="sk-..."
vault kv put secret/ohagent/telegram bot_token="123456:ABC-DEF..."

# Vault Agent template (vault-agent.hcl):
template {
  destination = "/tmp/ohagent.env"
  contents = <<EOF
DEEPSEEK_API_KEY={{ with secret "secret/ohagent/deepseek" }}{{ .Data.data.api_key }}{{ end }}
TELEGRAM_BOT_TOKEN={{ with secret "secret/ohagent/telegram" }}{{ .Data.data.bot_token }}{{ end }}
EOF
}

# Source the env file before starting:
source /tmp/ohagent.env
```

### Quick Start (without Vault)

```bash
export DEEPSEEK_API_KEY="sk-your-key-here"
export TELEGRAM_BOT_TOKEN="123456:ABC-DEF"
cargo run --release -p ohagent-daemon
```

---

## 2. Configuration

### CLI Flags

| Flag | Default | Description |
|---|---|---|
| `--config` | `~/.ohagent/config.toml` | Path to config file |
| `--log-level` | `info` | Log level: trace, debug, info, warn, error |
| `--health-port` | `9090` | Health check HTTP port |
| `--telegram` | `true` | Enable Telegram gateway |

### Environment Variables (via Vault → env → keys.toml)

ohAgent resolves secrets in priority order: **Vault → environment variables → `~/.ohagent/keys.toml`**.

| Variable | Required | Description |
|---|---|---|
| `DEEPSEEK_API_KEY` | Yes* | DeepSeek API key |
| `SF_API_KEY` | Recommended | SiliconFlow API key (200+ models) |
| `SCW_SECRET_KEY` | Optional | Scaleway Secret Key (Serverless + GPU) |
| `ANTHROPIC_API_KEY` | Yes* | Anthropic API key (fallback) |
| `OPENAI_API_KEY` | Yes* | OpenAI API key (fallback) |
| `GROQ_API_KEY` | Optional | Groq API key (fastest inference) |
| `GOOGLE_API_KEY` | Recommended | Google AI Studio — Gemini models (best Latvian OCR) |
| `ZAI_API_KEY` | Recommended | Z.ai API key — GLM-OCR + GLM-4.6V (bbox + vision) |
| `TELEGRAM_BOT_TOKEN` | For Telegram | Telegram Bot API token |
| `HETZNER_API_TOKEN` | For GPU infra | Hetzner Cloud API token |
| `WA_VERIFY_TOKEN` | For WhatsApp | Meta webhook verify token |
| `WA_PHONE_ID` | For WhatsApp | WhatsApp Business phone number ID |
| `WA_ACCESS_TOKEN` | For WhatsApp | Meta permanent access token |
| `SLACK_BOT_TOKEN` | For Slack | Slack Bot User OAuth Token (xoxb-...) |
| `SLACK_SIGNING_SECRET` | For Slack | Slack Events API signing secret |
| `OHAGENT_API_KEY` | Optional | API auth key for dashboard/audit endpoints |
| `OPENAI_API_BASE` | Optional | OpenAI-compatible base URL for Open WebUI |
| `OHAGENT_S3_BUCKET` | Optional | S3 bucket for message log archiving |
| `OHAGENT_CMC_ENABLED` | Optional | Set to `1` to enable CMC reasoning |

\* At least one provider key must be set.

### `~/.ohagent/keys.toml` Example

```toml
[keys]
DEEPSEEK_API_KEY = "sk-c02a719..."
SF_API_KEY = "sk-sf-abc123..."
SCW_SECRET_KEY = "scw-secret-xyz..."
ANTHROPIC_API_KEY = "sk-ant-api03-..."
OPENAI_API_KEY = "sk-proj-..."
GROQ_API_KEY = "gsk_..."
HETZNER_API_TOKEN = "..."
TELEGRAM_BOT_TOKEN = "123456:ABC-DEF"
```

### Vault Paths

| Secret | Vault Path |
|---|---|
| DEEPSEEK_API_KEY | `secret/ohagent/providers/deepseek/api-key` |
| SF_API_KEY | `secret/ohagent/providers/siliconflow/api-key` |
| SCW_SECRET_KEY | `secret/ohagent/providers/scaleway/secret-key` |
| ANTHROPIC_API_KEY | `secret/ohagent/providers/anthropic/api-key` |
| OPENAI_API_KEY | `secret/ohagent/providers/openai/api-key` |
| GROQ_API_KEY | `secret/ohagent/providers/groq/api-key` |
| ZAI_API_KEY | `secret/ohagent/providers/zai/api-key` |
| GOOGLE_API_KEY | `secret/ohagent/providers/google/api-key` |
| TELEGRAM_BOT_TOKEN | `secret/ohagent/telegram/bot-token` |

### Health Check

```bash
curl http://localhost:9090/health
# {"status":"ok","service":"ohagent","version":"0.1.0"}
```

---

## 3. Usage

### Telegram Bot Commands

Once the daemon is running and the bot is connected:

| Command | Description |
|---|---|
| `/start` | Start the bot, show greeting |
| `/pair` | Generate a pairing code |
| `/confirm <CODE>` | Confirm pairing with the code |
| `/help` | Show available commands |
| `/new` | Start a fresh conversation |
| `/lang` | Cycle language: EN → LV → RU → EN |
| `/stop` | Stop the current task |
| `/status` | Check agent status |
| `/model` | Show model router status and preferences |
| `/model set <cap> <model>` | Set model preference for a capability |
| `/model clear [cap]` | Clear model preference(s) |
| `/skills` | List learned skills with quality scores |
| `/skill <name>` | Show skill details (triggers, instructions, stats) |
| `/skilluse <name>` | Record a successful skill usage |
| `/logging` | Show message logging status |
| `/logging on` | Enable message logging |
| `/logging off` | Disable message logging |

### Pairing Flow

1. User sends `/pair` → receives a 6-character code
2. User sends `/confirm ABC123` → pairing confirmed
3. After pairing, any text message is processed by the agent

### Languages

ohAgent supports three languages from day one:
- **English** (default)
- **Latvian** (Latviešu)
- **Russian** (Русский)

Language is detected from Telegram's `language_code` and can be cycled with `/lang`.

### Multi-Tenant

Every paired user gets a unique `tenant_id` in format `telegram_{user_id}`. All agent interactions are scoped to this tenant.

---

## Deep Memory System (Phase 3)

ohAgent remembers conversations across sessions. The memory engine stores:
- **Conversation summaries** — key points and decisions from each session
- **Semantic search** — find relevant past context via meaning, not keywords
- **Proactive nudges** — the agent suggests relevant memories when they're useful

Memory is stored in SQLite at `~/.ohagent/memory.db` by default.

### Embeddings (optional)

For best results, enable embeddings:
```bash
cargo build --features ohagent-memory/embeddings
```
This loads Jcode's ONNX model (all-MiniLM-L6-v2, ~90MB download on first run).
Without embeddings, text-based keyword matching is used as a fallback.

### Memory Commands (planned for Telegram bot)

| Command | Description |
|---|---|
| `/remember <text>` | Explicitly save something to memory |
| `/recall <query>` | Search past memories |
| `/forget` | Clear memory for the current chat |

---

## Self-Learning Skills (Phase 4)

ohAgent watches what you ask and automatically learns reusable skills. This means it gets faster and more accurate at tasks you do regularly.

### How It Works

1. **Creator** — scans your conversations for patterns (tasks you've asked for 2+ times) and proposes new skills
2. **Evaluator** — tracks when skills are used successfully, computes quality scores, promotes good skills to Active
3. **Curator** — periodically cleans up: merges similar skills, prunes old unused ones

Skills are stored alongside memory in SQLite at `~/.ohagent/skills.db`.

### Manually Recording Skills

Use `/skilluse <name>` after the agent successfully completes a task using a known pattern. This boosts the skill's quality score and helps it stay active.

### Skill Statuses

| Status | Meaning |
|---|---|
| `Proposed` | Auto-created from patterns, not yet proven |
| `Active` | Proven useful through usage |
| `Disabled` | Quality dropped below threshold |
| `Retired` | Stale — unused for too long |

---

## Web Dashboard (Phase 5)

The dashboard gives you a visual overview of your agent:

```bash
cd crates/ohagent-dashboard
npm install
npm run dev        # opens http://localhost:5173
```

### Pages

- **Dashboard** — status cards (uptime, provider, skill/memory counts)
- **Skills** — skill list with status filter, quality scores, triggers. Click any skill for full details and to record usage.
- **Memory** — searchable memory entries with source type, importance, and timestamps

### REST API

The dashboard talks to the daemon's REST API on port `:9090`:

```
GET  /health                     → health check
GET  /api/status                 → full daemon status
GET  /api/skills?status=active   → list skills with filtering
GET  /api/skills/:id             → skill detail
POST /api/skills/:id/record      → record usage (body: {"success":true})
GET  /api/memory?q=deploy        → search memories
GET  /api/memory/:id             → memory entry
GET  /v1/ws/chat                → WebSocket streaming chat (JSON protocol)
POST /webhooks/telegram          → Telegram webhook (if configured)
POST /webhooks/whatsapp          → WhatsApp webhook (if configured)
POST /webhooks/slack             → Slack webhook (if configured)
```

These endpoints can be used by any external tool or integration.

---

## OpenAI-Compatible API (Phase 7)

ohAgent exposes an OpenAI-compatible chat completions API, enabling drop-in
integration with Open WebUI and other OpenAI SDK-compatible tools.

### Endpoints

```
POST  /v1/chat/completions   → streaming (SSE) and non-streaming chat
GET   /v1/models              → model list for client pickers
GET   /v1/ws/chat             → WebSocket streaming (bidirectional JSON)
```

When built-in tools are registered (`bash`, `write`, `edit`, `read`, `ls`),
chat completions route through `agent_runner` — a tool-calling loop that
lets the agent execute commands and modify files, streaming both text deltas
and tool call events as SSE.

### Usage with Open WebUI

1. In Open WebUI Admin Panel → Settings → Connections → OpenAI API
2. Set Base URL to `http://ohagent:9090/v1`
3. Set API Key to any non-empty value (not validated by ohAgent)
4. Save — ohAgent models will appear in the model picker

### Example curl

```bash
# Non-streaming
curl -X POST http://localhost:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-chat",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# Streaming (SSE)
curl -X POST http://localhost:9090/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "deepseek-chat",
    "messages": [{"role": "user", "content": "Tell me a joke"}],
    "stream": true
  }'
```

### Available Models

`GET /v1/models` returns models from all configured providers: `deepseek-chat`,
`deepseek-reasoner`, `claude-3-5-sonnet-*`, `gpt-4o`, etc.

---

## Multi-Platform Gateways (Phase 8)

### WhatsApp (Meta Cloud API)

Configure the WhatsApp adapter via environment variables:

| Variable | Required | Description |
|---|---|---|
| `WA_VERIFY_TOKEN` | Yes | Verification token from Meta app dashboard |
| `WA_PHONE_ID` | Yes | Phone number ID from Meta Business settings |
| `WA_ACCESS_TOKEN` | Yes | Permanent access token from Meta Business |

**Setup steps:**
1. Create a Meta Business app at https://developers.facebook.com
2. Add the WhatsApp product and configure a phone number
3. Set the Webhook URL to `https://your-domain/webhooks/whatsapp`
4. Set the Verify Token to match `WA_VERIFY_TOKEN`
5. Subscribe to `messages` webhook field

The webhook handles both the GET verification challenge and POST message events.

### Slack (Events API)

| Variable | Required | Description |
|---|---|---|
| `SLACK_BOT_TOKEN` | Yes | Bot User OAuth Token (xoxb-...) |
| `SLACK_SIGNING_SECRET` | Yes | Signing Secret from Slack app settings |

**Setup steps:**
1. Create a Slack App at https://api.slack.com/apps
2. Enable Events API under Event Subscriptions
3. Set Request URL to `https://your-domain/webhooks/slack`
4. Subscribe to `message.channels` and `app_mention` events
5. Add OAuth scopes: `chat:write`, `channels:history`, `app_mentions:read`
6. Install the app to your workspace

The bot responds when mentioned (`@ohagent ...`) and strips the mention prefix
before forwarding to the agent.

### Graceful Degradation

Both adapters are optional. If their env vars are not set, the daemon prints
a warning and continues with other gateways (Telegram, etc.) still operational.

---

## Swarm Orchestration (Phase 9)

ohAgent can decompose complex tasks into a DAG of subtasks and execute them
in parallel across multiple sub-agents.

### Architecture

```text
TaskGraph (plan)   →   SwarmOrchestrator   →   subprocess agents
    ↓                        ↓                        ↓
 DAG nodes         spawns workers          jcode instances
 with deps         tracks state            one per leaf task
                   merges results           returns findings
```

### Task Kinds

| Kind | Purpose |
|---|---|
| `explore` | Gather information, read docs, scan code |
| `implement` | Write code, create files, run commands |
| `verify` | Run tests, validate invariants |
| `fix` | Apply corrections based on verify results |
| `synthesize` | Combine results from dependencies |

### Example Plan (JSON)

```json
{
  "goal": "Build a Rust CLI tool",
  "max_concurrency": 3,
  "nodes": [
    {
      "id": "explore",
      "label": "Research",
      "kind": "explore",
      "prompt": "Research best practices for Rust CLI argument parsing",
      "priority": 0
    },
    {
      "id": "implement",
      "label": "Build",
      "kind": "implement",
      "prompt": "Write the Rust CLI tool using clap",
      "depends_on": ["explore"],
      "priority": 1
    },
    {
      "id": "verify",
      "label": "Test",
      "kind": "verify",
      "prompt": "Run cargo test and clippy on the new code",
      "depends_on": ["implement"],
      "priority": 1
    }
  ]
}
```

### Configuration

| Env / Config | Default | Description |
|---|---|---|
| `SWARM_MAX_CONCURRENCY` | 5 | Max parallel workers |
| `SWARM_MAX_DEPTH` | 5 | Max DAG nesting depth |
| `SWARM_TIMEOUT_SECS` | 600 | Per-worker timeout |

### API

The `swarm_run` tool is registered on the Jcode bridge and becomes available
to agents. Agents can invoke it by name with a JSON plan.

---

## Vault Integration (Phase 10)

ohAgent uses HashiCorp Vault as the primary secret store.
Resolution order: **Vault → env vars → keys.toml (on disk)**.

### Configuration

| Variable | Default | Description |
|---|---|---|
| `VAULT_ADDR` | `http://localhost:8200` | Vault server address |
| `VAULT_TOKEN` | (none) | Auth token; if set, Vault is enabled |
| `VAULT_KV_PATH` | `kv` | KV secrets engine mount path |

### Vault Paths

Secrets are resolved at the following Vault paths:

| Secret | Vault Path |
|---|---|
| DEEPSEEK_API_KEY | `secret/ohagent/providers/deepseek/api-key` |
| ANTHROPIC_API_KEY | `secret/ohagent/providers/anthropic/api-key` |
| OPENAI_API_KEY | `secret/ohagent/providers/openai/api-key` |
| TELEGRAM_BOT_TOKEN | `secret/ohagent/telegram/bot-token` |

### Auth Methods

- **Token** — set `VAULT_TOKEN` env var (simplest, used in dev)
- **Kubernetes** — auto-detects SA token at `/var/run/secrets/kubernetes.io/serviceaccount/token`
- **AppRole** — role_id + secret_id
- **Token File** — read token from a file (sidecar pattern)

### API Endpoints

```
GET  /api/vault/health   → {"available": true, "healthy": true}
GET  /api/vault/status   → {"available": true, "sealed": false, "token_set": true}
GET  /api/status         → includes "vault_available": true/false
```

### Graceful Degradation

If `VAULT_TOKEN` is not set, Vault is skipped and the daemon falls back to
environment variables, then `~/.ohagent/keys.toml`. No Vault server is required
for development — it is completely optional.

---

## Kubernetes Deployment (Phase 11)

ohAgent ships with production-ready K8s manifests using Kustomize.

### Quick Deploy

```bash
# Dev (reduced resources, local-path storage)
kubectl apply -k k8s/overlays/dev

# Production (full resources, Scaleway SSD)
kubectl apply -k k8s/overlays/prod
```

### Architecture

```text
┌─────────────────────────────────────────┐
│ Namespace: ohagent                        │
│                                           │
│  ┌──────────────────┐  ┌──────────────┐  │
│  │ ohagent-daemon   │  │ Vault Agent  │  │
│  │ (Deployment 1)   │  │ (sidecar)    │  │
│  │ port 9090        │  │ inject token │  │
│  └────────┬─────────┘  └──────────────┘  │
│           │                                │
│  ┌────────▼─────────┐                     │
│  │ Service: ohagent │  ClusterIP :9090    │
│  └────────┬─────────┘                     │
│           │                                │
│  ┌────────▼─────────┐                     │
│  │   HPA            │  1–5 replicas       │
│  └──────────────────┘                     │
│                                           │
│  ┌──────────────────┐                     │
│  │ PVC: ohagent-data│  10Gi RWO           │
│  └──────────────────┘                     │
└───────────────────────────────────────────┘
```

### Resources

| Resource | Requests | Limits | Notes |
|---|---|---|---|
| CPU | 250m | 2000m | Burstable for LLM calls |
| Memory | 256Mi | 2Gi | SQLite cache + model data |
| Storage | 10Gi | — | SQLite DBs, message logs |

### Secrets

All secrets are stored in Vault, never in K8s Secrets:
- `VAULT_TOKEN` is injected via Vault Agent sidecar
- Provider API keys resolved from `secret/ohagent/providers/*`
- Bot tokens from `secret/ohagent/telegram/bot-token`

### Health Probes

- **Liveness:** HTTP `GET /health` every 30s (15s initial delay)
- **Readiness:** HTTP `GET /health` every 10s (5s initial delay)
- **Startup:** HTTP `GET /health` every 5s, 30 retries before kill

### Autoscaling

HPA scales based on CPU (target 70%) and memory (target 80%):
- Min: 1 replica
- Max: 5 replicas
- Scale-down: 50% per 60s after 5min stabilization
- Scale-up: 100% per 30s after 1min stabilization

---

## E2E Testing (Phase 12)

ohAgent includes a Cucumber/Gherkin test suite that runs end-to-end scenarios
against a live daemon.

### Running Tests

```bash
# Build and run E2E tests (starts daemon subprocess on port 19090)
cargo test -p ohagent-daemon --test e2e

# Run only a specific feature
cargo test -p ohagent-daemon --test e2e -- --name "health"
```

### Feature Files

| File | Scenarios |
|---|---|
| `features/health.feature` | Health check, status endpoint, TCP connectivity |
| `features/openai_api.feature` | Model listing, streaming/non-streaming chat, error handling |
| `features/vault.feature` | Vault health, seal status, graceful unavailability |
| `features/skills.feature` | Skill listing, status filter, tenant queries |
| `features/memory.feature` | Memory search, empty query defaults |

### Architecture

```text
cargo test --test e2e
  → spawns ohagent-daemon (port 19090, VAULT_TOKEN="")
  → waits for /health to respond
  → runs .feature scenarios via cucumber-rs
  → each step: HTTP request → JSON assertion
  → cleanup: kill daemon + orphan processes
```

The test daemon runs with `VAULT_TOKEN=""` so Vault scenarios test
graceful degradation without a real Vault server.

---

## Docker & CI/CD

### Docker Build

```bash
# Build the image
docker build -t ohagent-daemon:latest .

# Run locally
docker run -p 9090:9090 \
  -e DEEPSEEK_API_KEY=sk-... \
  -e OHAGENT_API_KEY=my-secret-key \
  ohagent-daemon:latest
```

### Docker Compose (Local Dev)

Full dev environment with Vault:

```bash
docker compose up -d
curl http://localhost:9090/health
curl http://localhost:9090/api/status -H "X-API-Key: dev-key-change-me"
```

Services:
- **ohagent-daemon** — port 9090
- **Vault** — port 8200, root token `dev-root-token`

### CI/CD Pipeline (GitLab)

Three stages: `test` → `build` → `deploy`

| Stage | Job | Trigger |
|---|---|---|
| test | `test:unit` | MR, main, develop |
| test | `test:lint` (allow-failure) | MR, main |
| test | `test:e2e` (allow-failure) | MR, main |
| build | `build:docker` (kaniko) | main |
| deploy | `deploy:dev` (auto) | develop |
| deploy | `deploy:prod` (manual) | main |

### API Authentication

All `/api/*` endpoints require authentication:

```bash
# Via X-API-Key header
curl -H "X-API-Key: $OHAGENT_API_KEY" http://localhost:9090/api/status

# Via Authorization: Bearer
curl -H "Authorization: Bearer $OHAGENT_API_KEY" http://localhost:9090/api/status
```

If `OHAGENT_API_KEY` is not set, a random key is generated on startup
and logged. Public endpoints (`/health`, `/v1/*`, `/webhooks/*`) are
never authenticated.

### Prometheus Metrics

```bash
curl http://localhost:9090/metrics
```

Key metrics:
- `ohagent_requests_total{path,method,status}` — all HTTP requests
- `ohagent_llm_calls_total{provider,model}` — LLM API calls
- `ohagent_llm_tokens_total{provider,type}` — prompt/completion tokens
- `ohagent_sessions_active` — active Jcode sessions
- `ohagent_request_duration_seconds{path}` — request latency histogram

K8s ServiceMonitor auto-scrapes this endpoint via prometheus-operator.

### Rate Limiting

Per-tenant sliding window (default: 30 req/min/tenant):

| Env | Default | Description |
|---|---|---|
| `RATE_LIMIT_MAX_REQUESTS` | 30 | Max requests per window |
| `RATE_LIMIT_WINDOW_SECS` | 60 | Window duration in seconds |
| `RATE_LIMIT_BAN_SECS` | 300 | Ban duration after exceeding limit |

Rate-limited tenants get a 5-minute ban. Admin tenants can be exempted.

### DB Migrations

SQLite migrations are auto-applied on startup. Current migrations:

| Version | Description |
|---|---|
| 1 | skills table |
| 2 | memories table |
| 3 | usage_records table |
| 4 | message_log table |
| 5 | message_log_prefs table |
| 6 | pairing_codes table |
| 7 | Common indexes |

Migrations are tracked in the `_migrations` table. Already-applied
migrations are skipped on restart.

---

## CMC Reasoning Engine (AutoTTS-inspired)

The CMC (Confidence Momentum Controller) replaces naive single-model routing
with budget-aware, replay-optimized reasoning. Inspired by the AutoTTS paper
(LLMs Improving LLMs) — discovered controller saves 30–70% tokens at same accuracy.

### How It Works

```text
User message → N parallel branches (cheap model)
                    ↓
            CMC Controller → Stop? (EMA gate on confidence)
            |    ↓                     ↓
            |  Widen (more models)   Return winner
            |    ↓
            |  Abandon deviants
            ↓
          Continue probing
```

1. **β parameterization**: Single scalar β ∈ [0,1] controls all behavior:
   - β=0: cheap/fast (few branches, early stop)
   - β=1: thorough (many branches, high inertia)
2. **EMA gate**: Smoothes confidence over time, avoids false stops on noise
3. **Trend widening**: Spawns new branches when confidence trend is weak
4. **Conservative abandonment**: Only drops branches after persistent disagreement
5. **Budget-driven**: β auto-derives from remaining token budget

### Replay Environment

Frozen LLM traces → evaluate controllers offline (zero LLM calls):

```
Coding Agent → writes controller.py
                   ↓
             ReplayEnv.evaluate(controller)  ← 0$ cost
                   ↓
             accuracy + cost + traces
                   ↓
Coding Agent → improves controller → deploy
```

Full optimization cycle cost: ~$40 (matching AutoTTS results).

### Usage (from code)

```rust
use ohagent_reasoning::cmc::CmcConfig;
use ohagent_reasoning::budget::BudgetTracker;
use ohagent_reasoning::router::ReasoningRouter;

// Create budget with β=0.5 (balanced)
let cmc = CmcConfig::balanced();
let budget = BudgetTracker::new(/* config */);
let mut router = ReasoningRouter::new(cmc, budget);

// Initialize with initial batch
router.init(initial_results);

// Main loop
loop {
    match router.decide() {
        ReasoningAction::Stop { answer, .. } => break,
        ReasoningAction::Probe { allocations } => { /* call LLMs */ },
        ReasoningAction::Widen { count } => { /* spawn branches */ },
        ReasoningAction::Abandon { .. } => { /* already handled */ },
    }
}
```

### Replay Evaluation

```rust
use ohagent_reasoning::replay::ReplayEnv;

let mut env = ReplayEnv::new("./replay_store");
env.load()?;

// Sweep β from 0 to 1 to find optimal operating point
let optimal = env.find_optimal_beta(0.9, 20);  // ≥90% accuracy, 20 steps

// Single config evaluation
let eval = env.evaluate(&CmcConfig::balanced());
println!("{eval:?}");
// accuracy=94.50% tokens=12400 queries=100 correct=94
```

### Configuration

| Env var | Default | Description |
|---|---|---|
| `OHAGENT_CMC_BETA` | `0.5` | CMC behavior scalar |
| `OHAGENT_REASONING_MAX_TOKENS` | `50000` | Max tokens per reasoning session |
| `OHAGENT_REASONING_MAX_COST_CENTS` | `500` | Max cost per session in cents |
| `OHAGENT_REPLAY_DIR` | `./replay_store` | Replay trace storage directory |

---

## Session Persistence (Phase 13)

ohAgent remembers conversations across daemon restarts. When the daemon
starts, it restores all active sessions from SQLite.

### How It Works

1. On every message, `SessionStore` writes session metadata to `active_sessions` table
2. `/new` command clears all sessions for a tenant (both in-memory and SQLite)
3. On daemon restart, sessions are restored from the message log + summary store

No user action needed — it's fully automatic.

---

## Push Notifications (Phase 13)

ohAgent can send proactive messages without waiting for user input.

### Registration

Push registration happens automatically on pairing (`/confirm`):
- `PushService` maps `tenant_id → chat_id`
- After pairing, the agent can push reminders, completion alerts, and errors

### Sending a Push (from code)

```rust
push.send(&tenant_id, "Build completed successfully!").await?;
```

Push messages are delivered via the highest-priority active gateway
(currently Telegram Bot API).

---

## Cron Scheduler (Phase 13)

ohAgent can run tasks on a schedule: reminders, daily reports, periodic checks.

### One-shot Reminders (in-memory)

```rust
use ohagent_core::scheduler::Scheduler;

let scheduler = Scheduler::new(Some(push_service));
let job_id = scheduler.schedule_in(
    "telegram_12345",           // tenant_id
    Duration::from_secs(600),    // fire in 10 minutes
    "Проверь почту!",            // message
);
```

The scheduler fires after the delay and delivers the message via `PushService`.

### Recurring Tasks (ohagent-cron)

For persistent, recurring schedules, use the `ohagent-cron` crate:
- Cron expressions: `0 9 * * *` (daily at 9 AM)
- Intervals: `*/30 * * * *` (every 30 minutes)
- Skills attachment: each cron job can run a specific skill
- SQLite storage survives restarts

```rust
use ohagent_cron::scheduler::CronScheduler;

let scheduler = CronScheduler::new(db_path, push_service);
scheduler.add_daily("telegram_12345", 9, 0, "Пришли статистику за сегодня").await?;
```

---

## WebSocket Streaming (Phase 13)

Real-time bidirectional streaming for chat completions.

### Endpoint

```
GET /v1/ws/chat  → WebSocket upgrade (JSON protocol)
```

### Protocol

**Client → Server** (JSON):
```json
{"type": "chat", "model": "deepseek-chat", "messages": [...], "temperature": 0.7}
{"type": "cancel"}
```

**Server → Client** (JSON):
```json
{"type": "token", "content": "Hello"}
{"type": "token", "content": " world"}
{"type": "done", "usage": {"prompt": 100, "completion": 50}}
{"type": "error", "message": "Provider error"}
```

### Example (wscat)

```bash
wscat -c ws://localhost:9090/v1/ws/chat
> {"type":"chat","model":"deepseek-chat","messages":[{"role":"user","content":"Hi"}]}
< {"type":"token","content":"Hello"}
< {"type":"token","content":"!"}
< {"type":"done","usage":{"prompt":8,"completion":2}}
```

---

## File Attachments (Phase 13)

Telegram users can send photos and documents — ohAgent reads and passes them
to the agent as base64-encoded images.

### How It Works

1. Telegram adapter downloads photo/document from Telegram servers → local temp file
2. `FileAttachment` stores: `local_path`, `file_name`, `mime_type`, `size_bytes`
3. `Dispatcher.encode_attachment()` reads the file, base64-encodes it, detects MIME type
4. `SessionHandle.send_message_with_images()` passes `Vec<(mime, base64)>` to Jcode's agent loop

### Supported Formats

| Extension | MIME Type |
|---|---|
| .png | image/png |
| .jpg, .jpeg | image/jpeg |
| .gif | image/gif |
| .webp | image/webp |
| .pdf | application/pdf |
| .txt | text/plain |

### Upload Flow

```text
Telegram API → handle_message() → FileAttachment { local_path }
                                         ↓
                              encode_attachment() → (mime, base64)
                                         ↓
                              session.send_message_with_images(text, images)
                                         ↓
                              Jcode agent loop (vision model)
```

---

## Jcode Bridge Tools (Phase 13)

ohAgent registers built-in coding tools on the Jcode bridge, making the agent
capable of executing commands and modifying files.

### Built-in Tools

| Tool | Description |
|---|---|
| `bash` | Run bash commands with timeout |
| `write` | Create or overwrite files |
| `edit` | Find-and-replace text in files |
| `read` | Read file contents |
| `ls` | List directory contents |

These are registered at startup via `register_builtin_tools()`. When tools
are available, chat completions route through `agent_runner` (tool-calling loop)
instead of direct `provider.complete()`.

### Tool-Augmented Chat Flow

```text
POST /v1/chat/completions
         ↓
chat_completions_handler()
         ↓
    tool_registry.has_tools()? ── Yes → handle_streaming_with_tools()
         │                                    ↓
         │                           run_agent_turn(provider, messages, tools)
         │                                    ↓
         │                           SSE: text deltas + tool_call events
         │
         └── No  → provider.complete() (direct, no tools)
```

---

## Plugin System (Phase 14)

ohAgent supports a chainable plugin pipeline for message processing.
Plugins can redact PII, moderate content, inject context, and more.

| Document | Content |
|---|---|
| **[USAGE.md](USAGE.md)** | All usage scenarios: plugins, MCP, routing, deployment |
| **[PLUGINS.md](PLUGINS.md)** | Plugin SDK — build your own plugin |
| **[PRICING.md](PRICING.md)** | Provider cost comparison + speed benchmarks |

### Built-in Plugins

| Plugin | Purpose | License |
|---|---|---|
| **PII Redactor** | Detect + redact 15 categories of sensitive data | Proprietary |
| **Infrastructure Launcher** | Deploy GPU instances on Scaleway/Hetzner/SiliconFlow | Proprietary |

### Desktop Control

ohAgent includes an MCP server for desktop automation:

```bash
cargo build -p ohagent-desktop-mcp
# Registered automatically via ~/.jcode/mcp.json
```

10 tools: screenshot, mouse_move/click/drag, keyboard_type/press,
accessibility_tree, window_list/focus, get_screen_size.

### Dynamic Provider Routing

```bash
# Price scraper
cargo run -p ohagent-provider-metrics -- scrape

# Get optimal provider for a task
cargo run -p ohagent-provider-metrics -- route \
  --capabilities chat,code --tier budget
```

### MCP Server Pool

All MCP servers from `~/.jcode/mcp.json` are automatically available
in every ohAgent session via `SharedMcpPool`.

### CMC Reasoning (optional)

Enable multi-branch confidence-momentum controller for 30-50% token savings:

```bash
OHAGENT_CMC_ENABLED=1 cargo run -p ohagent-daemon
```

---
## Receipt / Multi-Document Processing Pipeline (Phase 15)

ohAgent can process photos with multiple documents (receipts, invoices, pages)
using a four-step pipeline: **Pre-classify → BBox → Crop → OCR → Validate**.

### Pipeline Architecture

```text
📸 Photo (multi-doc)
       ↓
   Step 0: PreClassifier — "How many documents?"
       │     Scaleway Mistral-small, 0.7s, €0.00017
       │
   ┌───┼───────────┐
   ↓   ↓           ↓
  1 doc  2+ docs  Unknown
   │     │          │
   │     │          └→ normal vision routing
   │     │
   │     └→ Step 1: BBox Detection — GLM-4.6V, 6s, €0.0011
   │              Returns [[xmin,ymin,xmax,ymax], ...] per document
   │              + rotation_degrees for deskew
   │
   └→ cheap OCR directly (no crop needed)
        │
   Step 2: PIL Crop + Enhance (local, 0s)
        │
   Step 3: OCR — Gemini 3.1 Flash-Lite (primary, 4s, FREE)
                ↓ fallback
              Gemini Flash-Latest (20s, FREE)
                ↓ last resort
              GLM-OCR per-receipt ($0.03/M)
        │
   Step 4: Mathematical Arbiter (0s)
        │     Σitems≈subtotal, VAT≈subtotal×%, sub+VAT≈total
```

### OCR Providers

| Priority | Model | Time | Cost | Notes |
|---|---|---|---|---|
| 🥇 Primary | **Gemini 3.1 Flash-Lite** | 4s | **FREE** | All 4 receipts at once. Diacritics, discounts. |
| 🥈 Fallback | Gemini Flash-Latest (2.5) | 20s | FREE | Better subtotal separation |
| 🥉 Last resort | GLM-OCR (0.9B) | 2s×4 | $0.00012 | Per-receipt, honest, misses faint text |

**All Gemini models are FREE on the free tier.** Paid tier pricing:
- 3.1 Flash-Lite: $0.25/M input, $1.50/M output
- 2.5 Flash: $0.30/M input, $2.50/M output

### Configuring the Pipeline

```bash
# Required API keys
export GOOGLE_API_KEY="..."      # Gemini (primary OCR, free)
export ZAI_API_KEY="..."         # GLM-4.6V (bbox) + GLM-OCR (fallback)
export SCW_SECRET_KEY="..."      # Scaleway (pre-classifier)
export SCW_PROJECT_ID="..."      # Scaleway project UUID

# Run the pipeline
python3 scripts/receipt_pipeline.py [path/to/photo.jpg]

| Step | Model | Time | Cost | Notes |
|---|---|---|---|---|
| Pre-classify | Scaleway Mistral-small | 0.7s | €0.00017 | 100% accurate on 4 receipts |
| BBox detect | GLM-4.6V | 6.0s | €0.0011 | Returns pixel coords + hints |
| Crop | PIL (local) | 0s | €0 | Sharpen + contrast |
| OCR ×4 | Scaleway Mistral-small | 3-29s | €0.0002-0.0015 | raw_text_dump included |
| **Total** | | **47s** | **€0.0034** | **€0.00085/receipt** |

### Configuring the Pipeline

```bash
# Required API keys
export ZAI_API_KEY="..."       # GLM-4.6V for bbox detection
export SCW_SECRET_KEY="..."    # Scaleway for pre-classifier + OCR
export SCW_PROJECT_ID="..."    # Scaleway project UUID

# Run the pipeline script
python3 scripts/receipt_bbox_pipeline.py
```

### BBox Detection with GLM-4.6V

GLM-4.6V has **native bounding-box capability** — it can return pixel coordinates
for objects in an image, including rotation angles for deskewing:

```json
{
  "receipts": [
    {
      "index": 1,
      "store_name_hint": "SIA Tirdzniecibas nams Kurs",
      "bbox": [147, 0, 764, 215],
      "rotation_degrees": 0
    }
  ],
  "total_count": 4
}
```

**Critical**: GLM-4.6V requires `"thinking": {"type": "disabled"}` in the API call.
With thinking enabled, the model consumes max_tokens budget on internal reasoning
instead of producing output.

### Important: GLM-4.6V-flash (FREE) is Unusable

The free tier (`glm-4.6v-flash`) is permanently rate-limited (HTTP 429).
Use `glm-4.6v-flashx` (¥1.00/M input, ¥5.00/M output) instead.

### PreClassifier Fallback Chain

```
1st: Scaleway Mistral-small (0.7s, €0.00017) — primary
2nd: GLM-4.6V-flashx (2.7s, €0.00014) — needs thinking=disabled
3rd: GPT-4o-mini (2.4s, €0.00352) — last resort
```

### What NOT to Do

**Do not ask any model to OCR multiple small documents at once.**
5 models tested — all hallucinated completely (100% failure rate).
The text is too small (~200px per receipt at 960px-wide photo), and
structured JSON schemas force models to invent data.

✅ Correct: count → detect → crop → OCR individually
❌ Wrong:   OCR all at once as JSON

### Pipeline Implementation

Rust crate: `ohagent-provider-metrics` provides:
- `PreClassifier` — multi-provider document counter
- `DocumentCount` — enum (Unknown, Single, Multiple(n))
- `DynamicRouter` — filters models by `multi_doc` capability

Python script: `scripts/receipt_bbox_pipeline.py` — reference implementation.

Full results: `scripts/ocr_results.json` — 4 extracted receipts with raw_text_dump.

---

## Usage Tracking & Message Logging (Phase 7)

ohAgent tracks all LLM usage and can log all prompts/responses for audit.

### Usage Tracker

- Tracks tokens (prompt + completion) and cost per model/provider/tenant
- Stats available at `GET /api/usage/stats`
- Recent events at `GET /api/usage/recent?limit=50`

### Message Logging

Per-tenant toggle (default ON), controlled via:
```bash
# Check current setting
curl http://localhost:9090/api/logging/prefs/telegram_12345

# Turn logging off
curl -X PUT http://localhost:9090/api/logging/prefs/telegram_12345 \
  -H "Content-Type: application/json" \
  -d '{"enabled": false}'
```

Logs are stored in SQLite (`~/.ohagent/message_log.db`) with gzip compression
and archived to S3 Glacier after 30 days (requires `OHAGENT_S3_BUCKET` env var).

### Telegram Commands

| Command | Description |
|---|---|
| `/logging` | Show current logging status |
| `/logging on` | Enable message logging |
| `/logging off` | Disable message logging |

---

## 4. Troubleshooting

### Common Errors

**`WA_VERIFY_TOKEN not set` / WhatsApp adapter disabled**
→ Set the three WA_* env vars to enable WhatsApp. The daemon continues with other gateways.

**`SLACK_BOT_TOKEN not set` / Slack adapter disabled**
→ Set SLACK_BOT_TOKEN and SLACK_SIGNING_SECRET to enable Slack.

**Webhook verification fails** (WhatsApp/Slack)
→ Check that the verify token / signing secret matches the dashboard, and that the webhook URL is publicly accessible.

**`DEEPSEEK_API_KEY not set` / provider not configured**
→ Export your API key or set it via Vault.

**Bot not responding**
→ Check logs: `RUST_LOG=debug cargo run -p ohagent-daemon`
→ Verify the bot token is valid via `https://api.telegram.org/bot<TOKEN>/getMe`

**"No tokens/providers left" error**
→ The provider couldn't authenticate. Check your API key is valid and the Jcode submodule is at the correct commit.

**Compilation errors**
→ Ensure submodules are initialized: `git submodule update --init --recursive`
→ The Jcode fork (`akvarel/jcode`) must be at the commit tracked by ohAgent.

**File attachment not recognized**
→ Ensure the file is in a supported format (PNG, JPEG, GIF, WebP, PDF, TXT).
→ Check the file was fully downloaded: look for `local_path` in debug logs.
→ The attachment flow: Telegram → download → base64 encode → Jcode agent loop.

**WebSocket connection fails**
→ Verify the daemon is running: `curl http://localhost:9090/health`
→ WebSocket endpoint is at `/v1/ws/chat`, not `/ws`.
→ Use `wscat -c ws://localhost:9090/v1/ws/chat` for testing.

**Push notification not received**
→ Push is only registered on `/confirm`. Re-pair with `/pair` → `/confirm <code>`.
→ Check that `TELEGRAM_BOT_TOKEN` is set and the bot can send messages.
→ Push messages use the Telegram Bot API directly, bypassing the normal message pipeline.

### Log Locations

- All logs go to **stdout/stderr** (structured with `tracing-subscriber`)
- Systemd journal: `journalctl -u ohagent -f`
- Log level can be increased: `--log-level debug` or `RUST_LOG=debug`

### Recovery

```bash
# Restart the daemon
systemctl restart ohagent

# Rebuild from clean
cargo clean && cargo build --release -p ohagent-daemon

# Check provider connectivity
curl -H "Authorization: Bearer $DEEPSEEK_API_KEY" https://api.deepseek.com/user/balance
```
