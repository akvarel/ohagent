//! Task classifier.
//!
//! Re-exports `classify_task` from `model_router` for convenience.
//! LLM-based classification (`classify_with_llm`) is planned for future release.

use crate::model_router::Capability;

/// Fast keyword-based task classification.
pub fn classify_task(message: &str) -> Vec<Capability> {
    crate::model_router::classify_task(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delegates_to_model_router() {
        let caps = classify_task("deploy to kubernetes");
        assert!(caps.contains(&Capability::Coding));
    }
}
