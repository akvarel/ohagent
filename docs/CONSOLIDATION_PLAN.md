# ohAgent ↔ Jcode — Capability Consolidation Plan

> **Date:** 2026-08-20
> **Status:** Proposed plan (needs approval before code changes)
> **Scope:** memory, skills, swarm, cron — capabilities that exist BOTH in `ohagent-*` crates
> and in the Jcode SDK submodule. Resolves roadmap §5.1.

## 0. Ground truth (audited 2026-08-20)

| Capability | ohAgent impl | Jcode impl | Actually wired in daemon? |
|---|---|---|---|
| Memory | `ohagent-memory` (2185 LOC) | `jcode-base` memory (memory.rs, memory_graph, memory_external/pgvector, memory_rerank, memory_agent) | ✅ **ohagent-memory** (MemoryEngine in daemon/api/ws/context_compressor) |
| Skills | `ohagent-skills` (1626 LOC) | `jcode-base` skill (skill.rs, skill/invocation.rs) | ✅ **ohagent-skills** (SkillRegistry + cron in daemon) |
| Swarm | `ohagent-swarm` (680 LOC) | `jcode-swarm-core` (853 LOC) | ✅ **ohagent-swarm** (SwarmOrchestrator in core/tools.rs) |
| Cron | `ohagent-cron` (390 LOC) | `jcode-overnight-core`, `jcode-plan` | ⚠️ **Neither** — real cron = `ohagent-core::scheduler::Scheduler` (+ skills-cron in daemon). `ohagent-cron` crate is **orphaned** (workspace member only, no crate depends on it). |

**Key finding:** ohAgent is NOT actually consuming the Jcode memory/skill/swarm/cron
implementations — the daemon runs its own `ohagent-*` versions. The Jcode crates are pulled in
for other purposes (`jcode-base` embeddings, providers, SDK, harness). So this is **not** a
runtime conflict; it's a **maintenance-divergence risk** (two implementations to keep in sync)
plus **one orphan crate**.

---

## 1. Recommended source-of-truth per capability

| Capability | Source of truth | Rationale |
|---|---|---|
| **Memory** | Keep **ohagent-memory** as the running store; **consume Jcode memory primitives** for the advanced bits | ohagent-memory is wired and tested; Jcode adds pgvector-external + graph + rerank not yet in ohagent. Best value: upgrade ohagent-memory to *call* Jcode's embedding/rerank/external-retrieval instead of re-implementing. |
| **Skills** | **ohagent-skills** (keep) | It is the Phase-4 deliverable (creator/evaluator/curator/security_audit) with cron loop; Jcode skill is a different shape (tool invocation). No migration. |
| **Swarm** | **ohagent-swarm** (keep) | Wired into core/tools.rs with ohAgent TaskKind/Dependency semantics; jcode-swarm-core is close in LOC but different API. No migration now. |
| **Cron** | **Delete `ohagent-cron`; use `ohagent-core::scheduler`** | `ohagent-cron` is orphaned dead code (5 tests, no consumers). Real cron already lives in `ohagent-core::scheduler::Scheduler` + skills-cron in daemon. Removing it removes the divergence. |

---

## 2. Concrete actions

### 2.1 DELETE orphan `ohagent-cron` (low risk, immediate)
- `ohagent-cron` crate has **zero dependents** (only listed in workspace `members`).
- Its tests pass but nothing uses it; the daemon uses `ohagent-core::scheduler` instead.
- **Action:**
  1. Remove `"crates/ohagent-cron"` from `Cargo.toml` members.
  2. `git rm -r crates/ohagent-cron`.
  3. `cargo build --workspace` + `cargo test --workspace` to confirm no breakage.
- **Watch:** confirm no doc/script references `ohagent_cron` before deleting.

### 2.2 MEMORY — reuse Jcode primitives (partially done; rerank/enrichment blocked)
- **Already done:** ohagent-memory's embeddings are unified with Jcode — `embeddings.rs`
  calls `jcode_base::embedding::embed` and `jcode_base::embedding::cosine_similarity`
  (same local MiniLM vector source Jcode uses). No second embedding implementation exists.
- **Blocked / deferred (not safe to wire today):**
  - **LLM rerank** (`jcode_base::memory_rerank`) requires a Jcode `Sidecar` LLM judge
    process, which ohAgent does not run.
  - **External enrichment** (`jcode_base::memory_external::enrich_context`) reads Jcode's
    global `config()` and shells out to external `graphify`/vault/pgvector services, none of
    which are configured in the ohAgent deployment; it also returns a different
    `MemoryEntry` type (`jcode-memory-types`).
- **Decision:** do NOT force rerank/enrichment into the daemon hot path (context_compressor,
  ws, openai_api) — it would spawn missing binaries and read absent global config. Revisit
  only when ohAgent adopts the Jcode memory runtime / Sidecar infrastructure wholesale.

### 2.3 SKILLS — keep ohagent-skills; document boundary (done 2026-08-20)
- ohagent-skills is the product feature; Jcode skill is a lower-level tool-invocation registry.
- **Done:** boundary documented in `ohagent-skills/lib.rs`.

### 2.4 SWARM — keep ohagent-swarm; add cross-reference (done 2026-08-20)
- ohagent-swarm's TaskKind/Dependency model is tailored to ohAgent tools.
- **Done:** boundary documented in `ohagent-swarm/lib.rs`; revisit only for a shared DAG engine tied to Graph Engineering ADR-001.

### 2.5 CRON — (covered by 2.1) after deletion, single source = `ohagent-core::scheduler`

---

## 3. Definition of done

- [x] `ohagent-cron` removed from workspace; `cargo build --workspace` + `cargo test --workspace` green.
- [x] Memory embedding source unified with Jcode (already the case; verified).
- [ ] Memory rerank / external enrichment — deferred, blocked on Jcode Sidecar + external services (see 2.2).
- [x] Skills + swarm boundaries documented (comment + roadmap link).
- [x] `TEAM_MEMORY/ROADMAP.md` §5.1 updated with consolidation plan reference.

## 4. Risks / notes

- **Memory upgrade** touches the hottest path (context_compressor, ws, openai_api all use
  MemoryEngine) — do it behind the existing `MemoryConfig` feature flag and validate with the
  16 memory tests + daemon integration.
- Jcode v0.78.1 carries upstream test debt (auth/provider/config + tool gate) unrelated to this
  work; do not let those mask a real regression. Confirm ohagent crate tests pass independently.
- No destructive data migration: memory store models stay identical.
