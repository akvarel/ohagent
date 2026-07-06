# ohAgent — Project Instructions

## What is ohAgent?

ohAgent is the OrangeHat AI Agent — a 24/7 personal AI assistant built on top of Jcode.
It combines the coding power of Jcode with Hermes-style gateway, memory, and self-learning.

## Project Structure

```
ohAgent/
├── crates/
│   ├── ohagent-core/       # Core agent loop, config, provider bridge
│   ├── ohagent-daemon/     # 24/7 daemon process (systemd service)
│   ├── ohagent-gateway/    # Messaging gateway (Telegram first)
│   ├── ohagent-memory/     # Deep memory engine (SQLite + pgvector)
│   ├── ohagent-skills/     # Self-learning skill creation & curator
│   ├── ohagent-dashboard/  # React web dashboard (TypeScript + Tailwind)
│   └── ohagent-cron/       # Cron scheduler for background tasks
├── docs/                    # Documentation
└── README.md                # Front page (overview, why ohAgent, quick start)
```

## Design Rules

- **Rust first.** Core components in Rust for performance and safety.
- **Python sidecar for AI-heavy.** Skill creation, curator, memory nudges — Python via MCP.
- **Jcode as engine.** ohAgent embeds/spawns Jcode for agent execution, not reimplementing.
- **Multi-tenant from day one.** Every session/agent scoped to `tenant_id`.
- **Vault for secrets.** All credentials, API keys, tokens in HashiCorp Vault.
- **OpenTelemetry.** Tracing and metrics from day one.

## Development Rules

- Commit as you go — small, focused commits with `AI-assisted: Jcode` trailer.
- ALL documentation, comments, commit messages in English.
- Test-driven where practical. Integration tests for gateway and memory.

## Architecture

```
┌─────────────────────────────────────────────┐
│                 ohAgent Daemon               │
│  ┌──────────┐  ┌──────────┐  ┌────────────┐ │
│  │ Gateway  │  │  Cron    │  │  Scheduler  │ │
│  │(Telegram)│  │ Scheduler│  │(Background) │ │
│  └────┬─────┘  └────┬─────┘  └─────┬──────┘ │
│       │              │               │       │
│       ▼              ▼               ▼       │
│  ┌──────────────────────────────────────┐   │
│  │         ohAgent Core (Loop)          │   │
│  │  ┌─────────┐  ┌─────────┐  ┌───────┐ │   │
│  │  │ Session │  │ Provider│  │ Tool  │ │   │
│  │  │ Manager │  │ Bridge  │  │System │ │   │
│  │  └────┬────┘  └────┬────┘  └───┬───┘ │   │
│  │       │              │           │     │   │
│  │       ▼              ▼           ▼     │   │
│  │  ┌──────────────────────────────────┐ │   │
│  │  │         Jcode Engine             │ │   │
│  │  │  (turn_loops + swarm + MCP)      │ │   │
│  │  └──────────────────────────────────┘ │   │
│  └──────────────────────────────────────┘   │
│                                              │
│  ┌──────────┐  ┌──────────┐  ┌────────────┐ │
│  │ Memory   │  │ Skills   │  │ Auth/RBAC   │ │
│  │ Engine   │  │ Engine   │  │ Manager     │ │
│  └──────────┘  └──────────┘  └────────────┘ │
└─────────────────────────────────────────────┘
```

## Phase Roadmap (MVP = Phases 1-2)

| Phase | Component | Status |
|---|---|---|
| 1 | Daemon + Profiles + Session Storage | ✅ Done |
| 2 | Gateway (Telegram) | ✅ Done |
| 3 | Deep Memory (SQLite + vector) | ✅ Done |
| 4 | Self-Learning Skills | ✅ Done |
| 5 | REST API + React Dashboard | ✅ Done |
| 6 | Model Router + Multi-Provider + Agent Tools | ✅ Done |
| 7 | OpenAI-Compatible API + Usage Tracking | ✅ Done |
| 8 | Multi-Platform Gateways (WhatsApp, Slack) | ✅ Done |
| 9 | Swarm Orchestration (DAG-based multi-agent) | ✅ Done |
| 10 | Vault Secrets Integration | ✅ Done |
| 11 | Kubernetes Deployment (Kustomize) | ✅ Done |
| 12 | E2E Testing (Cucumber/Gherkin) | ✅ Done |
| 13 | Session Persistence, Push, Cron, WebSocket, Attachments, MCP | ✅ Done |

## Phase 3 Implementation Details

### Memory Architecture
- **SQLite** with WAL mode for structured storage (entries, summaries, embeddings)
- **Jcode ONNX embedder** (all-MiniLM-L6-v2) for vector embeddings — gated behind `embeddings` feature
- Pure **Rust cosine similarity** fallback when embeddings are disabled
- **Semantic + temporal scoring**: `combined = α*similarity + β*recency + γ*importance`

### Memory Schema
| Table | Purpose |
|---|---|
| `memory_entries` | Core memory records with tenant scoping |
| `memory_embeddings` | 384-dim float vectors as BLOBs |
| `conversation_summaries` | Structured summaries of completed sessions |

### Key Modules
| Module | Purpose |
|---|---|
| `store.rs` | SQLite CRUD, schema init, embedding serialization |
| `engine.rs` | `MemoryEngine` — public orchestrator API |
| `embeddings.rs` | Jcode embedder wrapper with feature gate |
| `retrieval.rs` | search() — semantic + text fallback pipeline |
| `summarizer.rs` | Conversation summarizer + memory entry creation |
| `nudge.rs` | Proactive nudges from related past context |

### Daemon Integration
- Memory engine initialized at daemon startup (graceful fallback if unavailable)
- Stored in `Daemon.memory: Option<Arc<MemoryEngine>>`
- Ready for gateway integration (nudge injection into agent context)

## Phase 4 Implementation Details

### Skills Architecture
- **SQLite** with WAL mode for skills + usage tables
- **Creator**: analyzes conversation patterns (keyword co-occurrence on 20 task verbs), proposes `Proposed` skills
- **Evaluator**: tracks usage → quality scoring → `Proposed→Active` promotion, `Disabled` demotion, stale `Retired` retirement
- **Curator**: prunes retired skills >90 days, merges similar skills (Jaccard overlap), enforces per-tenant limits
- **LLM prompt builder** for richer extraction (ready, not yet wired)

### Skills Lifecycle
```
Conversation → creator → Proposed → evaluator → Active
                                      ↓
                                   Disabled → evaluator → Active
                                      ↓
                                   Retired → curator → Deleted
```

### Skills Schema
| Table | Purpose |
|---|---|
| `skills` | Skill templates (name, triggers, instructions, quality_score, status) |
| `skill_usage` | Invocation records (success/failure, duration, session) |

### Telegram Skills Commands
| Command | Description |
|---|---|
| `/skills` | List learned skills with quality % |
| `/skill <name>` | Full detail: triggers, instructions, usage stats |
| `/skilluse <name>` | Record successful use (boosts quality) |

### Daemon Cron
- Every 5 min: evaluate all tenant skills
- Every 10 min: scan conversations → propose new skills
- Every 10 min: curate (merge, prune, enforce limits)

## Phase 5 Implementation Details

### REST API
- Axum server on `:9090` (same port as health check)
- Modular `api.rs` with `ApiState` shared state
- Endpoints: `/health`, `/api/status`, `/api/skills`, `/api/skills/:id`, `/api/skills/:id/record`, `/api/memory`, `/api/memory/:id`

### Dashboard
- React 18 + TypeScript + Vite + Tailwind CSS
- Located at `crates/ohagent-dashboard/`
- Pages: Dashboard (status cards), Skills (list + filter + detail + record use), Memory (search)
- Vite proxies `/api` → `:9090` in dev
- Production build: `npm run build` → `target/dashboard-dist/`

## Phase 1-2 Implementation Details

### Jcode Integration
- Jcode is embedded as a **git submodule** (`akvarel/jcode` fork, synced to v0.35.1+)
- Headless sessions via `create_headless_session` + `process_message_streaming_mpsc`
- `JcodeBridge` in `ohagent-core` wraps the API cleanly
- **Two upstream fixes contributed:**
  1. `fork()` preserves `openai_compatible_profiles` (DeepSeek runtime)
  2. `set_model()` preserves active OpenAI-compatible profile on OpenRouter fallback

### Gateway Architecture
- `PlatformAdapter` trait — unified interface for messaging platforms
- `PairingManager` — time-limited pairing codes (6 chars, 10 min TTL)
- `SessionManager` — per-chat Jcode sessions via `DashMap`
- `Dispatcher` — message routing with command handling
- i18n: EN, LV, RU with `Lang::from_code()`

### Telegram Bot
- Built with **teloxide 0.13** (long-polling mode)
- Commands: `/start`, `/pair`, `/confirm`, `/help`, `/new`, `/lang`, `/stop`, `/status`
- Typing indicator during processing
- Auto-pairs with tenant scoping (`telegram_{user_id}`)

### Provider Setup
- `setup_provider_runtimes()` registers OpenRouter + OpenAI-compatible profiles
- DeepSeek V4 Flash as primary provider (via `DEEPSEEK_API_KEY`)
- Claude/OpenAI fallback chain

### Key Files
| File | Purpose |
|---|---|
| `crates/ohagent-core/src/jcode_bridge.rs` | Headless session bridge |
| `crates/ohagent-gateway/src/adapter.rs` | PlatformAdapter trait |
| `crates/ohagent-gateway/src/i18n.rs` | Multi-language strings |
| `crates/ohagent-gateway/src/pairing.rs` | User pairing/authorization |
| `crates/ohagent-gateway/src/session.rs` | Per-chat session manager |
| `crates/ohagent-gateway/src/dispatch.rs` | Message routing |
| `crates/ohagent-gateway/src/platforms/telegram.rs` | Telegram bot adapter |
| `crates/ohagent-daemon/src/lib.rs` | Daemon main loop |
| `docs/MANUAL.md` | User manual |
