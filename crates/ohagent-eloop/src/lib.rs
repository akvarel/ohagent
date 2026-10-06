//! ohagent-eloop — engineering task compilation and campaign foundation.
//!
//! The first vertical slice turns a short goal into a validated, persisted
//! task contract. Execution is deliberately abstracted behind [`TaskExecutor`]
//! so the existing embedded Jcode bridge can be connected without leaking
//! Jcode types into the task, policy, storage, or reporting layers.

pub mod compiler;
pub mod context;
pub mod engine;
pub mod error;
pub mod executor;
pub mod policy;
pub mod report;
pub mod storage;
pub mod task;
pub mod usage;
pub mod verifier;

pub use compiler::{TaskCompiler, TemplateTaskCompiler};
pub use context::{RepositoryContext, RepositoryContextCollector};
pub use engine::{EloopEngine, GoalRequest, PlanResult};
pub use error::{EloopError, Result};
pub use executor::{ExecutionContext, ExecutionResult, TaskExecutor};
pub use policy::ExecutionPolicy;
pub use storage::{FileRunStore, RunStore};
pub use task::{CompiledTask, TaskCritique};
pub use usage::{ModelUsage, UsageReport};
pub use verifier::{ContractVerifier, TaskVerifier, VerificationResult};
