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

### Environment Variables (via Vault)

| Variable | Required | Description |
|---|---|---|
| `DEEPSEEK_API_KEY` | Yes* | DeepSeek API key |
| `ANTHROPIC_API_KEY` | Yes* | Anthropic API key (fallback) |
| `OPENAI_API_KEY` | Yes* | OpenAI API key (fallback) |
| `TELEGRAM_BOT_TOKEN` | For Telegram | Telegram Bot API token |
| `WA_VERIFY_TOKEN` | For WhatsApp | Meta webhook verify token |
| `WA_PHONE_ID` | For WhatsApp | WhatsApp Business phone number ID |
| `WA_ACCESS_TOKEN` | For WhatsApp | Meta permanent access token |
| `SLACK_BOT_TOKEN` | For Slack | Slack Bot User OAuth Token (xoxb-...) |
| `SLACK_SIGNING_SECRET` | For Slack | Slack Events API signing secret |
| `OPENAI_API_BASE` | Optional | OpenAI-compatible base URL for Open WebUI |
| `OHAGENT_S3_BUCKET` | Optional | S3 bucket for message log archiving |

\* At least one provider key must be set.

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
| `/skills` | List learned skills with quality scores |
| `/skill <name>` | Show skill details (triggers, instructions, stats) |
| `/skilluse <name>` | Record a successful skill usage |

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
```

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
