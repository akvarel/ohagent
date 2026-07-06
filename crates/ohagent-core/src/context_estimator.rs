//! Context estimator — counts tokens in a multi-turn conversation.
//!
//! Each model has a different context window, and the router needs to know
//! whether a conversation will fit before selecting a model. This module
//! provides a fast, approximate token counter (no external tokenizer needed).
//!
//! ## Algorithm
//!
//! Rule of thumb: ~1.3 tokens per English word, ~0.5 per non-English character.
//! This is intentionally simple — expensive models are *excluded* if the estimate
//! exceeds their window, which is the safe side of the fence.

use jcode_message_types::{ContentBlock, Message};

/// Estimate token count for a single message.
pub fn estimate_message_tokens(msg: &Message) -> u32 {
    let mut tokens: u32 = 0;

    for block in &msg.content {
        match block {
            ContentBlock::Text { text, .. } => {
                // ~1.3 tokens per word for English text
                let word_count = text.split_whitespace().count() as u32;
                let char_count = text.chars().count() as u32;
                // Conservative: use max of word-based and char-based estimates
                let word_estimate = (word_count as f64 * 1.3) as u32;
                let char_estimate = (char_count as f64 * 0.25) as u32;
                tokens += word_estimate.max(char_estimate).max(1);
            }
            ContentBlock::Image { .. } => {
                // Images: ~85 tokens for low-res, ~170 for high-res (OpenAI-style)
                tokens += 85;
            }
            ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. } => {
                // Tool calls are roughly 50-200 tokens
                tokens += 100;
            }
            _ => {
                tokens += 50; // Unknown block — conservative
            }
        }
    }

    tokens.max(1)
}

/// Estimate total token count for a conversation (messages + system prompt).
pub fn estimate_conversation_tokens(messages: &[Message], system_prompt: &str) -> u32 {
    let msg_tokens: u32 = messages.iter().map(estimate_message_tokens).sum();
    let system_tokens = system_prompt.split_whitespace().count() as u32;
    // Add overhead for message formatting (~3 tokens per message for role markers)
    let overhead = messages.len() as u32 * 3;
    msg_tokens + system_tokens + overhead
}

/// Check if a conversation fits in a model's context window.
/// Returns true if it fits (with 20% safety margin).
pub fn fits_context_window(
    messages: &[Message],
    system_prompt: &str,
    context_window: u32,
) -> bool {
    let estimated = estimate_conversation_tokens(messages, system_prompt);
    let safe_window = (context_window as f64 * 0.80) as u32; // 20% safety margin
    estimated <= safe_window
}

/// Filter a list of model context windows to only those that can fit.
/// Returns (model_id, context_window) pairs.
pub fn filter_by_context<'a>(
    candidates: &[(&'a str, u32)],
    messages: &[Message],
    system_prompt: &str,
) -> Vec<&'a str> {
    candidates
        .iter()
        .filter(|(_, window)| fits_context_window(messages, system_prompt, *window))
        .map(|(id, _)| *id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_message_types::Role;

    fn make_msg(text: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
            timestamp: None,
            tool_duration_ms: None,
        }
    }

    #[test]
    fn test_estimate_short() {
        let msg = make_msg("hello world");
        let tokens = estimate_message_tokens(&msg);
        assert!(tokens > 0 && tokens < 10);
    }

    #[test]
    fn test_estimate_long() {
        let msg = make_msg(&"test ".repeat(1000));
        let tokens = estimate_message_tokens(&msg);
        assert!(tokens > 500, "expected >500, got {tokens}");
    }

    #[test]
    fn test_fits_1m_context() {
        let msgs: Vec<Message> = (0..100)
            .map(|i| make_msg(&format!("message number {}", i)))
            .collect();
        assert!(fits_context_window(&msgs, "you are helpful", 1_000_000));
    }

    #[test]
    fn test_does_not_fit_small_context() {
        let msgs: Vec<Message> = (0..500)
            .map(|i| make_msg(&format!("this is a somewhat long message that says {}", i)))
            .collect();
        // Should NOT fit in 4000 token window
        assert!(!fits_context_window(&msgs, "system", 4_000));
    }

    #[test]
    fn test_filter_by_context() {
        // 40 repeats of "this is a very long message " = 240 words ≈ 312 tokens.
        // With system prompt + overhead: ~318 tokens.
        let big_msg = "this is a very long message ".repeat(40);
        let msgs = vec![make_msg(&big_msg)];
        let candidates = vec![
            ("gpt-4o", 128_000),
            ("deepseek-v4-flash", 1_000_000),
            ("tiny-model", 4_000), // 40 * 7 words * 1.3 ≈ 364 tokens; with system ≈ 367. Usable: 3200. Fits.
            ("nano-model", 200),  // 200 * 0.8 = 160 tokens. won't fit.
        ];
        let fitting = filter_by_context(&candidates, &msgs, "system prompt here");
        assert!(fitting.contains(&"deepseek-v4-flash"));
        assert!(fitting.contains(&"gpt-4o"));
        assert!(fitting.contains(&"tiny-model"));
        assert!(!fitting.contains(&"nano-model"));
    }
}
