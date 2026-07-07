//! Pre-classifier — fast, cheap document counting before routing.
//!
//! Uses GLM-4.6V-flash (FREE tier at Z.ai) to count documents in an image.
//! Takes ~3 seconds. Result determines which model gets the full OCR job:
//! - 1 document → cheapest vision model (Scaleway Mistral-small, €0.0005)
//! - 2+ documents → multi_doc model (GLM-4.6V-flashx, €0.002)
//!
//! Architecture:
//! ```
//! photo → PreClassifier::count_docs() → DocumentCount::Multiple(4)
//!                ↓                                 ↓
//!         "How many documents?"            Router filters to multi_doc models
//!         (FREE, ~3s, ~200 tokens)         → GLM-4.6V-flashx
//! ```

use crate::models::DocumentCount;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::Client;
use serde_json::Value;

/// Configuration for the pre-classifier.
pub struct PreClassifierConfig {
    /// API key for Z.ai (same as glm-4.6v-flash)
    pub zai_api_key: String,
    /// Timeout for the pre-classification call (default 10s)
    pub timeout_secs: u64,
    /// Maximum tokens for the response (default 10 — we only need a number)
    pub max_tokens: u32,
}

impl Default for PreClassifierConfig {
    fn default() -> Self {
        Self {
            zai_api_key: String::new(),
            timeout_secs: 10,
            max_tokens: 10,
        }
    }
}

/// Pre-classifies an image to determine document count before routing.
pub struct PreClassifier {
    client: Client,
    config: PreClassifierConfig,
}

impl PreClassifier {
    pub fn new(config: PreClassifierConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client for pre-classifier");
        Self { client, config }
    }

    /// Count how many distinct documents/receipts/pages are in an image.
    ///
    /// Sends image to GLM-4.6V-flash (FREE) with a minimal prompt.
    /// Returns DocumentCount::Unknown on any error — caller falls back to normal routing.
    pub async fn count_documents(&self, image_bytes: &[u8], mime_type: &str) -> DocumentCount {
        let base64_image = BASE64.encode(image_bytes);
        let data_uri = format!("data:{};base64,{}", mime_type, base64_image);

        let body = serde_json::json!({
            "model": "glm-4.6v-flash",
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": { "url": &data_uri }
                    },
                    {
                        "type": "text",
                        "text": "How many distinct documents, receipts, pages, or separate items are visible in this image? Answer with ONLY a single integer number, nothing else."
                    }
                ]
            }],
            "max_tokens": self.config.max_tokens,
            "temperature": 0.0
        });

        let resp = match self.client
            .post("https://api.z.ai/api/paas/v4/chat/completions")
            .header("Authorization", format!("Bearer {}", self.config.zai_api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Pre-classifier HTTP error: {e}");
                return DocumentCount::Unknown;
            }
        };

        if !resp.status().is_success() {
            log::warn!("Pre-classifier HTTP {}: {:?}", resp.status(), resp.text().await.unwrap_or_default());
            return DocumentCount::Unknown;
        }

        let json: Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Pre-classifier JSON parse error: {e}");
                return DocumentCount::Unknown;
            }
        };

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("");

        let count = Self::parse_count(content);

        match count {
            0 => DocumentCount::Unknown,
            1 => DocumentCount::Single,
            n => DocumentCount::Multiple(n),
        }
    }

    /// Parse an integer from the model's response.
    /// Handles common variations: "4", "4.", "4 documents", "There are 4 receipts."
    fn parse_count(text: &str) -> u8 {
        // First: try to parse the whole trimmed string as a u8
        if let Ok(n) = text.trim().parse::<u8>() {
            return n;
        }
        // Second: find the first integer in the text
        for word in text.split(|c: char| !c.is_ascii_digit()) {
            if let Ok(n) = word.parse::<u8>() {
                return n;
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_count_exact() {
        assert_eq!(PreClassifier::parse_count("4"), 4);
        assert_eq!(PreClassifier::parse_count("1"), 1);
        assert_eq!(PreClassifier::parse_count("  12  "), 12);
    }

    #[test]
    fn test_parse_count_with_period() {
        assert_eq!(PreClassifier::parse_count("4."), 4);
        assert_eq!(PreClassifier::parse_count("1."), 1);
    }

    #[test]
    fn test_parse_count_in_sentence() {
        assert_eq!(PreClassifier::parse_count("There are 4 documents"), 4);
        assert_eq!(PreClassifier::parse_count("I see 3 distinct receipts in this image"), 3);
        assert_eq!(PreClassifier::parse_count("4 receipts total"), 4);
    }

    #[test]
    fn test_parse_count_zero() {
        assert_eq!(PreClassifier::parse_count("0"), 0);
        assert_eq!(PreClassifier::parse_count(""), 0);
        assert_eq!(PreClassifier::parse_count("no documents"), 0);
        assert_eq!(PreClassifier::parse_count("not a number"), 0);
    }
}
