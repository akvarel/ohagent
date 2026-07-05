//! ohagent-cron: Cron scheduler for ohAgent background tasks.
//!
//! First-class agent tasks (not shell tasks).
//! Supports cron expressions, intervals, skills attachment, and platform delivery.

pub mod job;
pub mod scheduler;

/// Cron result type.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
