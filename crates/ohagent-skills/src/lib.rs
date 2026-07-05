//! ohagent-skills: Self-learning skill creation and curator.
//!
//! Autonomous skill creation from complex task patterns,
//! skill improvement during use, and periodic curation.

pub mod creator;
pub mod curator;
pub mod evaluator;
pub mod registry;

/// Skills result type.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
