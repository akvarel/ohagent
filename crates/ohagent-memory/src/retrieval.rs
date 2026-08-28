//! Memory retrieval — semantic + temporal search.
//!
//! Combines vector similarity (semantic) with recency and importance
//! to produce ranked search results.

use chrono::Utc;
use std::sync::Arc;
use tracing::debug;

use crate::embeddings::{embed_text, find_similar};
use crate::models::{MemoryConfig, MemoryEntry, SearchResult};
use crate::store::MemoryStore;
use crate::Result;

/// Retrieve relevant memories for a query.
///
/// Pipeline:
/// 1. Generate query embedding
/// 2. Load all tenant entries with embeddings
/// 3. Compute cosine similarity
/// 4. Boost by recency and importance
/// 5. Return top-K results
pub fn search(
    store: &MemoryStore,
    tenant_id: &str,
    query: &str,
    config: &MemoryConfig,
) -> Result<Vec<SearchResult>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    // Skip vector search if embeddings are disabled
    if config.skip_embeddings {
        return text_search(store, tenant_id, query, config);
    }

    // Step 1: Embed the query
    let query_emb = match embed_text(query) {
        Ok(emb) => emb,
        Err(_) => {
            return text_search(store, tenant_id, query, config);
        }
    };

    // Step 2: Load candidate entries with embeddings
    let candidates: Vec<Arc<(MemoryEntry, Vec<f32>)>> = store
        .entries_with_embeddings(tenant_id)?
        .into_iter()
        .map(|(entry, emb)| Arc::new((entry, emb)))
        .collect();

    if candidates.is_empty() {
        debug!(tenant_id = %tenant_id, "No embedded entries for tenant");
        return text_search(store, tenant_id, query, config);
    }

    // Step 3: Semantic search
    let semantic_hits = find_similar(
        &query_emb,
        &candidates,
        config.max_retrieval_results * 2, // Over-fetch for re-ranking
        config.similarity_threshold,
    );

    debug!(
        tenant_id = %tenant_id,
        candidates = candidates.len(),
        hits = semantic_hits.len(),
        "Semantic search"
    );

    // Step 4: Combine scores with recency and importance
    let now = Utc::now();
    let mut results: Vec<SearchResult> = semantic_hits
        .into_iter()
        .map(|(entry, semantic_score)| {
            let hours_old = (now - entry.created_at).num_hours().max(0) as f32;
            let recency_score = if config.max_entry_age_hours > 0 {
                (1.0 - hours_old / config.max_entry_age_hours as f32).max(0.0)
            } else {
                1.0
            };
            let importance = entry.importance;
            let combined_score = (1.0 - config.recency_weight - config.importance_weight)
                * semantic_score
                + config.recency_weight * recency_score
                + config.importance_weight * importance;

            SearchResult {
                entry,
                semantic_score,
                recency_score,
                combined_score,
            }
        })
        .collect();

    // Sort by combined score descending
    results.sort_by(|a, b| {
        b.combined_score
            .partial_cmp(&a.combined_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(config.max_retrieval_results);

    debug!(results = results.len(), "Final retrieval results");
    Ok(results)
}

/// Fallback text search using SQLite LIKE.
fn text_search(
    store: &MemoryStore,
    tenant_id: &str,
    query: &str,
    config: &MemoryConfig,
) -> Result<Vec<SearchResult>> {
    // Load all entries and do a simple keyword match
    let entries = store.list_by_tenant(tenant_id, None, 100)?;
    let query_lower = query.to_lowercase();
    let terms: Vec<&str> = query_lower.split_whitespace().collect();

    let mut results: Vec<SearchResult> = entries
        .into_iter()
        .filter(|e| {
            let content_lower = e.content.to_lowercase();
            terms.iter().any(|t| content_lower.contains(t))
        })
        .map(|entry| {
            let hours_old = (Utc::now() - entry.created_at).num_hours().max(0) as f32;
            let recency_score = if config.max_entry_age_hours > 0 {
                (1.0 - hours_old / config.max_entry_age_hours as f32).max(0.0)
            } else {
                1.0
            };
            SearchResult {
                entry,
                semantic_score: 0.5, // nominal text-match score
                recency_score,
                combined_score: 0.5 * (1.0 - config.recency_weight)
                    + config.recency_weight * recency_score,
            }
        })
        .collect();

    results.sort_by(|a, b| {
        b.combined_score
            .partial_cmp(&a.combined_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(config.max_retrieval_results);

    debug!(
        results = results.len(),
        query = %query,
        "Text search fallback"
    );
    Ok(results)
}

/// Retrieve recent memories (most recent N) for a tenant.
pub fn recent(store: &MemoryStore, tenant_id: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
    store.list_by_tenant(tenant_id, None, limit)
}
