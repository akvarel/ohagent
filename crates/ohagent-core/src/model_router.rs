//! Intelligent model router — task-based model selection.
//!
//! ## How it works
//!
//! 1. **Catalog** loads from `models.toml` — lists available models with
//!    capabilities, cost tiers, and provider info.
//! 2. **TaskClassifier** scans the user message for patterns and maps them
//!    to required capabilities (coding, reasoning, image_gen, etc.).
//! 3. **Router** picks the best model: cheapest available that satisfies
//!    all required capabilities, respecting the max auto-tier.
//!
//! ## Usage
//!
//! ```ignore
//! let router = ModelRouter::load()?;
//! let (provider, model_name) = router.route("Deploy to Kubernetes")?;
//! // → deepseek-v4-flash (coding task, low tier)
//!
//! let (provider, model_name) = router.route("Generate a photo of a cat")?;
//! // → flux-1.1-pro (image_gen task)
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

use jcode_base::provider::MultiProvider;
use jcode_provider_core::Provider;

// ── Catalog types ──

/// A single model entry from the catalog.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelEntry {
    /// Unique model ID (e.g. "deepseek-v4-flash")
    pub id: String,
    /// Provider family: "deepseek", "openai", "anthropic", "openrouter", etc.
    pub provider: String,
    /// Environment variable that holds the API key
    pub api_key_env: String,
    /// Human-readable name
    pub display: String,
    /// What this model can do
    pub capabilities: Vec<String>,
    /// Pricing tier: "low", "medium", "high"
    pub cost_tier: String,
    /// Max context window in tokens
    pub context: u32,
}

/// Top-level catalog config.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelCatalog {
    pub models: Vec<ModelEntry>,
    #[serde(default)]
    pub defaults: CatalogDefaults,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogDefaults {
    #[serde(default = "default_fallback_capability")]
    pub fallback_capability: String,
    #[serde(default = "default_tier")]
    pub default_tier: String,
    #[serde(default = "default_max_auto_tier")]
    pub max_auto_tier: String,
}

fn default_fallback_capability() -> String {
    "general_chat".into()
}
fn default_tier() -> String {
    "low".into()
}
fn default_max_auto_tier() -> String {
    "medium".into()
}

impl Default for CatalogDefaults {
    fn default() -> Self {
        Self {
            fallback_capability: default_fallback_capability(),
            default_tier: default_tier(),
            max_auto_tier: default_max_auto_tier(),
        }
    }
}

// ── Task classification ──

/// Capability tags set by the classifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    Coding,
    Reasoning,
    Analysis,
    GeneralChat,
    CreativeWriting,
    ImageGen,
    VideoGen,
}

impl Capability {
    fn as_str(&self) -> &'static str {
        match self {
            Capability::Coding => "coding",
            Capability::Reasoning => "reasoning",
            Capability::Analysis => "analysis",
            Capability::GeneralChat => "general_chat",
            Capability::CreativeWriting => "creative_writing",
            Capability::ImageGen => "image_gen",
            Capability::VideoGen => "video_gen",
        }
    }
}

/// Analyze a user message and determine required capabilities.
pub fn classify_task(message: &str) -> Vec<Capability> {
    let lower = message.to_lowercase();
    let mut caps = Vec::new();

    // Image generation patterns
    if lower.contains("generate") && (lower.contains("image") || lower.contains("picture") || lower.contains("photo") || lower.contains("draw"))
        || lower.contains("create") && (lower.contains("image") || lower.contains("picture") || lower.contains("photo"))
        || lower.starts_with("draw ")
        || lower.contains("dall-e")
    {
        caps.push(Capability::ImageGen);
        return caps; // Image gen is exclusive
    }

    // Video generation patterns
    if (lower.contains("generate") || lower.contains("create") || lower.contains("make"))
        && lower.contains("video")
        || lower.contains("animate")
        || lower.contains("animation")
    {
        caps.push(Capability::VideoGen);
        return caps;
    }

    // Coding patterns
    if lower.contains("code") || lower.contains("implement") || lower.contains("fix bug")
        || lower.contains("refactor") || lower.contains("function") || lower.contains("class ")
        || lower.contains("api") || lower.contains("endpoint") || lower.contains("deploy")
        || lower.contains("docker") || lower.contains("kubernetes") || lower.contains("k8s")
        || lower.contains("test ") || lower.contains("compile") || lower.contains("build ")
        || lower.contains("error") || lower.contains("debug") || lower.contains("commit")
        || lower.contains("git ") || lower.contains("pull request") || lower.contains("pr ")
        || lower.contains("merge") || lower.contains("cargo") || lower.contains("npm ")
        || lower.contains("pip ") || lower.contains("import ") || lower.contains("mod ")
        || lower.contains("trait ") || lower.contains("struct ") || lower.contains("impl ")
        || lower.contains("fn ") || lower.contains("def ") || lower.contains("class ")
        || lower.contains("use ") && (lower.contains("rust") || lower.contains("python"))
    {
        caps.push(Capability::Coding);
    }

    // Reasoning patterns (deep thinking needed)
    if lower.contains("think") || lower.contains("reason") || lower.contains("analyze")
        || lower.contains("explain why") || lower.contains("prove") || lower.contains("logic")
        || lower.contains("puzzle") || lower.contains("riddle") || lower.contains("solve")
        || lower.contains("optimize") || lower.contains("architecture") || lower.contains("design pattern")
    {
        caps.push(Capability::Reasoning);
    }

    // Analysis patterns
    if lower.contains("analyze") || lower.contains("summarize") || lower.contains("break down")
        || lower.contains("compare") || lower.contains("evaluate") || lower.contains("review")
        || lower.contains("audit") || lower.contains("assess")
    {
        caps.push(Capability::Analysis);
    }

    // Creative writing
    if lower.contains("write") && (lower.contains("story") || lower.contains("poem") || lower.contains("script")
        || lower.contains("article") || lower.contains("blog") || lower.contains("creative"))
        || lower.contains("compose") || lower.contains("narrative")
    {
        caps.push(Capability::CreativeWriting);
    }

    // Default: general chat
    if caps.is_empty() {
        caps.push(Capability::GeneralChat);
    }

    caps
}

// ── Cost tier ordering ──

fn tier_value(tier: &str) -> u8 {
    match tier {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        _ => 1,
    }
}

// ── Router ──

/// Intelligent model router.
pub struct ModelRouter {
    catalog: ModelCatalog,
}

/// Result of routing a task to a model.
pub struct RoutedModel {
    pub provider: Arc<dyn Provider>,
    pub model_id: String,
    pub display_name: String,
    pub task_capabilities: Vec<Capability>,
}

impl ModelRouter {
    /// Load the catalog from the embedded `models.toml`.
    pub fn load() -> Result<Self> {
        let catalog_str = include_str!("models.toml");
        let catalog: ModelCatalog = toml::from_str(catalog_str)
            .context("Failed to parse model catalog")?;
        info!(
            models = catalog.models.len(),
            "Model catalog loaded"
        );
        Ok(Self { catalog })
    }

    /// Load from a custom catalog path.
    pub fn load_from(path: &str) -> Result<Self> {
        let catalog_str = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read catalog from {path}"))?;
        let catalog: ModelCatalog = toml::from_str(&catalog_str)
            .context("Failed to parse model catalog")?;
        Ok(Self { catalog })
    }

    /// List all models in the catalog.
    pub fn list_models(&self) -> &[ModelEntry] {
        &self.catalog.models
    }

    /// Find the closest matching model for a set of capabilities.
    ///
    /// Priority: cheapest tier that has all requested capabilities and has
    /// an API key set in the environment.
    pub fn find_model(
        &self,
        required_caps: &[Capability],
        max_tier: Option<&str>,
    ) -> Option<&ModelEntry> {
        let max_tier = max_tier.unwrap_or(&self.catalog.defaults.max_auto_tier);
        let max_tier_val = tier_value(max_tier);

        let required: Vec<&str> = required_caps.iter().map(|c| c.as_str()).collect();

        self.catalog
            .models
            .iter()
            .filter(|m| {
                // Must have all required capabilities
                required
                    .iter()
                    .all(|rc| m.capabilities.iter().any(|mc| mc == rc))
            })
            .filter(|m| tier_value(&m.cost_tier) <= max_tier_val)
            .filter(|m| {
                // Must have API key available
                std::env::var(&m.api_key_env).is_ok()
            })
            .min_by_key(|m| (tier_value(&m.cost_tier), m.id.as_str()))
    }

    /// Route a user message to the best model.
    ///
    /// Returns a `RoutedModel` with a ready-to-use provider, or falls back
    /// to the first available model in the catalog.
    pub fn route(
        &self,
        message: &str,
    ) -> Result<RoutedModel> {
        let caps = classify_task(message);
        let cap_names: Vec<&str> = caps.iter().map(|c| c.as_str()).collect();
        debug!(message = %message, capabilities = ?cap_names, "Classified task");

        let entry = self
            .find_model(&caps, None)
            .or_else(|| {
                // Fall back: try general_chat capability
                let fallback = vec![Capability::GeneralChat];
                self.find_model(&fallback, None)
            })
            .or_else(|| {
                // Last resort: first model in catalog with any key set
                self.catalog.models.iter().find(|m| {
                    std::env::var(&m.api_key_env).is_ok()
                })
            })
            .context("No available model found — check API keys")?;

        let provider = self.build_provider(entry)?;

        info!(
            model = %entry.display,
            capabilities = ?cap_names,
            cost_tier = %entry.cost_tier,
            "Routed to model"
        );

        Ok(RoutedModel {
            provider,
            model_id: entry.id.clone(),
            display_name: entry.display.clone(),
            task_capabilities: caps,
        })
    }

    /// Build a MultiProvider configured for the given model entry.
    fn build_provider(&self, entry: &ModelEntry) -> Result<Arc<dyn Provider>> {
        let multi = MultiProvider::default();

        match entry.provider.as_str() {
            "deepseek" => {
                let key = std::env::var(&entry.api_key_env)
                    .with_context(|| format!("{} not set", entry.api_key_env))?;
                // MultiProvider reads DEEPSEEK_API_KEY from env; ensure it's set
                std::env::set_var("DEEPSEEK_API_KEY", &key);
                // Use the openrouter path for DeepSeek (it's an OpenAI-compatible profile)
                multi.set_model(&format!("deepseek:{}", entry.id))
                    .with_context(|| format!("Failed to set model deepseek:{}", entry.id))?;
            }
            "anthropic" => {
                let key = std::env::var(&entry.api_key_env)
                    .with_context(|| format!("{} not set", entry.api_key_env))?;
                std::env::set_var("ANTHROPIC_API_KEY", &key);
                multi.set_model(&format!("claude:{}", entry.id))
                    .with_context(|| format!("Failed to set model claude:{}", entry.id))?;
            }
            "openai" => {
                let key = std::env::var(&entry.api_key_env)
                    .with_context(|| format!("{} not set", entry.api_key_env))?;
                std::env::set_var("OPENAI_API_KEY", &key);
                multi.set_model(&format!("openai:{}", entry.id))
                    .with_context(|| format!("Failed to set model openai:{}", entry.id))?;
            }
            "openrouter" => {
                let key = std::env::var(&entry.api_key_env)
                    .with_context(|| format!("{} not set", entry.api_key_env))?;
                std::env::set_var("OPENROUTER_API_KEY", &key);
                multi.set_model(&format!("openrouter:{}", entry.id))
                    .with_context(|| format!("Failed to set model openrouter:{}", entry.id))?;
            }
            _ => {
                // Unknown provider — try as openrouter model
                let key = std::env::var(&entry.api_key_env)
                    .with_context(|| format!("{} not set", entry.api_key_env))?;
                std::env::set_var("OPENROUTER_API_KEY", &key);
                multi.set_model(&format!("openrouter:{}", entry.id))
                    .with_context(|| format!("Failed to set model openrouter:{}", entry.id))?;
            }
        }

        Ok(Arc::new(multi))
    }

    /// Get diagnostic info: available models grouped by capability.
    pub fn diagnostics(&self) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for m in &self.catalog.models {
            let available = std::env::var(&m.api_key_env).is_ok();
            let display = if available {
                m.display.clone()
            } else {
                format!("{} (no key)", m.display)
            };
            for cap in &m.capabilities {
                map.entry(cap.clone()).or_default().push(display.clone());
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_coding() {
        let caps = classify_task("deploy to kubernetes cluster");
        assert!(caps.contains(&Capability::Coding));
    }

    #[test]
    fn test_classify_image_gen() {
        let caps = classify_task("generate a photo of a sunset over mountains");
        assert!(caps.contains(&Capability::ImageGen));
        assert_eq!(caps.len(), 1, "Image gen should be exclusive");
    }

    #[test]
    fn test_classify_video_gen() {
        let caps = classify_task("create a video of a rocket launch");
        assert!(caps.contains(&Capability::VideoGen));
    }

    #[test]
    fn test_classify_reasoning() {
        let caps = classify_task("think deeply about the architecture of distributed systems");
        assert!(caps.contains(&Capability::Reasoning));
    }

    #[test]
    fn test_classify_creative_writing() {
        let caps = classify_task("write a short story about a robot learning to love");
        assert!(caps.contains(&Capability::CreativeWriting));
    }

    #[test]
    fn test_classify_general_chat() {
        let caps = classify_task("hello, how are you?");
        assert!(caps.contains(&Capability::GeneralChat));
    }

    #[test]
    fn test_classify_analysis() {
        let caps = classify_task("analyze the performance of this code");
        assert!(caps.contains(&Capability::Analysis));
    }

    #[test]
    fn test_catalog_parses() {
        let catalog_str = include_str!("models.toml");
        let catalog: ModelCatalog = toml::from_str(catalog_str).unwrap();
        assert!(!catalog.models.is_empty());
        assert!(catalog.models.iter().any(|m| m.id == "deepseek-v4-flash"));
    }

    #[test]
    fn test_router_loads() {
        let router = ModelRouter::load().unwrap();
        let models = router.list_models();
        assert!(models.len() > 5, "Expected many models, got {}", models.len());
    }

    #[test]
    fn test_find_coding_model() {
        let router = ModelRouter::load().unwrap();
        // At minimum DEEPSEEK_API_KEY should be set for tests
        let found = router.find_model(&[Capability::Coding], None);
        if std::env::var("DEEPSEEK_API_KEY").is_ok() {
            assert!(found.is_some(), "Should find a coding model");
        }
    }

    #[test]
    fn test_diagnostics() {
        let router = ModelRouter::load().unwrap();
        let diag = router.diagnostics();
        assert!(diag.contains_key("coding"));
        assert!(diag.contains_key("image_gen"));
    }
}
