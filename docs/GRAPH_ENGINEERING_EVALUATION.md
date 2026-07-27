# Graph Engineering Paper Evaluation

## Purpose

This document evaluates the architecture described in `Graph-Engineering-Athropic-Karpathy-Loop.pdf` against two OrangeHat projects:

- **engineering-loop (Eloop):** an independent open-source Go engineering workflow orchestrator.
- **ohAgent:** the Rust-based, always-on multi-tenant agent platform.

The projects have different product boundaries. Eloop will be completed as a standalone open-source project and retained as a reference implementation. ohAgent will not depend on Eloop at runtime. Instead, a Rust-native ohAgent module will reuse and improve the proven architectural ideas.

See [RUST_GRAPH_ENGINEERING_MODULE.md](RUST_GRAPH_ENGINEERING_MODULE.md) for the implementation contract.

## Paper Thesis

The paper combines five mechanisms:

1. A bounded Karpathy-style ratchet loop that makes one change, evaluates it, retains improvements, and reverts failures.
2. Dynamic planning and artifact contracts for tasks whose execution path cannot be fixed in advance.
3. Parallel swarm execution for independent hypotheses.
4. Two distinct graphs:
   - an experiment or commit DAG for work lineage;
   - a knowledge graph for entities, claims, relations, sources, and contradictions.
5. Separate control, execution, artifact, graph, and observability planes.

The central distinction is important: a task dependency DAG is not an experiment DAG, and vector memory is not a provenance-aware knowledge graph.

## Executive Assessment

| Project | Architectural alignment | Strongest area | Main missing component |
|---|---:|---|---|
| Eloop | Approximately 75% | Bounded control, verification, recovery, and artifact persistence | Parallel swarm and durable experiment lineage |
| ohAgent | Approximately 60% | Worker runtime, task DAG, semantic memory, and reasoning controls | Verified engineering ratchet and typed provenance graph |
| Combined concepts | Approximately 80% | Most required mechanisms have at least one reference implementation | One coherent Rust-native architecture connecting attempts, artifacts, evaluations, and lineage |

The percentages indicate coverage of the paper's architecture, not general product quality.

## Eloop Assessment

### Implemented Well

Eloop provides a strong reference for the control plane:

- validated task contracts and acceptance criteria;
- independent task critic;
- explicit local verification commands;
- independently routed model verification;
- persisted campaign state and interruption recovery;
- dependency-aware task scheduling;
- task, retry, failure, cost, and active-runtime limits;
- stale repository detection;
- scope and dirty-tree enforcement;
- production, release, migration, destructive-operation, and secret boundaries;
- immutable run evidence, patches, command logs, model logs, and reports.

Relevant implementation paths:

- `engineering-loop/internal/campaign/state.go`
- `engineering-loop/internal/campaignengine/engine.go`
- `engineering-loop/internal/campaignrun/campaignrun.go`
- `engineering-loop/internal/autopilot/runtime.go`
- `engineering-loop/internal/taskcompile/`

Eloop completion is verifier-owned. An executor cannot mark a task completed without an approved `VerificationSummary`.

### Gaps Relative to the Paper

Eloop remains primarily a goal-completion loop rather than a metric optimization loop. It does not natively perform:

```text
measure baseline
create candidate
run correctness gates
measure candidate
compare against baseline
promote or reject
branch the next experiment
```

Its dependency graph records task prerequisites, not experiment ancestry. It does not yet preserve parent experiment, hypothesis, candidate commit, metrics, environment fingerprint, promotion decision, or alternative leaves.

Eloop also does not currently provide a parallel isolated worker swarm. Its concurrency safety model correctly detects external mutations rather than coordinating concurrent writers.

### Validation

The full Go test suite passed. The consequential campaign, campaign engine, campaign run, task compiler, and workflow suites were also rerun without the Go test cache and passed.

## ohAgent Assessment

### Implemented Well

ohAgent already provides important execution-plane primitives:

- a Rust task DAG with `explore`, `implement`, `verify`, `fix`, and `synthesize` nodes;
- dependency-aware runnable-node selection;
- bounded concurrency, retries, worker timeout, and depth configuration;
- subprocess-based Jcode workers;
- persistent multi-tenant semantic memory and rolling summaries;
- reasoning traces and offline controller replay evaluation;
- skill usage scoring, promotion, disabling, retirement, and reactivation;
- provider routing, token accounting, and cost-aware reasoning.

Relevant implementation paths:

- `crates/ohagent-swarm/src/dag.rs`
- `crates/ohagent-swarm/src/coordinator.rs`
- `crates/ohagent-core/src/tools.rs`
- `crates/ohagent-memory/src/models.rs`
- `crates/ohagent-memory/src/store.rs`
- `crates/ohagent-reasoning/src/replay.rs`
- `crates/ohagent-skills/src/evaluator.rs`

### Gaps Relative to the Paper

The current memory system stores free-form content, tags, importance, source category, and embeddings. It does not yet store typed entities, claims, source-backed relations, aliases, contradictions, validity intervals, or stable evidence edges. It is semantic memory, not the paper's knowledge graph.

The swarm task DAG controls execution order for one run. It is not a durable experiment DAG and cannot answer lineage questions across runs.

Current swarm success means that a subprocess exited successfully. It does not mean that acceptance criteria passed or that an independent verifier approved the result.

### Concrete Swarm Blockers

1. Dependency outputs are not injected into dependent node prompts, despite a comment saying they are.
2. The public `swarm_run` tool returns only 500-byte output previews.
3. Invalid node UUIDs and invalid dependencies are silently skipped.
4. Unknown task kinds silently become `explore`.
5. Process exit success is treated as node success.
6. All workers share one working directory.
7. `max_depth` is stored but not enforced by the coordinator.
8. `worker_id` is never assigned.
9. Worker timeout is hard-coded to 600 seconds in the tool adapter.
10. There is no swarm-wide token, cost, output-size, graph-write, model-call, or wall-clock budget.
11. Retry attempts lack stable attempt identities and immutable attempt artifacts.
12. Tests do not exercise real subprocess execution, timeout, retry, dependency artifact transfer, cancellation, or workspace isolation.
13. The committed root `Cargo.lock` is inconsistent with focused locked workspace tests. `cargo metadata --locked --no-deps` succeeds, but focused `cargo test --locked` requires a lockfile update and fails.

### Validation

Focused memory, reasoning, skills, and swarm tests passed after Cargo refreshed dependency resolution. Re-running with the committed lockfile exposed the reproducibility issue described above. This must be fixed before implementing the new module.

## Comparative Scorecard

Scores are out of five.

| Capability | Eloop | ohAgent | Rust-native target |
|---|---:|---:|---:|
| Measurable evaluation | 4.5 | 3.0 | 5.0 |
| Independent evaluator | 4.5 | 2.0 | 5.0 |
| Bounded autonomy | 5.0 | 3.5 | 5.0 |
| Durable artifacts | 4.5 | 3.0 | 5.0 |
| Dynamic planning | 4.5 | 3.5 | 4.5 |
| Task dependency DAG | 3.5 | 4.0 | 5.0 |
| Parallel workers | 1.5 | 3.5 | 5.0 |
| Retry and recovery | 4.5 | 3.0 | 5.0 |
| Reversible experiments | 2.0 | 1.5 | 5.0 |
| Alternative lineage preservation | 1.5 | 2.0 | 5.0 |
| Typed knowledge graph | 1.0 | 1.5 | Deferred until justified |
| Provenance and contradictions | 1.5 | 1.5 | 4.5 |
| Production safety | 4.5 | 3.0 | 5.0 |
| Observability and cost accounting | 4.5 | 3.5 | 5.0 |

## Architecture Decision

The target is not an Eloop-to-ohAgent integration.

The target is a Rust-native ohAgent graph-engineering module that:

- treats Eloop as a behavioral and safety reference;
- reuses existing ohAgent/Jcode runtime primitives;
- owns its Rust task contracts, verifier, artifacts, budgets, recovery, and experiment lineage;
- keeps engineering-loop independent and free from ohAgent-specific dependencies;
- does not copy Go code or bind the projects through a runtime protocol.

## Recommended Build Order

1. Restore reproducible locked Rust builds.
2. Harden the existing swarm DAG and worker lifecycle.
3. Add typed Rust contracts and strict plan validation.
4. Add isolated Git worktrees and immutable artifacts.
5. Add independent local and model verification.
6. Add append-only campaign state and recovery.
7. Add experiment lineage and metric ratchets.
8. Add a domain knowledge graph only after a real relationship/provenance use case justifies it.

## Design Warnings

- Do not combine the task DAG, experiment DAG, and domain knowledge graph into one universal graph.
- Do not allow a worker to approve its own output.
- Do not run parallel writers in one checkout.
- Do not equate semantic similarity with factual support.
- Do not optimize a primary metric without correctness and non-regression constraints.
- Do not introduce a specialized graph database before PostgreSQL traversal becomes insufficient.
- Preserve rejected experiments as evidence without promoting their changes.
