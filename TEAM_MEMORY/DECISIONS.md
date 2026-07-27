# Architecture Decisions

## ADR-001: Keep Eloop Independent and Build Graph Engineering Natively in Rust

**Date:** 2026-07-27
**Status:** Accepted

### Context

The Graph Engineering / Anthropic / Karpathy architecture was evaluated against the standalone Go `engineering-loop` project and the Rust ohAgent platform. Eloop has strong campaign control, verification, recovery, and artifact patterns. ohAgent has the worker runtime, task DAG, semantic memory, provider routing, and multi-tenant platform primitives.

### Decision

- Complete and retain Eloop as an independent open-source Go project.
- Do not make ohAgent depend on the Eloop binary, Go packages, campaign storage, or runtime protocol.
- Build an equivalent and improved graph-engineering capability as Rust-native ohAgent modules.
- Use Eloop as a behavioral and safety reference implementation.
- Store the evaluation and target specification in `docs/GRAPH_ENGINEERING_EVALUATION.md` and `docs/RUST_GRAPH_ENGINEERING_MODULE.md`.
- Keep task dependencies, experiment lineage, and future domain knowledge graphs as separate data models.

### Consequences

- Eloop remains reusable and free from OrangeHat product coupling.
- ohAgent receives native integration with Jcode, Vault, provider routing, multi-tenancy, i18n, and Rust deployment infrastructure.
- Concepts may be ported, but code and runtime state are not shared.
- Rust implementation must independently reproduce Eloop's verification and safety guarantees.
- The first implementation milestone is swarm correctness and reproducibility, not a full campaign engine.
