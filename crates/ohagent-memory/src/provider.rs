//! Memory provider trait — pluggable memory backends.
//!
//! Allows different storage backends (SQLite, PostgreSQL, Dragonfly, etc.)
//! to be used as memory providers. Each provider implements the `MemoryProvider`
//! trait and is registered with the `MemoryManager`.

use crate::models::{ConversationSummary, MemoryEntry, RollingSummary, SearchResult};
use crate::Result;

/// Pluggable memory backend trait.
///
/// Implement this trait to create custom memory storage backends.
/// The `MemoryManager` orchestrates one primary provider with optional
/// fallback/replica providers.
pub trait MemoryProvider: Send + Sync {
    /// Human-readable provider name (e.g. "sqlite", "postgres", "dragonfly").
    fn name(&self) -> &str;

    /// Check if the provider is available and operational.
    fn is_available(&self) -> bool {
        true
    }

    // ── Memory Entries ──

    /// Store a new memory entry (with embedding, if any).
    fn insert(&self, entry: &MemoryEntry) -> Result<()>;

    /// Retrieve a memory entry by ID.
    fn get(&self, id: &str) -> Result<Option<MemoryEntry>>;

    /// Delete a memory entry.
    fn delete(&self, id: &str) -> Result<()>;

    /// List entries for a tenant, optionally filtered by session.
    fn list_by_tenant(
        &self,
        tenant_id: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>>;

    /// Get all entries with embeddings (for vector search).
    fn entries_with_embeddings(&self, tenant_id: &str) -> Result<Vec<(MemoryEntry, Vec<f32>)>>;

    /// Number of entries for a tenant.
    fn count(&self, tenant_id: &str) -> Result<usize>;

    /// Delete all entries for a tenant.
    fn clear_tenant(&self, tenant_id: &str) -> Result<()>;

    // ── Conversation Summaries ──

    /// Save a conversation summary.
    fn save_summary(&self, summary: &ConversationSummary) -> Result<()>;

    /// Retrieve a conversation summary.
    fn get_summary(&self, tenant_id: &str, session_id: &str)
        -> Result<Option<ConversationSummary>>;

    // ── Rolling Summaries ──

    /// Save (upsert) a rolling summary.
    fn save_rolling_summary(&self, rs: &RollingSummary) -> Result<()>;

    /// Retrieve a rolling summary.
    fn get_rolling_summary(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<Option<RollingSummary>>;

    /// Delete a rolling summary.
    fn delete_rolling_summary(&self, tenant_id: &str, session_id: &str) -> Result<()>;
}
