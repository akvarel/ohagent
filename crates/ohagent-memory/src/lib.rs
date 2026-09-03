//! ohagent-memory: Deep memory engine for ohAgent.
//!
//! Provides persistent, semantic memory that survives across sessions.
//!
//! Architecture:
//! - SQLite for structured metadata (entries, summaries)
//! - Jcode's ONNX embedder for vector embeddings (local, offline)
//! - Cosine similarity + recency + importance for retrieval ranking
//! - Automatic conversation summarization
//! - Proactive memory nudges

pub mod embeddings;
pub mod engine;
pub mod backlog;
pub mod manager;
pub mod models;
pub mod nudge;
pub mod provider;
pub mod retrieval;
pub mod rolling_summary;
pub mod store;
pub mod summarizer;

/// Memory engine result type.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
