//! Proactive memory nudges.
//!
//! Periodically checks recent activity and suggests relevant memories
//! that could help the agent. Nudges are injected into the agent's context.

use chrono::Utc;
use tracing::{debug, info};

use crate::models::{MemoryConfig, MemoryEntry, MemoryNudge, MemorySource, SearchResult};
use crate::retrieval;
use crate::store::MemoryStore;
use crate::Result;

/// Generate proactive nudges for a tenant based on recent topics.
///
/// 1. Get recent memory entries as context clues
/// 2. Search for semantically related older memories
/// 3. Build nudge text from the most relevant matches
pub fn generate_nudges(
    store: &MemoryStore,
    tenant_id: &str,
    config: &MemoryConfig,
) -> Result<Vec<MemoryNudge>> {
    if !config.nudges_enabled {
        return Ok(Vec::new());
    }

    debug!(tenant_id = %tenant_id, "Generating nudges");

    // Get recent entries as context
    let recent = retrieval::recent(store, tenant_id, 5)?;

    if recent.is_empty() {
        return Ok(Vec::new());
    }

    // Build a query from recent topics
    let query = recent
        .iter()
        .map(|e| e.content.chars().take(200).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ");

    // Search for relevant older memories
    let results = retrieval::search(store, tenant_id, &query, config)?;

    // Filter out very recent entries (same session)
    let recent_session_ids: Vec<&str> = recent.iter().map(|e| e.session_id.as_str()).collect();

    let relevant: Vec<&SearchResult> = results
        .iter()
        .filter(|r| {
            !recent_session_ids.contains(&r.entry.session_id.as_str())
                && r.combined_score > 0.5
        })
        .collect();

    if relevant.is_empty() {
        return Ok(Vec::new());
    }

    let nudge = MemoryNudge {
        content: build_nudge_text(&relevant),
        source_entries: relevant.iter().map(|r| r.entry.clone()).collect(),
        confidence: relevant.iter().map(|r| r.combined_score).fold(0.0f32, f32::max),
        urgent: relevant.iter().any(|r| r.entry.importance > 0.8),
    };

    info!(
        tenant_id = %tenant_id,
        entries = relevant.len(),
        confidence = nudge.confidence,
        "Nudge generated"
    );

    Ok(vec![nudge])
}

/// Build a concise nudge text from matching entries.
fn build_nudge_text(results: &[&SearchResult]) -> String {
    if results.len() == 1 {
        return format!(
            "🔔 Memory: \"{}\" (may be relevant)",
            results[0].entry.content.chars().take(200).collect::<String>(),
        );
    }

    let mut text = String::from("🔔 Related past context:\n");
    for (i, r) in results.iter().take(3).enumerate() {
        let snippet: String = r.entry.content.chars().take(100).collect();
        text.push_str(&format!("  {}. {snippet}\n", i + 1));
    }
    text
}

/// Store a nudge as a memory entry for tracking.
pub fn record_nudge(store: &MemoryStore, nudge: &MemoryNudge, tenant_id: &str) -> Result<()> {
    let entry = MemoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        tenant_id: tenant_id.to_string(),
        session_id: String::new(),
        content: nudge.content.clone(),
        source: MemorySource::Nudge,
        importance: 0.3,
        tags: vec!["nudge".to_string()],
        embedding: None,
        created_at: Utc::now(),
        last_accessed_at: Utc::now(),
        access_count: 0,
    };
    store.insert(&entry)
}
