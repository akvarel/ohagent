# ohAgent User Stories

This document captures the key user stories that drive ohAgent's development.
Each story follows the standard format:

> **As a** [role] **I want** [feature] **So that** [benefit]

---

## Story 1: Personal AI Assistant (Daily Driver)

**As a** software developer
**I want** a 24/7 personal AI assistant that I can chat with via Telegram and terminal
**So that** I can get coding help, file management, and task automation without
switching between browser and IDE.

**Acceptance criteria:**
- Start daemon once, it stays running 24/7
- Send messages via Telegram bot or TUI client
- Agent reads/writes files, runs bash commands, searches memory
- All state survives daemon restarts
- Agent remembers context across conversations (rolling summary + semantic search)

---

## Story 2: WebSocket Chat with Cancel

**As a** ohAgent CLI user
**I want** to start a long-running agent task and be able to cancel it mid-stream
**So that** I can interrupt a wrong assumption and ask a follow-up question
without reconnecting.

**Acceptance criteria:**
- Send a chat message → see streaming tokens in TUI
- Press `Esc` → agent stops, connection stays alive
- Type new message → agent responds to the new question on the same connection
- Tool calls (bash, read, write) are displayed with their results

---

## Story 3: Multi-Provider Model Routing

**As an** ohAgent user with multiple API keys
**I want** the daemon to automatically route my requests to the best provider
for each task type
**So that** I get fast responses for simple questions (cheap models) and
thorough answers for complex tasks (powerful models), without manual selection.

**Acceptance criteria:**
- Simple chat → routed to SiliconFlow or DeepSeek V4 Flash
- Code review / complex reasoning → routed to Claude or DeepSeek Reasoner
- Vision / OCR tasks → routed to Gemini or GLM (best Latvian OCR)
- Fallback chain works when primary provider is down

---

## Story 4: Memory Across Sessions

**As a** long-term ohAgent user
**I want** the assistant to remember my projects, preferences, and past decisions
**So that** I don't have to repeat context every time I ask for help.

**Acceptance criteria:**
- Past conversations are summarized and stored in SQLite
- Agent searches memory for relevant context before answering
- `/remember "deployment server: 192.168.1.100"` explicitly saves facts
- `/recall "what was my server IP?"` retrieves stored facts
- Memory survives daemon restart

---

## Story 5: Self-Learning Skills

**As a** ohAgent user who asks for similar tasks repeatedly
**I want** the agent to automatically recognise patterns and create reusable skills
**So that** repeated tasks (e.g. "deploy to staging", "run tests") get faster
and more accurate over time.

**Acceptance criteria:**
- Agent detects when a task is repeated 2+ times
- A Proposed skill is created automatically
- Successful usage promotes the skill to Active
- `/skills` lists all skills with quality scores
- Skills are suggested proactively when applicable

---

## Story 6: Open WebUI / SDK Integration

**As a** user with external tools (Open WebUI, custom scripts)
**I want** ohAgent to expose an OpenAI-compatible API
**So that** I can use my existing tools and scripts without modification.

**Acceptance criteria:**
- `POST /v1/chat/completions` works with OpenAI SDK
- `GET /v1/models` returns available models
- Streaming (SSE) and non-streaming modes both work
- Tool-augmented completions work (agent can run bash/write/etc.)
- WebSocket endpoint provides real-time tool call visibility

---

## Story 7: Multi-Platform Gateways

**As a** user who switches between Telegram, WhatsApp, and Slack
**I want** to interact with the same ohAgent instance from any platform
**So that** I can ask questions from whichever app I'm currently using.

**Acceptance criteria:**
- Telegram: bot responds, supports photos, pairing flow
- WhatsApp: webhook receives messages, sends replies
- Slack: responds when mentioned (`@ohagent ...`)
- Conversation context is shared across platforms for the same user
- All gateways can run simultaneously

---

## Story 8: Heroku-Style Dashboard

**As a** non-technical user
**I want** a web dashboard where I can see my agent's status, skills, and memory
**So that** I understand what the agent knows and can manage it visually.

**Acceptance criteria:**
- Dashboard shows uptime, active provider, skill count, memory count
- Skill list with status filter (proposed/active/disabled/retired)
- Memory search with source type and importance filtering
- REST API powers the dashboard and is available for custom integrations

---

## Story 9: Secure Secret Management

**As a** production operator
**I want** ohAgent to read API keys and secrets from HashiCorp Vault
**So that** no secrets are stored in config files, env files, or Kubernetes Secrets.

**Acceptance criteria:**
- Vault integration follows resolution order: Vault → env → keys.toml
- All provider API keys, Telegram tokens, and database passwords live in Vault
- Graceful degradation: if Vault is unavailable, falls back to env/keys.toml
- Kubernetes sidecar (Vault Agent) injects secrets automatically
- Vault health endpoint (`/api/vault/health`) for monitoring

---

## Story 10: Crash Recovery

**As a** ohAgent operator
**I want** the daemon to start up cleanly even after an unexpected crash
**So that** I can rely on it as a 24/7 assistant without manual recovery steps.

**Acceptance criteria:**
- All SQLite databases use WAL mode for crash safety
- Migrations are auto-applied on startup (idempotent)
- Active sessions are restored from message log
- Heartbeat recovers stale sessions
- Daemon starts even if some components fail (graceful degradation)
- Health endpoint returns detailed component status (`/health`)

---

## Implementation Status

| # | Story | Phase | Priority |
|---|-------|-------|----------|
| 1 | Personal AI Assistant | Core | P0 |
| 2 | WebSocket Chat with Cancel | Phase 13 | P0 |
| 3 | Multi-Provider Routing | Phase 4 | P1 |
| 4 | Memory Across Sessions | Phase 3 | P1 |
| 5 | Self-Learning Skills | Phase 4 | P2 |
| 6 | Open WebUI / SDK Integration | Phase 7 | P1 |
| 7 | Multi-Platform Gateways | Phase 8 | P2 |
| 8 | Dashboard | Phase 5 | P2 |
| 9 | Secure Secret Management | Phase 10 | P1 |
| 10 | Crash Recovery | Phase 13 | P1 |

---

See [MANUAL.md](MANUAL.md) for detailed installation, configuration, and usage instructions.
