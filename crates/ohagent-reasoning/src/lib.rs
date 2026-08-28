//! ohagent-reasoning: Test-time scaling via replay-driven code optimization.
//!
//! Inspired by AutoTTS (LLMs Improving LLMs) and TRM (Tiny Recursive Model).
//!
//! # Architecture
//!
//! ```text
//! CMC Controller   →   Replay Environment   →   ModelRouter   →   Swarm
//! (EMA gate,        (offline traces,         (model selection,  (dynamic
//!  trend widening,   0 LLM calls,            budget-aware)      branching)
//!  branch abandon)   optimization loop)
//! ```
//!
//! # Key components
//!
//! - **CMC** (Confidence Momentum Controller): EMA-based stopping gate,
//!   confidence-trend widening, alignment-aware depth allocation,
//!   conservative branch abandonment.
//! - **ReplayEnv**: Frozen trace store for offline optimization — evaluate
//!   controllers without making LLM calls.
//! - **BudgetTracker**: Token/cost budget with β parameterization.
//! - **RouterBridge**: Adapter between CMC and ModelRouter.

pub mod budget;
pub mod cmc;
pub mod replay;
pub mod router;

pub use budget::BudgetTracker;
pub use cmc::{BranchState, CmcConfig, CmcController, CmcDecision, PoolStats};
pub use replay::{ReplayEnv, ReplayTrace, TraceStep};
pub use router::ReasoningRouter;
