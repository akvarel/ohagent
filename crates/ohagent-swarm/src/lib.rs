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

pub mod dag;
pub mod coordinator;

pub use dag::{TaskGraph, TaskNode, TaskKind, TaskState, Dependency};
pub use coordinator::{SwarmOrchestrator, CoordinatorConfig, WorkerResult};
