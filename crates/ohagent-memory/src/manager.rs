//! Memory manager — orchestrates memory providers.
//!
//! Manages one primary provider and zero or more secondary/fallback providers.
//! All read/write operations go to the primary provider. Secondary providers
//! can be added for read-replica or fallback scenarios.

use std::sync::Arc;
use tracing::info;

use crate::models::{ConversationSummary, MemoryEntry, MemoryNudge, RollingSummary, SearchResult};
use crate::provider::MemoryProvider;
use crate::Result;

/// Orchestrates one or more memory providers.
///
/// By default, the manager has no providers. Call `add_provider` to register
/// the active backend. The first registered provider is treated as primary.
pub struct MemoryManager {
    providers: Vec<Box<dyn MemoryProvider>>,
}

impl MemoryManager {
    /// Create a new empty memory manager.
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Register a memory provider. The first added provider is the primary.
    pub fn add_provider(&mut self, provider: Box<dyn MemoryProvider>) {
        info!(
            name = %provider.name(),
            index = self.providers.len(),
            "Memory provider registered"
        );
        self.providers.push(provider);
    }

    /// Get the primary provider, or None if none registered.
    fn primary(&self) -> Option<&dyn MemoryProvider> {
        self.providers.first().map(|p| p.as_ref())
    }

    /// Check if any provider is available.
    pub fn has_provider(&self) -> bool {
        self.providers.iter().any(|p| p.is_available())
    }

    /// Number of registered providers.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Provider names for diagnostics.
    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.name()).collect()
    }

    // ── Memory Entries ──

    /// Insert a memory entry into the primary provider.
    pub fn insert(&self, entry: &MemoryEntry) -> Result<()> {
        match self.primary() {
            Some(p) => p.insert(entry),
            None => Err("No memory provider registered".into()),
        }
    }

    /// Get a memory entry from the primary provider.
    pub fn get(&self, id: &str) -> Result<Option<MemoryEntry>> {
        match self.primary() {
            Some(p) => p.get(id),
            None => Ok(None),
        }
    }

    /// Delete a memory entry from all providers.
    pub fn delete(&self, id: &str) -> Result<()> {
        let mut last_err = None;
        for provider in &self.providers {
            if let Err(e) = provider.delete(id) {
                last_err = Some(e);
            }
        }
        match last_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// List entries for a tenant from the primary provider.
    pub fn list_by_tenant(
        &self,
        tenant_id: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        match self.primary() {
            Some(p) => p.list_by_tenant(tenant_id, session_id, limit),
            None => Ok(Vec::new()),
        }
    }

    /// Get entries with embeddings.
    pub fn entries_with_embeddings(&self, tenant_id: &str) -> Result<Vec<(MemoryEntry, Vec<f32>)>> {
        match self.primary() {
            Some(p) => p.entries_with_embeddings(tenant_id),
            None => Ok(Vec::new()),
        }
    }

    /// Count entries for a tenant.
    pub fn count(&self, tenant_id: &str) -> Result<usize> {
        match self.primary() {
            Some(p) => p.count(tenant_id),
            None => Ok(0),
        }
    }

    /// Clear all entries for a tenant from all providers.
    pub fn clear_tenant(&self, tenant_id: &str) -> Result<()> {
        let mut last_err = None;
        for provider in &self.providers {
            if let Err(e) = provider.clear_tenant(tenant_id) {
                last_err = Some(e);
            }
        }
        match last_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    // ── Conversation Summaries ──

    /// Save a conversation summary.
    pub fn save_summary(&self, summary: &ConversationSummary) -> Result<()> {
        match self.primary() {
            Some(p) => p.save_summary(summary),
            None => Err("No memory provider registered".into()),
        }
    }

    /// Get a conversation summary.
    pub fn get_summary(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<Option<ConversationSummary>> {
        match self.primary() {
            Some(p) => p.get_summary(tenant_id, session_id),
            None => Ok(None),
        }
    }

    // ── Rolling Summaries ──

    /// Save a rolling summary.
    pub fn save_rolling_summary(&self, rs: &RollingSummary) -> Result<()> {
        match self.primary() {
            Some(p) => p.save_rolling_summary(rs),
            None => Err("No memory provider registered".into()),
        }
    }

    /// Get a rolling summary.
    pub fn get_rolling_summary(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<Option<RollingSummary>> {
        match self.primary() {
            Some(p) => p.get_rolling_summary(tenant_id, session_id),
            None => Ok(None),
        }
    }

    /// Delete a rolling summary.
    pub fn delete_rolling_summary(&self, tenant_id: &str, session_id: &str) -> Result<()> {
        let mut last_err = None;
        for provider in &self.providers {
            if let Err(e) = provider.delete_rolling_summary(tenant_id, session_id) {
                last_err = Some(e);
            }
        }
        match last_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
