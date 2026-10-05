//! Provider-neutral semantic advice for agent-managed live context.
//!
//! Decisions from this module are advisory. They do not mutate live context,
//! durable memory, evidence, or authorization state.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPressure {
    Normal,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextItemAction {
    KeepExact,
    Keep,
    Summarize,
    DropCandidate,
}

impl ContextItemAction {
    fn from_wire(value: &str) -> Option<Self> {
        match value {
            "keep_exact" => Some(Self::KeepExact),
            "keep" => Some(Self::Keep),
            "summarize" => Some(Self::Summarize),
            "drop_candidate" => Some(Self::DropCandidate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextItem {
    pub id: String,
    pub section: String,
    pub text: String,
    pub must_preserve_exact: bool,
}

#[derive(Debug, Clone)]
pub struct ContextDecisionRequest {
    pub current_goal: Option<String>,
    pub pressure: ContextPressure,
    pub items: Vec<ContextItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextItemDecision {
    pub item_id: String,
    pub action: ContextItemAction,
    pub confidence: f64,
    pub provider: String,
    pub model: String,
}

#[async_trait]
pub trait ContextDecisionEngine: Send + Sync {
    fn provider_id(&self) -> &str;

    async fn evaluate(&self, request: &ContextDecisionRequest)
        -> Result<Vec<ContextItemDecision>>;
}

/// Extract bullet-level working-state items from the canonical live-context file.
///
/// This deliberately ignores arbitrary prose outside known markdown sections.
/// The parser is deterministic and does not decide whether content is true.
pub fn extract_context_items(content: &str) -> Vec<ContextItem> {
    let mut section = String::new();
    let mut items = Vec::new();

    for raw in content.lines() {
        let line = raw.trim();
        if let Some(heading) = line.strip_prefix("## ") {
            section = heading.trim().to_string();
            continue;
        }
        let Some(text) = line.strip_prefix("- ") else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() || text == "(empty)" || section.is_empty() {
            continue;
        }

        let must_preserve_exact = section == "Evidence references still in use";
        let id = context_item_id(&section, text);
        items.push(ContextItem {
            id,
            section: section.clone(),
            text: text.to_string(),
            must_preserve_exact,
        });
    }

    items
}

fn context_item_id(section: &str, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(section.as_bytes());
    hasher.update([0]);
    hasher.update(text.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    digest[..16].to_string()
}

/// Safe baseline used when no semantic provider is configured.
///
/// It never recommends dropping content. Exact evidence references remain exact;
/// all other working-state bullets are retained. This is intentionally boring:
/// semantic providers must beat it in benchmarks before automatic use.
#[derive(Debug, Default)]
pub struct ConservativeContextDecisionEngine;

#[async_trait]
impl ContextDecisionEngine for ConservativeContextDecisionEngine {
    fn provider_id(&self) -> &str {
        "deterministic-conservative"
    }

    async fn evaluate(
        &self,
        request: &ContextDecisionRequest,
    ) -> Result<Vec<ContextItemDecision>> {
        Ok(request
            .items
            .iter()
            .map(|item| ContextItemDecision {
                item_id: item.id.clone(),
                action: if item.must_preserve_exact {
                    ContextItemAction::KeepExact
                } else {
                    ContextItemAction::Keep
                },
                confidence: 1.0,
                provider: self.provider_id().to_string(),
                model: "rules-v1".to_string(),
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
pub struct SystemOneContextDecisionConfig {
    /// Full System One endpoint, for example http://127.0.0.1:11434/v1/systemone.
    pub endpoint: String,
    pub model: String,
    pub api_key: Option<String>,
    /// Remote endpoints are refused unless explicitly allowed.
    pub allow_external: bool,
}

impl SystemOneContextDecisionConfig {
    pub fn local(endpoint: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            model: model.into(),
            api_key: None,
            allow_external: false,
        }
    }

    pub fn external(
        endpoint: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            model: model.into(),
            api_key: Some(api_key.into()),
            allow_external: true,
        }
    }
}

/// Minimal System One wire adapter for context-management recommendations.
///
/// Compatible transports may include Jev or local /v1/systemone runtimes, but
/// wire compatibility does not imply calibration compatibility. This adapter
/// returns raw provider confidence; thresholding/calibration belongs above it.
pub struct SystemOneContextDecisionEngine {
    config: SystemOneContextDecisionConfig,
    client: reqwest::Client,
    provider_id: String,
}

impl SystemOneContextDecisionEngine {
    pub fn new(config: SystemOneContextDecisionConfig) -> Result<Self> {
        if config.endpoint.trim().is_empty() {
            return Err(anyhow!("System One endpoint must be non-empty"));
        }
        if config.model.trim().is_empty() {
            return Err(anyhow!("System One model must be non-empty"));
        }
        if !config.allow_external && !is_loopback_endpoint(&config.endpoint) {
            return Err(anyhow!(
                "external System One endpoint requires explicit allow_external"
            ));
        }

        let provider_id = if is_loopback_endpoint(&config.endpoint) {
            "system-one-local"
        } else {
            "system-one-external"
        }
        .to_string();

        Ok(Self {
            config,
            client: reqwest::Client::new(),
            provider_id,
        })
    }

    fn request_body(&self, request: &ContextDecisionRequest) -> Value {
        let items: Vec<Value> = request
            .items
            .iter()
            .map(|item| {
                json!({
                    "id": item.id,
                    "section": item.section,
                    "text": item.text,
                    "must_preserve_exact": item.must_preserve_exact,
                })
            })
            .collect();

        let mut questions = serde_json::Map::new();
        for item in &request.items {
            questions.insert(
                question_key(&item.id),
                json!({
                    "type": "choice",
                    "instructions": format!(
                        "Choose the safest context-management action for item {}. Never choose summarize or drop_candidate when must_preserve_exact is true. Prefer keep when uncertain.",
                        item.id
                    ),
                    "criteria": {
                        "keep_exact": "Preserve this item verbatim because exact identity/provenance matters.",
                        "keep": "Keep the item available in working context.",
                        "summarize": "The item may be condensed without losing material state.",
                        "drop_candidate": "The item is resolved/stale and is a candidate for removal, subject to deterministic policy."
                    }
                }),
            );
        }

        json!({
            "model": self.config.model,
            "state": {
                "purpose": "Advisory working-context management. These decisions never authorize deletion by themselves.",
                "current_goal": request.current_goal,
                "pressure": request.pressure,
                "items": items,
            },
            "questions": questions,
        })
    }
}

#[async_trait]
impl ContextDecisionEngine for SystemOneContextDecisionEngine {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn evaluate(
        &self,
        request: &ContextDecisionRequest,
    ) -> Result<Vec<ContextItemDecision>> {
        if request.items.is_empty() {
            return Ok(Vec::new());
        }

        let body = serde_json::to_vec(&self.request_body(request))?;
        let mut http = self
            .client
            .post(&self.config.endpoint)
            .header(CONTENT_TYPE, "application/json")
            .body(body);
        if let Some(api_key) = self.config.api_key.as_ref() {
            http = http.header(AUTHORIZATION, format!("Bearer {api_key}"));
        }

        let response = http
            .send()
            .await
            .context("System One context decision request failed")?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .context("read System One context decision response")?;
        if !status.is_success() {
            return Err(anyhow!(
                "System One context decision provider returned HTTP {}",
                status.as_u16()
            ));
        }

        let value: Value =
            serde_json::from_slice(&bytes).context("parse System One context decision response")?;
        parse_system_one_response(&value, request, self.provider_id())
    }
}

fn parse_system_one_response(
    value: &Value,
    request: &ContextDecisionRequest,
    provider: &str,
) -> Result<Vec<ContextItemDecision>> {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("System One response missing model"))?
        .to_string();
    let answers = value
        .get("answers")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("System One response missing answers"))?;

    let by_id: HashMap<&str, &ContextItem> =
        request.items.iter().map(|item| (item.id.as_str(), item)).collect();
    let mut decisions = Vec::with_capacity(request.items.len());

    for item in &request.items {
        let key = question_key(&item.id);
        let answer = answers
            .get(&key)
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow!("System One response missing answer for {}", item.id))?;
        if answer.get("type").and_then(Value::as_str) != Some("choice") {
            return Err(anyhow!("System One answer for {} is not choice", item.id));
        }
        let choice = answer
            .get("choice")
            .and_then(Value::as_str)
            .and_then(ContextItemAction::from_wire)
            .ok_or_else(|| anyhow!("System One answer for {} has invalid choice", item.id))?;
        let confidence = answer
            .get("confidence")
            .and_then(Value::as_f64)
            .filter(|value| (0.0..=1.0).contains(value))
            .ok_or_else(|| anyhow!("System One answer for {} has invalid confidence", item.id))?;

        let source = by_id
            .get(item.id.as_str())
            .ok_or_else(|| anyhow!("context decision item identity mismatch"))?;
        if source.must_preserve_exact
            && !matches!(choice, ContextItemAction::KeepExact | ContextItemAction::Keep)
        {
            return Err(anyhow!(
                "System One attempted lossy action for exact-preserve item {}",
                item.id
            ));
        }

        decisions.push(ContextItemDecision {
            item_id: item.id.clone(),
            action: choice,
            confidence,
            provider: provider.to_string(),
            model: model.clone(),
        });
    }

    Ok(decisions)
}

fn question_key(item_id: &str) -> String {
    format!("ctx_{item_id}")
}

fn is_loopback_endpoint(endpoint: &str) -> bool {
    let lower = endpoint.trim().to_ascii_lowercase();
    lower.starts_with("http://127.0.0.1:")
        || lower.starts_with("http://localhost:")
        || lower.starts_with("http://[::1]:")
        || lower.starts_with("https://127.0.0.1:")
        || lower.starts_with("https://localhost:")
        || lower.starts_with("https://[::1]:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_stable_bullet_items_and_protects_evidence_section() {
        let content = r#"# ohAgent Live Context

## Current goals
- finish incident review

## Evidence references still in use
- trace:abc123
- (empty)

## Next actions
- inspect Graphify path
"#;
        let items = extract_context_items(content);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].section, "Current goals");
        assert!(!items[0].must_preserve_exact);
        assert_eq!(items[1].section, "Evidence references still in use");
        assert!(items[1].must_preserve_exact);
        assert_eq!(items[0].id, context_item_id("Current goals", "finish incident review"));
    }

    #[tokio::test]
    async fn conservative_engine_never_drops_items() {
        let items = vec![
            ContextItem {
                id: "a".into(),
                section: "Evidence references still in use".into(),
                text: "trace:1".into(),
                must_preserve_exact: true,
            },
            ContextItem {
                id: "b".into(),
                section: "Next actions".into(),
                text: "run test".into(),
                must_preserve_exact: false,
            },
        ];
        let decisions = ConservativeContextDecisionEngine
            .evaluate(&ContextDecisionRequest {
                current_goal: None,
                pressure: ContextPressure::High,
                items,
            })
            .await
            .unwrap();

        assert_eq!(decisions[0].action, ContextItemAction::KeepExact);
        assert_eq!(decisions[1].action, ContextItemAction::Keep);
    }

    #[test]
    fn local_system_one_refuses_non_loopback_without_explicit_opt_in() {
        let result = SystemOneContextDecisionEngine::new(SystemOneContextDecisionConfig::local(
            "https://api.example.com/v1/systemone",
            "model",
        ));
        assert!(result.is_err());
    }

    #[test]
    fn parses_valid_system_one_decisions() {
        let request = ContextDecisionRequest {
            current_goal: Some("fix incident".into()),
            pressure: ContextPressure::High,
            items: vec![
                ContextItem {
                    id: "a1".into(),
                    section: "Next actions".into(),
                    text: "rerun test".into(),
                    must_preserve_exact: false,
                },
                ContextItem {
                    id: "b2".into(),
                    section: "Evidence references still in use".into(),
                    text: "trace:abc".into(),
                    must_preserve_exact: true,
                },
            ],
        };
        let value = json!({
            "model": "jev-1.13.0",
            "answers": {
                "ctx_a1": {
                    "type": "choice",
                    "choice": "summarize",
                    "confidence": 0.91,
                    "probabilities": {}
                },
                "ctx_b2": {
                    "type": "choice",
                    "choice": "keep_exact",
                    "confidence": 0.99,
                    "probabilities": {}
                }
            }
        });

        let decisions = parse_system_one_response(&value, &request, "typesafe").unwrap();
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[0].action, ContextItemAction::Summarize);
        assert_eq!(decisions[1].action, ContextItemAction::KeepExact);
        assert_eq!(decisions[0].model, "jev-1.13.0");
    }

    #[test]
    fn rejects_lossy_provider_action_for_exact_evidence() {
        let request = ContextDecisionRequest {
            current_goal: None,
            pressure: ContextPressure::Normal,
            items: vec![ContextItem {
                id: "e1".into(),
                section: "Evidence references still in use".into(),
                text: "evidence:42".into(),
                must_preserve_exact: true,
            }],
        };
        let value = json!({
            "model": "provider-model",
            "answers": {
                "ctx_e1": {
                    "type": "choice",
                    "choice": "drop_candidate",
                    "confidence": 0.99
                }
            }
        });

        assert!(parse_system_one_response(&value, &request, "provider").is_err());
    }
}
