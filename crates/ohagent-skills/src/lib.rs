//! ohagent-skills: Self-learning skill creation and curator.
//!
//! Autonomous skill creation from complex task patterns,
//! skill improvement during use, and periodic curation.
//!
//! The lifecycle:
//! 1. **Creator** analyzes conversations → proposes new skills (status: Proposed)
//! 2. **Evaluator** tracks usage → promotes to Active when proven
//! 3. **Curator** periodically prunes, merges, and cleans up
//!
//! Skills are stored in SQLite alongside memory, scoped per tenant.
//!
//! ## Boundary with Jcode
//!
//! This is the ohAgent source of truth for the self-learning skill lifecycle
//! (creator/evaluator/curator + daemon cron loop). Jcode ships a lower-level
//! tool-invocation skill registry (`jcode-base` `skill`/`skill::invocation`) that
//! serves a different purpose. Do NOT merge them; keep the ohAgent lifecycle here.

pub mod creator;
pub mod curator;
pub mod evaluator;
pub mod models;
pub mod registry;
pub mod security_audit;

/// Re-export key types for convenience.
pub use models::{Skill, SkillConfig, SkillOrigin, SkillStatus, SkillUsage};

/// Skills result type.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
