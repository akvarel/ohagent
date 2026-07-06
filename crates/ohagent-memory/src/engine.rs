//! Deep memory engine — main orchestrator.
//!
//! Coordinates the full memory pipeline:
//! store → embed → retrieve → summarize → nudge.
//!
//! This is the public API that the daemon and gateway use.

use std::sync::Arc;
use tracing::info;

use crate::embeddings::embed_entry;
use crate::models::{ConversationSummary, MemoryConfig, MemoryEntry, MemoryNudge, RollingSummary, SearchResult};
use crate::nudge;
use crate::retrieval;
use crate::rolling_summary;
use crate::store::MemoryStore;
use crate::summarizer;
use crate::Result;

/// The central memory engine.
///
/// Thread-safe: wraps MemoryStore in Arc for sharing across components.
pub struct MemoryEngine {
    store: Arc<MemoryStore>,
    config: MemoryConfig,
}

impl MemoryEngine {
    /// Open or create the memory database and return an engine.
    pub fn open(config: MemoryConfig) -> Result<Self> {
        let store = Arc::new(MemoryStore::open(config.clone())?);
        info!(
            db_path = %config.db_path,
            "Memory engine initialized"
        );
        Ok(Self { store, config })
    }

    // ── Memory Entries ──

    /// Store a new memory entry with automatic embedding.
    pub fn remember(&self, mut entry: MemoryEntry) -> Result<MemoryEntry> {
        // Generate embedding (skip if config says so, e.g. tests)
        if entry.embedding.is_none() && !self.config.skip_embeddings {
            let _ = embed_entry(&mut entry); // Best effort
        }

        self.store.insert(&entry)?;
        info!(
            id = %entry.id,
            source = %entry.source,
            importance = entry.importance,
            "Memory stored"
        );
        Ok(entry)
    }

    /// Retrieve a specific memory entry.
    pub fn get(&self, id: &str) -> Result<Option<MemoryEntry>> {
        self.store.get(id)
    }

    /// Delete a memory entry.
    pub fn forget(&self, id: &str) -> Result<()> {
        self.store.delete(id)
    }

    // ── Retrieval ──

    /// Search for relevant memories using semantic + temporal scoring.
    pub fn search(
        &self,
        tenant_id: &str,
        query: &str,
    ) -> Result<Vec<SearchResult>> {
        retrieval::search(&self.store, tenant_id, query, &self.config)
    }

    /// Get the most recent N memory entries for a tenant.
    pub fn recent(&self, tenant_id: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        retrieval::recent(&self.store, tenant_id, limit)
    }

    // ── Conversation Summaries ──

    /// Summarize a conversation and store it.
    pub fn summarize(
        &self,
        summary: ConversationSummary,
    ) -> Result<MemoryEntry> {
        let entry = summarizer::summarize_conversation(&self.store, &summary)?;

        // Re-read with embedding (skip if config says so)
        let mut entry = entry;
        if entry.embedding.is_none() && !self.config.skip_embeddings {
            let _ = embed_entry(&mut entry);
            if entry.embedding.is_some() {
                self.store.insert(&entry)?;
            }
        }

        Ok(entry)
    }

    /// Get a conversation summary.
    pub fn get_summary(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<Option<ConversationSummary>> {
        self.store.get_summary(tenant_id, session_id)
    }

    // ── Rolling Summaries ──

    /// Load or create a rolling summary for a session.
    pub fn get_rolling_summary(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<RollingSummary> {
        rolling_summary::load_or_create(&self.store, tenant_id, session_id)
    }

    /// Save an updated rolling summary.
    pub fn save_rolling_summary(&self, rs: &RollingSummary) -> Result<()> {
        self.store.save_rolling_summary(rs)
    }

    /// Delete a rolling summary (e.g. when session ends).
    pub fn delete_rolling_summary(&self, tenant_id: &str, session_id: &str) -> Result<()> {
        self.store.delete_rolling_summary(tenant_id, session_id)
    }

    /// Inherit a parent session's rolling summary for a swarm child.
    pub fn inherit_rolling_summary(
        &self,
        parent_session_id: &str,
        child_session_id: &str,
        tenant_id: &str,
    ) -> Result<RollingSummary> {
        let parent = rolling_summary::load_or_create(&self.store, tenant_id, parent_session_id)?;
        rolling_summary::inherit_for_child(&self.store, &parent, child_session_id)
    }

    // ── Nudges ──

    /// Generate proactive memory nudges for a tenant.
    pub fn generate_nudges(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<MemoryNudge>> {
        nudge::generate_nudges(&self.store, tenant_id, &self.config)
    }

    // ── Maintenance ──

    /// Number of entries for a tenant.
    pub fn count(&self, tenant_id: &str) -> Result<usize> {
        self.store.count(tenant_id)
    }

    /// Clear all memories for a tenant.
    pub fn clear_tenant(&self, tenant_id: &str) -> Result<()> {
        self.store.clear_tenant(tenant_id)
    }

    /// List entries for a tenant (paginated).
    pub fn list(
        &self,
        tenant_id: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.store.list_by_tenant(tenant_id, session_id, limit)
    }

    /// Access the underlying store (for advanced queries).
    pub fn store(&self) -> &Arc<MemoryStore> {
        &self.store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MemorySource;
    use chrono::Utc;

    fn test_config() -> MemoryConfig {
        MemoryConfig {
            db_path: ":memory:".to_string(),
            skip_embeddings: true,
            ..Default::default()
        }
    }

    #[test]
    fn test_remember_and_retrieve() {
        eprintln!("TEST: opening engine...");
        let engine = MemoryEngine::open(test_config()).unwrap();
        eprintln!("TEST: engine opened");

        let entry = MemoryEntry {
            id: "test-1".into(),
            tenant_id: "t1".into(),
            session_id: "s1".into(),
            content: "User prefers dark mode".into(),
            source: MemorySource::Explicit,
            importance: 0.8,
            tags: vec!["preference".into()],
            embedding: None,
            created_at: Utc::now(),
            last_accessed_at: Utc::now(),
            access_count: 0,
        };

        eprintln!("TEST: remembering...");
        engine.remember(entry).unwrap();
        eprintln!("TEST: remembered");
        assert_eq!(engine.count("t1").unwrap(), 1);

        let found = engine.get("test-1").unwrap().unwrap();
        assert_eq!(found.content, "User prefers dark mode");
        eprintln!("TEST: PASSED");
    }

    #[test]
    fn test_search_text_fallback() {
        let engine = MemoryEngine::open(test_config()).unwrap();

        for (i, text) in [
            "Rust async programming",
            "Python data science",
            "Docker containerization",
        ]
        .iter()
        .enumerate()
        {
            let entry = MemoryEntry {
                id: format!("t-{i}"),
                tenant_id: "t1".into(),
                session_id: "s1".into(),
                content: text.to_string(),
                source: MemorySource::Conversation,
                importance: 0.5,
                tags: vec![],
                embedding: None,
                created_at: Utc::now(),
                last_accessed_at: Utc::now(),
                access_count: 0,
            };
            engine.remember(entry).unwrap();
        }

        let results = engine.search("t1", "python").unwrap();
        assert!(!results.is_empty());
        assert!(results[0].entry.content.contains("Python"));
    }

    #[test]
    fn test_summarize() {
        let engine = MemoryEngine::open(test_config()).unwrap();

        let summary = ConversationSummary {
            session_id: "s1".into(),
            tenant_id: "t1".into(),
            summary: "Discussed Rust project architecture".into(),
            key_points: vec!["Use workspaces".into(), "Add tests".into()],
            decisions: vec!["Go with axum".into()],
            start_time: Utc::now(),
            end_time: Utc::now(),
            message_count: 15,
        };

        engine.summarize(summary).unwrap();
        assert!(engine.get_summary("t1", "s1").unwrap().is_some());
    }

    #[test]
    fn test_clear_tenant() {
        let engine = MemoryEngine::open(test_config()).unwrap();

        let entry = MemoryEntry {
            id: "ct-1".into(),
            tenant_id: "t2".into(),
            session_id: "s1".into(),
            content: "Test".into(),
            source: MemorySource::Explicit,
            importance: 0.5,
            tags: vec![],
            embedding: None,
            created_at: Utc::now(),
            last_accessed_at: Utc::now(),
            access_count: 0,
        };
        engine.remember(entry).unwrap();
        assert_eq!(engine.count("t2").unwrap(), 1);

        engine.clear_tenant("t2").unwrap();
        assert_eq!(engine.count("t2").unwrap(), 0);
    }
}
