//! Context compressor — triggers rolling summary merges during long sessions.
//!
//! Tracks accumulated tokens and iteration counts per session. When thresholds
//! are crossed, fires a background merge via DeepSeek Flash to keep the
//! compressed history fresh.

use std::sync::Arc;
use chrono::Utc;
use jcode_message_types::{ContentBlock, Message, StreamEvent};
use jcode_provider_core::Provider;
use ohagent_core::context_estimator;
use ohagent_memory::engine::MemoryEngine;
use ohagent_memory::models::{RollingSummary, TopicRef};
use ohagent_memory::rolling_summary::{
    self, should_compress, build_merge_prompt, parse_merge_response,
    merge_and_save, COMPRESSION_TOKEN_THRESHOLD, COMPRESSION_ITERATION_THRESHOLD,
};
use tracing::{debug, info, warn};

/// Tracks rolling summary state for a single session.
///
/// Usage:
/// 1. Create with `RollingSummaryTracker::new(memory, flash_provider, tenant_id, session_id)`
/// 2. Call `track_messages(&messages)` after each exchange
/// 3. If `should_compress()` returns true, call `compress(&messages).await`
/// 4. On route to small model, call `compressed_history()` to get injectable prompt
pub struct RollingSummaryTracker {
    memory: Arc<MemoryEngine>,
    flash_provider: Arc<dyn Provider>,
    tenant_id: String,
    session_id: String,
    summary: RollingSummary,
    tokens_since_last: u64,
    iterations_since_last: u32,
}

impl RollingSummaryTracker {
    /// Create a new tracker, loading existing summary from memory or creating fresh.
    pub fn new(
        memory: Arc<MemoryEngine>,
        flash_provider: Arc<dyn Provider>,
        tenant_id: String,
        session_id: String,
    ) -> ohagent_memory::Result<Self> {
        let summary = memory.get_rolling_summary(&tenant_id, &session_id)?;
        let iter_count = summary.iteration_count;
        info!(
            session_id = %session_id,
            tokens_compressed = summary.tokens_compressed,
            "RollingSummaryTracker initialized"
        );
        Ok(Self {
            memory,
            flash_provider,
            tenant_id,
            session_id,
            summary,
            tokens_since_last: 0,
            iterations_since_last: iter_count,
        })
    }

    /// Track a batch of messages — accumulates tokens + iteration count.
    pub fn track_messages(&mut self, messages: &[Message]) {
        let tokens: u64 = messages
            .iter()
            .map(|m| context_estimator::estimate_message_tokens(m) as u64)
            .sum();
        self.tokens_since_last += tokens;
        self.iterations_since_last += messages.len() as u32;
    }

    /// Check whether compression should fire.
    pub fn needs_compression(&self) -> bool {
        should_compress(self.tokens_since_last, self.iterations_since_last)
    }

    /// Get the compressed history for injection into a system prompt.
    pub fn compressed_history(&self) -> Option<&str> {
        if self.summary.compressed_history.is_empty() {
            None
        } else {
            Some(&self.summary.compressed_history)
        }
    }

    /// Get the full RollingSummary for advanced use (RAG recovery, etc).
    pub fn summary(&self) -> &RollingSummary {
        &self.summary
    }

    /// Fire compression: extract new messages, ask Flash to merge, save.
    pub async fn compress(
        &mut self,
        all_messages: &[Message],
        system_prompt: &str,
    ) -> Result<(), String> {
        let new_messages = &all_messages[self.summary.last_message_idx..];
        if new_messages.is_empty() {
            debug!("No new messages to compress");
            return Ok(());
        }

        let new_texts: Vec<String> = new_messages
            .iter()
            .map(|m| {
                m.content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();

        let prompt = build_merge_prompt(
            &self.summary.compressed_history,
            &new_texts,
            &self.summary.topic_index,
        );

        let fake_messages = vec![Message {
            role: jcode_message_types::Role::User,
            content: vec![ContentBlock::Text {
                text: prompt,
                cache_control: None,
            }],
            timestamp: None,
            tool_duration_ms: None,
        }];

        info!(
            session_id = %self.session_id,
            new_msgs = new_messages.len(),
            tokens_acc = self.tokens_since_last,
            "Firing rolling summary compression via Flash"
        );

        // Call Flash
        let existing = self.summary.clone();
        match self.flash_provider.complete(&fake_messages, &[], system_prompt, None).await {
            Ok(mut stream) => {
                use futures::StreamExt;
                let mut response = String::new();
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(StreamEvent::TextDelta(text)) => {
                            response.push_str(&text);
                        }
                        Err(e) => {
                            warn!(error = %e, "Flash stream error during compression");
                        }
                        _ => {}
                    }
                }

                // Parse Flash response
                let new_topics: Vec<TopicRef> = Vec::new(); // Flash may return topics, parse them later
                let (compressed, merged_topics) = parse_merge_response(
                    &response,
                    &existing,
                    new_messages.len(),
                    new_topics,
                )
                .ok_or_else(|| {
                    format!(
                        "Flash returned unparseable merge response: {}",
                        &response[..response.len().min(200)]
                    )
                })?;

                let new_token_count = new_texts.iter().map(|t| t.len() as u64).sum::<u64>() / 4;
                let new_idx = all_messages.len();

                let updated = merge_and_save(
                    self.memory.store(),
                    &existing,
                    compressed,
                    merged_topics,
                    new_token_count,
                    new_idx,
                )
                .map_err(|e| format!("Failed to save rolling summary: {e}"))?;

                self.summary = updated;
                self.tokens_since_last = 0;
                self.iterations_since_last = 0;

                Ok(())
            }
            Err(e) => {
                warn!(error = %e, "Flash compression call failed");
                Err(format!("Flash compression call failed: {e}"))
            }
        }
    }

    /// Persist current state to memory (call on session end).
    pub fn flush(&self) -> ohagent_memory::Result<()> {
        self.memory.save_rolling_summary(&self.summary)
    }

    /// Stats for monitoring.
    pub fn stats(&self) -> RollingSummaryStats {
        RollingSummaryStats {
            tokens_compressed_total: self.summary.tokens_compressed,
            tokens_since_last: self.tokens_since_last,
            iterations_since_last: self.iterations_since_last,
            topics: self.summary.topic_index.len(),
            last_compressed_at: self.summary.last_compressed_at,
        }
    }
}

/// Lightweight stats for monitoring/logging.
#[derive(Debug, Clone)]
pub struct RollingSummaryStats {
    pub tokens_compressed_total: u64,
    pub tokens_since_last: u64,
    pub iterations_since_last: u32,
    pub topics: usize,
    pub last_compressed_at: chrono::DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threshold_constants() {
        assert_eq!(COMPRESSION_TOKEN_THRESHOLD, 100_000);
        assert_eq!(COMPRESSION_ITERATION_THRESHOLD, 25);
    }

    #[test]
    fn test_needs_compression_edge_cases() {
        // Both below
        assert!(!should_compress(99_999, 24));
        // Token threshold
        assert!(should_compress(100_000, 1));
        // Iteration threshold
        assert!(should_compress(1, 25));
        // Both above
        assert!(should_compress(200_000, 50));
    }
}
