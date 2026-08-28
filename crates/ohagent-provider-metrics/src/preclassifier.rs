//! Pre-classifier — fast, cheap document counting before routing.
//!
//! Multi-provider fallback strategy (based on real benchmark: 16 models, Jul 7 2026):
//! 1. Scaleway Mistral-small — 0.7s, €0.00017, 100% accurate (EU, no rate limit)
//! 2. GLM-4.6V-flashx —      2.7s, €0.00014, 100% accurate (needs thinking=disabled)
//! 3. GPT-4o-mini —          2.4s, €0.00352, 100% accurate (expensive)
//!
//! Architecture:
//! ```
//! photo → PreClassifier::count_documents() → DocumentCount::Multiple(4)
//!           ↓ first: Scaleway (0.7s, €0.00017)
//!           ↓ fallback: GLM-4.6V (2.7s, €0.00014)
//!           ↓ last-resort: GPT-4o-mini (2.4s, €0.00352)
//!           → Router filters to multi_doc models → GLM-4.6V-flashx
//! ```
//!
//! Key lessons from the benchmark:
//! - GLM-4.6V models need `thinking: disabled` + `max_tokens ≥ 100` to produce output.
//!   With `max_tokens: 10` they consume all budget on thinking tokens → empty answer.
//! - GLM-4.6V-flash (FREE) is permanently rate-limited (429) — useless.
//! - 7/16 VLMs returned empty content despite having vision — can't count documents.

use crate::models::DocumentCount;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use reqwest::Client;
use serde_json::Value;

/// Configuration for the pre-classifier.
pub struct PreClassifierConfig {
    /// API key for Scaleway (Mistral-small — primary classifier)
    pub scaleway_api_key: String,
    /// Scaleway project ID
    pub scaleway_project_id: Option<String>,
    /// API key for Z.ai (GLM-4.6V-flashx — fallback)
    pub zai_api_key: String,
    /// API key for OpenAI (GPT-4o-mini — last resort)
    pub openai_api_key: String,
    /// Timeout for each attempt (default 10s)
    pub timeout_secs: u64,
    /// Max tokens for "how many documents?" — default 100 (GLM needs this for thinking)
    pub max_tokens: u32,
}

impl Default for PreClassifierConfig {
    fn default() -> Self {
        Self {
            scaleway_api_key: String::new(),
            scaleway_project_id: None,
            zai_api_key: String::new(),
            openai_api_key: String::new(),
            timeout_secs: 10,
            max_tokens: 100,
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
    /// Multi-provider fallback:
    /// 1. Scaleway Mistral-small (0.7s, €0.00017)
    /// 2. GLM-4.6V-flashx with thinking=disabled (2.7s, €0.00014)
    /// 3. GPT-4o-mini (2.4s, €0.00352)
    ///
    /// Returns DocumentCount::Unknown on all failures.
    pub async fn count_documents(&self, image_bytes: &[u8], mime_type: &str) -> DocumentCount {
        // Try providers in order
        let results = [
            self.try_scaleway(image_bytes, mime_type).await,
            self.try_zai(image_bytes, mime_type).await,
            self.try_openai(image_bytes, mime_type).await,
        ];

        for count in results {
            if !matches!(count, DocumentCount::Unknown) {
                return count;
            }
        }

        DocumentCount::Unknown
    }

    async fn try_scaleway(&self, image_bytes: &[u8], mime_type: &str) -> DocumentCount {
        let project_id = match &self.config.scaleway_project_id {
            Some(id) if !id.is_empty() => id,
            _ => return DocumentCount::Unknown,
        };
        let base = format!("https://api.scaleway.ai/{project_id}/v1");

        self.call_count(
            &base,
            &self.config.scaleway_api_key,
            "mistral-small-3.2-24b-instruct-2506",
            image_bytes,
            mime_type,
            false, // Scaleway doesn't have thinking mode
        )
        .await
    }

    async fn try_zai(&self, image_bytes: &[u8], mime_type: &str) -> DocumentCount {
        self.call_count_with_thinking_disabled(
            "https://api.z.ai/api/paas/v4",
            &self.config.zai_api_key,
            "glm-4.6v-flashx",
            image_bytes,
            mime_type,
        )
        .await
    }

    async fn try_openai(&self, image_bytes: &[u8], mime_type: &str) -> DocumentCount {
        self.call_count(
            "https://api.openai.com/v1",
            &self.config.openai_api_key,
            "gpt-4o-mini",
            image_bytes,
            mime_type,
            false,
        )
        .await
    }

    /// Standard OpenAI-compatible call (no thinking).
    async fn call_count(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        image_bytes: &[u8],
        mime_type: &str,
        _disable_thinking: bool,
    ) -> DocumentCount {
        let base64_image = BASE64.encode(image_bytes);
        let data_uri = format!("data:{mime_type};base64,{base64_image}");

        let body = serde_json::json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": &data_uri}},
                    {"type": "text", "text": "How many distinct documents, receipts, pages, or separate items are visible in this image? Answer with ONLY a single integer number, nothing else."}
                ]
            }],
            "max_tokens": self.config.max_tokens,
            "temperature": 0.0
        });

        let resp = match self
            .client
            .post(format!("{base_url}/chat/completions"))
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Pre-classifier {model}: HTTP error: {e}");
                return DocumentCount::Unknown;
            }
        };

        if !resp.status().is_success() {
            log::warn!(
                "Pre-classifier {model}: HTTP {}: {:?}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
            return DocumentCount::Unknown;
        }

        let json: Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Pre-classifier {model}: JSON parse error: {e}");
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

    /// Z.ai-specific call — disables thinking to prevent max_tokens starvation.
    /// GLM-4.6V models use thinking tokens from the max_tokens budget.
    /// With max_tokens=10 and thinking=enabled, all 10 tokens go to thinking → empty answer.
    async fn call_count_with_thinking_disabled(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        image_bytes: &[u8],
        mime_type: &str,
    ) -> DocumentCount {
        let base64_image = BASE64.encode(image_bytes);
        let data_uri = format!("data:{mime_type};base64,{base64_image}");

        let body = serde_json::json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": &data_uri}},
                    {"type": "text", "text": "How many distinct documents, receipts, pages, or separate items are visible in this image? Answer with ONLY a single integer number, nothing else."}
                ]
            }],
            "max_tokens": self.config.max_tokens,
            "temperature": 0.0,
            "thinking": {"type": "disabled"}
        });

        let resp = match self
            .client
            .post(format!("{base_url}/chat/completions"))
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Pre-classifier {model}: HTTP error: {e}");
                return DocumentCount::Unknown;
            }
        };

        if !resp.status().is_success() {
            log::warn!(
                "Pre-classifier {model}: HTTP {}: {:?}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
            return DocumentCount::Unknown;
        }

        let json: Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Pre-classifier {model}: JSON parse error: {e}");
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
    /// Handles: "4", "4.", "4 documents", "There are 4 receipts."
    pub(crate) fn parse_count(text: &str) -> u8 {
        if let Ok(n) = text.trim().parse::<u8>() {
            return n;
        }
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
        assert_eq!(
            PreClassifier::parse_count("I see 3 distinct receipts in this image"),
            3
        );
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
