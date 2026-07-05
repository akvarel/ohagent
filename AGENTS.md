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
│   └── ohagent-cron/       # Cron scheduler for background tasks
├── docs/                    # Documentation
├── PRODUCT-BRIEF.md         # Product lens analysis
└── CAPABILITY.md            # Implementation-ready spec (product-capability output)
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
| 1 | Daemon + Profiles + Session Storage | 🔲 Planned |
| 2 | Gateway (Telegram) | 🔲 Planned |
| 3 | Deep Memory (pgvector) | 🔲 Planned |
| 4 | Self-Learning Skills | 🔲 Planned |
