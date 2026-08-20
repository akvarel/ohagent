# ohAgent — Development Roadmap (reconciled)

> **Date:** 2026-08-20
> **Status:** Living document — reflects actual code state, reconciled against `PRODUCT-BRIEF.md` (2026-07-05) and what the underlying Jcode SDK provides out-of-the-box.

## 0. How to read this

This roadmap is the PRODUCT-BRIEF phase plan **checked against what already exists** in
`crates/` and what the Jcode SDK (submodule `jcode/`) ships natively. `✅ Done` means
implemented and tested. `🟡 Partial` means scaffold exists but not production-ready.
`⬜ Not started` means only designed or deferred.

> Rule of thumb: ohAgent is a **thin orchestration layer** over Jcode. Do NOT rebuild what
> Jcode already provides (memory, swarm, skills, cron, providers, desktop, plan, embedding).
> Reuse `jcode-*` crates; only add ohAgent value (daemon, gateway, tenancy, plugins, sandbox).

---

## 1. Core runtime (MVP = Phases 1–2)

| Capability | Status | Where |
|---|---|---|
| 24/7 daemon | ✅ Done | `ohagent-daemon` (api, ws, webhooks, health, metrics, auth, rate_limiter) |
| Agent loop / provider bridge | ✅ Done | `ohagent-core` (agent, agent_runner, model_router, session, tools) |
| Jcode SDK integration | ✅ Done | `jcode-sdk`, `jcode-base`, `jcode-provider-*` deps in core |
| Telegram gateway | ✅ Done | `ohagent-gateway` (telegram + platform adapters, i18n) |
| Session storage | ✅ Done | `ohagent-core` session_store; Jcode session types |
| Multi-tenant profile/isolation | ✅ Done | `ohagent-daemon` tenancy (socket-safe homes, SHA-256 keys) |
| Packaging / compose | ✅ Done | `docker-compose.yml` (vault, ohagent), Dockerfile, k8s/ |

**MVP thesis is effectively shipped.** The original PRODUCT-BRIEF success criterion
(Telegram → task → reply, background execution) is implemented.

---

## 2. Intelligence layer (v1.0 = Phases 3–4)

| Capability | Status | Where |
|---|---|---|
| Deep memory (vector/pgvector) | ✅ Done | `ohagent-memory` (embeddings, retrieval, rolling_summary, summarizer) + Jcode `jcode-base` memory |
| External memory (Graphify/vault/pgvector) | ✅ Done | Jcode fork feature (memory_external, vault, graphify) |
| Self-learning skills | ✅ Done | `ohagent-skills` (creator, evaluator, curator, security_audit) |
| Skill lifecycle + cron loop | ✅ Done | daemon `start_skills_cron` (PHASE4_REPORT) |
| Cron scheduler | ✅ Done | `ohagent-cron` (job, scheduler) |
| Swarm / DAG multi-agent | ✅ Done | `ohagent-swarm` (coordinator, dag) + Jcode `jcode-swarm-core` |
| Reasoning / test-time scaling (CMC) | ✅ Done | `ohagent-reasoning` (cmc, replay, budget, router) |
| Provider routing + metrics | ✅ Done | `ohagent-provider-metrics` + Jcode provider catalog |

**v1.0 intelligence stack is built.** Note: skills/memory/swarm exist BOTH in ohAgent crates
and in Jcode — a reconciliation/consolidation decision is needed (see §5).

---

## 3. Extensions & product surface

| Capability | Status | Where / Notes |
|---|---|---|
| Plugin system | ✅ Done | `ohagent-plugins` (pre/post pipeline) |
| PII / secret redaction | ✅ Done | `ohagent-pii-redactor` |
| Sandbox (isolated compute) | 🟡 Partial | `ohagent-sandbox` (lib), `ohagent-infra-launcher` (GPU on-demand) — heavy VMs design in docs/SANDBOX*.md |
| Desktop automation | 🟡 Partial | `ohagent-desktop-mcp` (screenshot/mouse/keyboard) + Jcode `jcode-desktop2` (richer desktop app) — decide which is canonical |
| Web dashboard | 🟡 Partial | `ohagent-dashboard` (React, vite) — not in workspace members, no tests |
| TUI client | ✅ Done | `ohagent-cli` |
| Aggregator / billing | ✅ Done | `ohagent-aggregator-core` + plugin (open/closed split) |

---

## 4. Deferred / explicitly NOT in scope

From PRODUCT-BRIEF anti-goals, plus anything still unbuilt:

- ⬜ OAuth/app integrations (Phase 5) — deferred
- ⬜ Other messengers (Discord/WhatsApp/Slack/Signal) — Telegram first
- ⬜ Voice interface — deferred
- ⬜ Managed SaaS for clients — Phase ∞
- 🟡 Full desktop automation unification — Phase 6, decide jcode-desktop2 vs ohagent-desktop-mcp

---

## 5. Reconciliation gaps / decisions to make

These are the actual open work items surfaced by this audit (not a full rebuild):

1. **Consolidate duplicate capabilities.** Memory, skills, swarm, cron exist in both
   `ohagent-*` crates and Jcode (`jcode-base` memory/skill, `jcode-swarm-core`,
   `jcode-overnight-core`, `jcode-plan`). Pick the source of truth per capability to avoid
   divergence and double maintenance.
2. **Desktop: one implementation.** `ohagent-desktop-mcp` vs Jcode `jcode-desktop2` overlap.
   Decide canonical path before Phase 6.
3. **Dashboard maturity.** `ohagent-dashboard` is React but not a workspace member and has no
   tests; decide if it becomes a first-class deliverable.
4. **Sandbox → production.** `ohagent-sandbox` + `ohagent-infra-launcher` are thin; the
   "isolated compute VM" story needs hardening (docs/SANDBOX.md, SANDBOX-SERVERS.md).
5. **Upstream test debt.** Jcode v0.78.1 tag carries failing tests (auth/provider/config +
   tool gate) — confirmed upstream bugs, not ours. Track so the next Jcode bump picks up fixes.
6. **Graph Engineering module.** ADR-001 accepted (Rust-native, not Eloop). First milestone:
   swarm correctness/reproducibility. Partially present via `ohagent-swarm` + `jcode-plan`.

---

## 6. Proposed near-term order

1. Resolve consolidation decisions (item §5.1–5.3) — lowest effort, highest long-term value.
2. Harden sandbox + infra-launcher to production (item §5.4).
3. Land the active SDK-runtime branch (`feature/jcode-sdk-runtime-boundary`) to `master`.
4. Next Jcode bump once upstream fixes its v0.78.1 test debt (item §5.5).
5. Progress Graph Engineering per ADR-001 (item §5.6).
