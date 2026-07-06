# ohAgent — 24/7 Personal AI Super-Agent

**ohAgent** is a self-hosted, always-on AI agent that lives in your Telegram and works through Jcode — the same engine that powers one of the most capable coding agents. It remembers everything, learns from your tasks, and gets smarter every day.

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.82%2B-orange" alt="Rust 1.82+">
  <img src="https://img.shields.io/badge/status-active%20development-brightgreen" alt="Status">
  <img src="https://img.shields.io/badge/platform-Telegram-blue" alt="Telegram">
  <img src="https://img.shields.io/badge/web-dashboard-orange" alt="Web Dashboard">
</p>

---

## Why ohAgent?

Most AI tools are **reactive**: you open them, ask something, close them. They don't remember what you did yesterday. They can't ping you when something needs attention. They're not *yours*.

ohAgent is different:

| Capability | ohAgent | ChatGPT | Claude Code | Copilot |
|---|---|---|---|---|
| **Runs 24/7** | ✅ Daemon | ❌ | ❌ | ❌ |
| **Telegram native** | ✅ Built-in | ❌ | ❌ | ❌ |
| **Web dashboard** | ✅ Built-in | ❌ | ❌ | ❌ |
| **Deep memory (across sessions)** | ✅ Semantic search | ❌ (per-chat) | ❌ | ❌ |
| **Self-learns skills** | ✅ Auto-creates | ❌ | ❌ | ❌ |
| **Multi-tenant** | ✅ Day one | ❌ | ❌ | ❌ |
| **Self-hosted** | ✅ Your server | ❌ | ❌ | ✅ (limited) |
| **i18n (EN/LV/RU)** | ✅ Day one | Partial | ❌ | ❌ |
| **HashiCorp Vault** | ✅ Secrets | ❌ | ❌ | ❌ |
| **Open source** | ✅ MIT | ❌ | ❌ | ❌ |

### The "Super-Agent" difference

1. **Always on.** Runs as a daemon — systemd service, Docker, or bare metal. You message it like a colleague.

2. **Never forgets.** Deep memory engine stores everything you discuss. Semantic search finds relevant context across months of conversations. Proactive nudges suggest "hey, last time you built this API you used Axum."

3. **Learns your tasks.** Skills engine watches what you ask and automatically creates reusable skill templates from recurring patterns. "Deploy to K8s" happens three times? It becomes a learned skill.

4. **Your hardware, your data.** Everything runs locally or on your infrastructure. No vendor lock-in, no data leaving your control.

---

## Architecture

```
                  Telegram  ───  ohAgent Daemon  ───  Jcode Engine
                      │                                 │
                      ├─ /skills                        ├─ DeepSeek V4
                      ├─ /status                        ├─ Claude/OpenAI
                      ├─ "any message"                  └─ MCP tools
                      │
                      ▼
              ┌──────────────┐
              │  Web Dashboard│  ← :9090 REST API
              │  React + TW   │     Skills, Memory, Status
              └──────────────┘
                      │
              ┌───────┴────────┐
              │  Memory Engine  │  SQLite + Vector embeddings
              │  Skills Engine  │  Auto-create, evaluate, curate
              └────────────────┘
```

---

## Quick Start

### Prerequisites
- Rust 1.82+
- DeepSeek API key (or Anthropic/OpenAI)
- Telegram Bot Token (from [@BotFather](https://t.me/BotFather))

### 5-minute install

```bash
git clone --recurse-submodules https://github.com/orangehat/ohAgent.git
cd ohAgent

# Set credentials
export DEEPSEEK_API_KEY="sk-..."
export TELEGRAM_BOT_TOKEN="123456:ABC-DEF"

# Build and run
cargo build --release -p ohagent-daemon
cargo run --release -p ohagent-daemon
```

That's it. Find your bot on Telegram, send `/start`, then `/pair` to authenticate.

### Dashboard

The web dashboard is available at `http://localhost:9090` (API) and `http://localhost:5173` (dev UI):

```bash
cd crates/ohagent-dashboard
npm install && npm run dev
```

---

## What ohAgent Can Do

| Phase | Feature | Status |
|---|---|---|
| 1 | Daemon + Provider setup | ✅ |
| 2 | Telegram gateway (i18n: EN/LV/RU) | ✅ |
| 3 | Deep memory (SQLite + semantic search) | ✅ |
| 4 | Self-learning skills engine | ✅ |
| 5 | REST API + React dashboard | ✅ |
| 6 | Multi-platform gateways (WhatsApp, Slack) | 🔜 |
| 7 | Voice messages (STT → LLM → TTS) | 🔜 |
| 8 | Multi-agent orchestration (swarm) | 🔜 |

### Telegram Commands

| Command | Description |
|---|---|
| `/start` | Start the bot |
| `/pair` | Generate pairing code |
| `/confirm <code>` | Confirm pairing |
| `/new` | Start fresh conversation |
| `/lang` | Cycle EN → LV → RU |
| `/skills` | List learned skills |
| `/skill <name>` | Skill details |
| `/skilluse <name>` | Record skill usage |
| `/help` | Show all commands |

### REST API

```
GET  /health              — Health check
GET  /api/status          — Daemon status (uptime, provider, counts)
GET  /api/skills          — List skills (?tenant_id=&status=)
GET  /api/skills/:id      — Skill detail
POST /api/skills/:id/record — Record usage ({"success": true})
GET  /api/memory          — Search memories (?q=deploy&limit=20)
GET  /api/memory/:id      — Memory entry detail
```

---

## Documentation

| Doc | Contents |
|---|---|
| [MANUAL.md](docs/MANUAL.md) | Full user manual: install, config, usage, troubleshooting |
| [AGENTS.md](AGENTS.md) | Developer guide for AI agents working on the codebase |
| [PHASE4_REPORT.md](docs/PHASE4_REPORT.md) | Phase 4 completion report (skills engine) |

---

## Self-Hosting

ohAgent is designed for your infrastructure:

```bash
# systemd service
sudo cp contrib/ohagent.service /etc/systemd/system/
sudo systemctl enable --now ohagent

# Docker
docker run -e DEEPSEEK_API_KEY=... -e TELEGRAM_BOT_TOKEN=... orangehat/ohagent:latest

# Kubernetes (with Vault sidecar)
kubectl apply -f k8s/
```

See [MANUAL.md](docs/MANUAL.md#1-installation) for detailed setup including Vault integration.

---

## Development

```bash
# Run all tests
cargo test --workspace

# Run only ohAgent tests (skip Jcode)
cargo test -p ohagent-core -p ohagent-memory -p ohagent-skills -p ohagent-daemon

# Build dashboard
cd crates/ohagent-dashboard && npm install && npm run build
```

---

## License

MIT — [OrangeHat](https://github.com/orangehat)
