//! ohagent-memory: Deep memory engine for ohAgent.
//!
//! Structured memory with pgvector for semantic search,
//! SQLite for session storage with FTS5 full-text search,
//! and proactive memory nudges.

pub mod engine;
pub mod models;
pub mod nudge;
pub mod retrieval;
pub mod store;

/// Memory engine result type.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
