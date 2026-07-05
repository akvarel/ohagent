//! Conversation summarizer — generates summaries of completed agent sessions.
//!
//! Uses the JcodeBridge to call the LLM for summarization.
//! Stores summaries as structured memory entries.

use chrono::Utc;
use tracing::{debug, info};
use uuid::Uuid;

use crate::models::{ConversationSummary, MemoryEntry, MemorySource};
use crate::store::MemoryStore;
use crate::Result;

/// Summarize a conversation and store it as a memory entry.
///
/// The summary is both saved in the conversation_summaries table
/// *and* inserted as a MemoryEntry for future retrieval.
pub fn summarize_conversation(
    store: &MemoryStore,
    summary: &ConversationSummary,
) -> Result<MemoryEntry> {
    info!(
        session_id = %summary.session_id,
        tenant_id = %summary.tenant_id,
        "Summarizing conversation"
    );

    // Save structured summary
    store.save_summary(summary)?;

    // Create a memory entry from the summary
    let mut tags = vec!["summary".to_string(), "conversation".to_string()];
    tags.extend(summary.key_points.iter().map(|kp| {
        kp.chars().take(30).collect::<String>()
    }));

    let entry = MemoryEntry {
        id: Uuid::new_v4().to_string(),
        tenant_id: summary.tenant_id.clone(),
        session_id: summary.session_id.clone(),
        content: format_summary_text(summary),
        source: MemorySource::Conversation,
        importance: estimate_importance(summary),
        tags,
        embedding: None, // Will be embedded later
        created_at: Utc::now(),
        last_accessed_at: Utc::now(),
        access_count: 0,
    };

    store.insert(&entry)?;
    debug!(id = %entry.id, "Summary stored as memory entry");

    Ok(entry)
}

/// Generate a summary prompt for the LLM.
///
/// This function builds the prompt that should be sent to the agent's LLM
/// to generate a summary. The caller should use JcodeBridge to send this.
pub fn build_summary_prompt(messages: &[String]) -> String {
    let conversation = messages.join("\n");
    format!(
        "Summarize this conversation in 1-3 paragraphs. \
         Then list key points (3-5 bullet points). \
         Then list any decisions made.\n\n\
         Conversation:\n{conversation}\n\n\
         Format your response as JSON:\n\
         {{\"summary\": \"...\", \"key_points\": [\"...\"], \"decisions\": [\"...\"]}}"
    )
}

/// Format a ConversationSummary into a text content string for a MemoryEntry.
fn format_summary_text(summary: &ConversationSummary) -> String {
    let mut text = format!("## Conversation Summary\n\n{}\n", summary.summary);

    if !summary.key_points.is_empty() {
        text.push_str("\n### Key Points\n");
        for point in &summary.key_points {
            text.push_str(&format!("- {point}\n"));
        }
    }

    if !summary.decisions.is_empty() {
        text.push_str("\n### Decisions\n");
        for decision in &summary.decisions {
            text.push_str(&format!("- {decision}\n"));
        }
    }

    text
}

/// Estimate importance from summary metadata.
fn estimate_importance(summary: &ConversationSummary) -> f32 {
    let mut importance: f32 = 0.5;

    // Longer messages → more important
    if summary.message_count > 10 {
        importance += 0.1;
    }
    if summary.message_count > 30 {
        importance += 0.15;
    }

    // Decisions made → important
    if !summary.decisions.is_empty() {
        importance += 0.2;
    }

    // Key points → somewhat important
    if summary.key_points.len() >= 3 {
        importance += 0.1;
    }

    importance.min(1.0).max(0.1)
}
