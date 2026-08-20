//! Swarm orchestration for ohAgent.
//!
//! Provides a DAG-based multi-agent task execution engine.
//! The coordinator walks a task graph, spawns subprocess agents for leaf tasks,
//! and merges results back up the DAG.
//!
//! # Architecture
//!
//! ```text
//! TaskGraph (plan)   →   SwarmOrchestrator   →   subprocess agents
//!     ↓                        ↓                        ↓
//!  DAG nodes         spawns workers          jcode CLI instances
//!  with deps         tracks state            one per leaf task
//!                    merges results           returns findings
//! ```
//!
//! ## Boundary with Jcode
//!
//! This is the ohAgent source of truth for DAG orchestration (TaskKind /
//! Dependency semantics tailored to ohAgent tools, wired via
//! `ohagent-core::tools`). Jcode ships a sibling DAG engine
//! (`jcode-swarm-core`) with a different API — it is NOT a drop-in
//! replacement. Keep them separate; revisit only if a shared DAG engine is
//! wanted as part of the Graph Engineering effort (ADR-001).

pub mod dag;
pub mod coordinator;

pub use dag::{TaskGraph, TaskNode, TaskKind, TaskState, Dependency};
pub use coordinator::{SwarmOrchestrator, CoordinatorConfig, WorkerResult};
