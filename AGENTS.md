# ohAgent Project Instructions

## Scope

ohAgent is OrangeHat's always-on personal AI agent built on Jcode. It provides Telegram-first chat, daemonized sessions, memory, self-learning skills, a dashboard, OpenAI-compatible API, usage tracking, and deployment manifests.

## Architecture

- Rust workspace with crates under `crates/`.
- `ohagent-core`: Jcode bridge, provider routing, session logic.
- `ohagent-daemon`: long-running service, API, cron, gateway wiring.
- `ohagent-gateway`: platform adapters, Telegram bot, pairing, i18n.
- `ohagent-memory`: tenant-scoped SQLite memory and semantic retrieval.
- `ohagent-skills`: skill lifecycle, usage tracking, evaluation, curation.
- `ohagent-dashboard`: React + Vite + Tailwind UI.
- `jcode/` is a git submodule. Do not modify or advance it unless the task explicitly requires a Jcode update.

## Invariants

- Preserve tenant isolation. Scope persistent data, API queries, sessions, memory, skills, usage, and gateway state by `tenant_id`.
- Keep user-visible text localizable in English, Latvian, and Russian.
- Store real secrets in HashiCorp Vault or environment configuration. Never hardcode or commit tokens.
- Keep ohAgent as an orchestrator around Jcode. Do not reimplement Jcode engine behavior locally.
- Prefer graceful degradation when optional providers, embeddings, OCR, Vault, or gateways are unavailable.
- Keep persistent schemas and API formats backward compatible.

## Development

- Inspect `git status --short --branch` before edits and preserve unrelated work, especially submodule changes.
- Use small focused commits with `AI-assisted: Jcode` in the commit message.
- Keep documentation, comments, identifiers, API responses, and commit messages in English.
- Use TDD where practical for behavior changes.
- Do not deploy, restart production services, publish releases, or change production secrets without explicit user confirmation.

## Validation

Choose the smallest validation that covers the change:

- Rust formatting: `cargo fmt --all -- --check`
- Rust tests: `cargo test --workspace`
- Rust lint/build when relevant: `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`
- Dashboard: run commands from `crates/ohagent-dashboard/` such as `npm test`, `npm run lint`, or `npm run build` when UI code changes.
- Docs/config-only changes: verify affected files render/read correctly and review `git diff --check`.
