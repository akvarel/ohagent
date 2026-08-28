//! Rolling summary — incremental mid-session context compression.
//!
//! Unlike `ConversationSummary` (post-mortem at session end), the rolling
//! summary is updated *during* a session when tokens or iterations cross
//! thresholds. DeepSeek Flash merges new messages into the existing
//! compressed history, avoiding full re-compression.
//!
//! ## Triggers
//!
//! Compression fires when **either** condition is met:
//! - `tokens_since_last_compression >= COMPRESSION_TOKEN_THRESHOLD` (100K)
//! - `iterations_since_last_compression >= COMPRESSION_ITERATION_THRESHOLD` (25)
//!
//! ## Cost model
//!
//! Flash off-peak pricing (DeepSeek Flash with 50% discount UTC 16:30-00:30):
//! - Input: ~50K tokens ($0.0035) — compressed_history (8K) + new messages (~42K)
//! - Output: ~8K tokens ($0.0022)
//! - Total per merge: ~$0.006
//!
//! ## Session granularity
//!
//! Each `(tenant_id, session_id)` pair has its own rolling summary.
//! Swarm child sessions inherit the parent's summary on spawn.

use chrono::Utc;
use tracing::{debug, info};

use crate::models::{RollingSummary, TopicRef};
use crate::store::MemoryStore;
use crate::Result;

/// Token threshold: compress after this many new tokens accumulate.
pub const COMPRESSION_TOKEN_THRESHOLD: u64 = 100_000;

/// Iteration threshold: compress after this many message iterations.
pub const COMPRESSION_ITERATION_THRESHOLD: u32 = 25;

/// Maximum size of compressed_history in tokens to keep merge costs low.
pub const MAX_COMPRESSED_TOKENS: usize = 8_000;

/// Build a merge prompt for Flash to fold new messages into existing summary.
///
/// This is intentionally NOT a full re-compression — Flash merges the *delta*
/// into the existing compressed history, keeping output small.
pub fn build_merge_prompt(
    compressed_history: &str,
    new_messages: &[String],
    existing_topics: &[TopicRef],
) -> String {
    let new_text = new_messages.join("\n---\n");

    let topic_hint = if existing_topics.is_empty() {
        String::new()
    } else {
        let labels: Vec<&str> = existing_topics.iter().map(|t| t.label.as_str()).collect();
        format!(
            "\n\nExisting topic labels (keep these): {}",
            labels.join(", ")
        )
    };

    format!(
        "You are compressing a long conversation to save context space.\n\n\
         COMPRESSED HISTORY (so far):\n{compressed_history}\n\n\
         NEW MESSAGES to merge:\n{new_text}\n\n\
         TASK: Merge the new messages into the compressed history. \
         Keep the result under {MAX_COMPRESSED_TOKENS} tokens.\n\
         Preserve: decisions made, code snippets, specific facts/numbers, \
         key reasoning steps, and topic transitions.\n\
         Discard: greetings, filler, repeated arguments, and anything already captured.{topic_hint}\n\n\
         Output format — respond with ONLY a JSON object (no markdown, no commentary):\n\
         {{\"compressed_history\": \"...\", \"new_topics\": [\"label1\", \"label2\"]}}"
    )
}

/// Check whether compression should fire.
pub fn should_compress(tokens_since_last: u64, iterations_since_last: u32) -> bool {
    tokens_since_last >= COMPRESSION_TOKEN_THRESHOLD
        || iterations_since_last >= COMPRESSION_ITERATION_THRESHOLD
}

/// Parse the Flash JSON response into updated RollingSummary fields.
pub fn parse_merge_response(
    response_json: &str,
    existing: &RollingSummary,
    _new_messages_len: usize,
    new_topics: Vec<TopicRef>,
) -> Option<(String, Vec<TopicRef>)> {
    let parsed: serde_json::Value = serde_json::from_str(response_json).ok()?;
    let compressed = parsed["compressed_history"].as_str()?.to_string();

    // Merge existing topic_index with new_topics; keep at most 20 topics
    let mut merged_topics = existing.topic_index.clone();
    for nt in new_topics {
        if !merged_topics.iter().any(|t| t.label == nt.label) {
            merged_topics.push(nt);
        }
    }
    merged_topics.truncate(20);

    Some((compressed, merged_topics))
}

/// Create a fresh RollingSummary for a new session.
pub fn create_fresh(tenant_id: &str, session_id: &str) -> RollingSummary {
    RollingSummary {
        session_id: session_id.to_string(),
        tenant_id: tenant_id.to_string(),
        compressed_history: String::new(),
        topic_index: Vec::new(),
        tokens_compressed: 0,
        iteration_count: 0,
        last_message_idx: 0,
        last_compressed_at: Utc::now(),
    }
}

/// Perform a merge: save the result to the store and return updated summary.
///
/// Caller is responsible for calling the LLM with `build_merge_prompt` output
/// and passing the parsed result here.
pub fn merge_and_save(
    store: &MemoryStore,
    existing: &RollingSummary,
    compressed_history: String,
    merged_topics: Vec<TopicRef>,
    new_token_count: u64,
    new_message_idx: usize,
) -> Result<RollingSummary> {
    let updated = RollingSummary {
        session_id: existing.session_id.clone(),
        tenant_id: existing.tenant_id.clone(),
        compressed_history,
        topic_index: merged_topics,
        tokens_compressed: existing.tokens_compressed + new_token_count,
        iteration_count: 0, // reset counter
        last_message_idx: new_message_idx,
        last_compressed_at: Utc::now(),
    };

    store.save_rolling_summary(&updated)?;
    info!(
        session_id = %updated.session_id,
        total_compressed = updated.tokens_compressed,
        topics = updated.topic_index.len(),
        "Rolling summary merged and saved"
    );
    Ok(updated)
}

/// Load or create a rolling summary for a session.
pub fn load_or_create(
    store: &MemoryStore,
    tenant_id: &str,
    session_id: &str,
) -> Result<RollingSummary> {
    match store.get_rolling_summary(tenant_id, session_id)? {
        Some(rs) => {
            debug!(session_id = %session_id, tokens_compressed = rs.tokens_compressed, "Rolling summary loaded");
            Ok(rs)
        }
        None => {
            let fresh = create_fresh(tenant_id, session_id);
            store.save_rolling_summary(&fresh)?;
            debug!(session_id = %session_id, "Fresh rolling summary created");
            Ok(fresh)
        }
    }
}

/// Clone a parent session's rolling summary for a child swarm session.
///
/// The child inherits the compressed_history and topic_index but starts
/// with its own counters reset.
pub fn inherit_for_child(
    store: &MemoryStore,
    parent: &RollingSummary,
    child_session_id: &str,
) -> Result<RollingSummary> {
    let child = RollingSummary {
        session_id: child_session_id.to_string(),
        tenant_id: parent.tenant_id.clone(),
        compressed_history: parent.compressed_history.clone(),
        topic_index: parent.topic_index.clone(),
        tokens_compressed: parent.tokens_compressed,
        iteration_count: 0,
        last_message_idx: parent.last_message_idx,
        last_compressed_at: Utc::now(),
    };
    store.save_rolling_summary(&child)?;
    info!(
        parent = %parent.session_id,
        child = %child_session_id,
        "Rolling summary inherited by child session"
    );
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MemoryConfig;

    fn test_store() -> MemoryStore {
        MemoryStore::open(MemoryConfig {
            db_path: ":memory:".to_string(),
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn test_should_compress_token_threshold() {
        assert!(should_compress(100_000, 1));
        assert!(!should_compress(99_999, 1));
    }

    #[test]
    fn test_should_compress_iteration_threshold() {
        assert!(should_compress(1, 25));
        assert!(!should_compress(1, 24));
    }

    #[test]
    fn test_create_and_load() {
        let store = test_store();
        let rs = create_fresh("t1", "s1");
        store.save_rolling_summary(&rs).unwrap();

        let loaded = store.get_rolling_summary("t1", "s1").unwrap().unwrap();
        assert_eq!(loaded.session_id, "s1");
        assert_eq!(loaded.tenant_id, "t1");
        assert_eq!(loaded.tokens_compressed, 0);
    }

    #[test]
    fn test_load_or_create_new() {
        let store = test_store();
        let rs = load_or_create(&store, "t2", "s2").unwrap();
        assert_eq!(rs.session_id, "s2");
        assert_eq!(rs.compressed_history, "");
    }

    #[test]
    fn test_load_or_create_existing() {
        let store = test_store();
        let rs = create_fresh("t3", "s3");
        store.save_rolling_summary(&rs).unwrap();

        let loaded = load_or_create(&store, "t3", "s3").unwrap();
        assert_eq!(loaded.session_id, "s3");
    }

    #[test]
    fn test_merge_and_save() {
        let store = test_store();
        let existing = create_fresh("t4", "s4");
        store.save_rolling_summary(&existing).unwrap();

        let topics = vec![TopicRef {
            label: "auth".into(),
            message_ids: vec!["m1".into(), "m2".into()],
            token_estimate: 500,
        }];

        let updated = merge_and_save(
            &store,
            &existing,
            "compressed: user wants OAuth2".into(),
            topics,
            50_000,
            10,
        )
        .unwrap();

        assert_eq!(updated.tokens_compressed, 50_000);
        assert_eq!(updated.last_message_idx, 10);
        assert_eq!(updated.iteration_count, 0); // reset
        assert_eq!(updated.topic_index.len(), 1);
    }

    #[test]
    fn test_inherit_for_child() {
        let store = test_store();
        let parent = create_fresh("t5", "parent-s1");
        store.save_rolling_summary(&parent).unwrap();

        // Simulate some compression happened on parent
        let parent = merge_and_save(
            &store,
            &parent,
            "parent history".into(),
            vec![],
            100_000,
            50,
        )
        .unwrap();

        let child = inherit_for_child(&store, &parent, "child-s1").unwrap();
        assert_eq!(child.compressed_history, "parent history");
        assert_eq!(child.tokens_compressed, 100_000);
        assert_eq!(child.iteration_count, 0); // reset
        assert_eq!(child.session_id, "child-s1");
        assert_eq!(child.tenant_id, "t5");
    }

    #[test]
    fn test_parse_merge_response() {
        let existing = create_fresh("t1", "s1");
        let json = r#"{"compressed_history": "summarized text", "new_topics": ["topic-a"]}"#;
        let new_topics = vec![TopicRef {
            label: "topic-a".into(),
            message_ids: vec!["m5".into()],
            token_estimate: 300,
        }];

        let (compressed, topics) = parse_merge_response(json, &existing, 5, new_topics).unwrap();
        assert_eq!(compressed, "summarized text");
        assert!(topics.iter().any(|t| t.label == "topic-a"));
    }

    #[test]
    fn test_build_merge_prompt_includes_topics() {
        let topics = vec![TopicRef {
            label: "auth".into(),
            message_ids: vec!["m1".into()],
            token_estimate: 100,
        }];
        let prompt = build_merge_prompt("old", &["new msg".into()], &topics);
        assert!(prompt.contains("auth"));
        assert!(prompt.contains("new msg"));
        assert!(prompt.contains("old"));
    }

    #[test]
    fn test_delete_rolling_summary() {
        let store = test_store();
        let rs = create_fresh("t6", "s6");
        store.save_rolling_summary(&rs).unwrap();
        assert!(store.get_rolling_summary("t6", "s6").unwrap().is_some());

        store.delete_rolling_summary("t6", "s6").unwrap();
        assert!(store.get_rolling_summary("t6", "s6").unwrap().is_none());
    }
}
