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
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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
    /// Provider family: "deepseek", "openai", "anthropic", "openrouter", "zhipu"
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
    /// USD per 1M input tokens (None for non-LLM models like image gen)
    #[serde(default)]
    pub input_price: Option<f64>,
    /// USD per 1M output tokens
    #[serde(default)]
    pub output_price: Option<f64>,
    /// Off-peak discount multiplier (0.0–1.0, e.g. 0.50 = 50% off)
    #[serde(default)]
    pub off_peak_discount: Option<f64>,
    /// Off-peak window start in UTC (HH:MM format)
    #[serde(default)]
    pub off_peak_start_utc: Option<String>,
    /// Off-peak window end in UTC (HH:MM format)
    #[serde(default)]
    pub off_peak_end_utc: Option<String>,
    /// Whether model is enabled (can be toggled at runtime via API)
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
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
    /// Per-tenant capability -> model_id overrides
    user_prefs: HashMap<String, HashMap<String, String>>,
    /// Path for persisting preferences
    prefs_path: Option<PathBuf>,
    /// Runtime disabled models (via API toggle). Persisted to disabled_path.
    disabled_models: HashSet<String>,
    /// Path for persisting disabled models
    disabled_path: Option<PathBuf>,
}

/// Result of routing a task to a model.
pub struct RoutedModel {
    pub provider: Arc<dyn Provider>,
    pub model_id: String,
    pub display_name: String,
    pub task_capabilities: Vec<Capability>,
}

/// Model status returned by the API.
#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub display: String,
    pub provider: String,
    pub cost_tier: String,
    pub enabled: bool,
    pub has_api_key: bool,
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
        Ok(Self {
            catalog,
            user_prefs: HashMap::new(),
            prefs_path: None,
            disabled_models: HashSet::new(),
            disabled_path: None,
        })
    }

    /// Load from a custom catalog path.
    pub fn load_from(path: &str) -> Result<Self> {
        let catalog_str = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read catalog from {path}"))?;
        let catalog: ModelCatalog = toml::from_str(&catalog_str)
            .context("Failed to parse model catalog")?;
        Ok(Self {
            catalog,
            user_prefs: HashMap::new(),
            prefs_path: None,
            disabled_models: HashSet::new(),
            disabled_path: None,
        })
    }

    /// Set the path for disabled models persistence and load existing.
    pub fn with_disabled_path(mut self, path: PathBuf) -> Self {
        self.disabled_path = Some(path.clone());
        if let Err(e) = self.load_disabled() {
            debug!(error = %e, "No existing disabled list, starting fresh");
        }
        self
    }

    /// Check if a model is enabled (catalog default + runtime override).
    pub fn is_enabled(&self, model_id: &str) -> bool {
        // Runtime override takes precedence
        if self.disabled_models.contains(model_id) {
            return false;
        }
        // Catalog default
        self.catalog.models
            .iter()
            .find(|m| m.id == model_id)
            .map(|m| m.enabled)
            .unwrap_or(true)
    }

    /// Enable or disable a model at runtime (persisted).
    pub fn set_enabled(&mut self, model_id: &str, enabled: bool) -> Result<()> {
        // Validate model exists
        if !self.catalog.models.iter().any(|m| m.id == model_id) {
            return Err(anyhow::anyhow!("Unknown model: {model_id}"));
        }
        if enabled {
            self.disabled_models.remove(model_id);
            info!(%model_id, "Model enabled");
        } else {
            self.disabled_models.insert(model_id.to_string());
            info!(%model_id, "Model disabled");
        }
        self.save_disabled()?;
        Ok(())
    }

    /// List all models with their enabled state.
    pub fn model_statuses(&self) -> Vec<ModelStatus> {
        self.catalog.models.iter().map(|m| {
            ModelStatus {
                id: m.id.clone(),
                display: m.display.clone(),
                provider: m.provider.clone(),
                cost_tier: m.cost_tier.clone(),
                enabled: self.is_enabled(&m.id),
                has_api_key: std::env::var(&m.api_key_env).is_ok(),
            }
        }).collect()
    }

    /// Load disabled list from disk.
    fn load_disabled(&mut self) -> Result<()> {
        if let Some(ref path) = self.disabled_path {
            if path.exists() {
                let data = std::fs::read_to_string(path)?;
                let list: Vec<String> = serde_json::from_str(&data)?;
                self.disabled_models = list.into_iter().collect();
                debug!(count = self.disabled_models.len(), "Loaded disabled models");
            }
        }
        Ok(())
    }

    /// Persist disabled list to disk.
    fn save_disabled(&self) -> Result<()> {
        if let Some(ref path) = self.disabled_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let list: Vec<&String> = self.disabled_models.iter().collect();
            let data = serde_json::to_string_pretty(&list)?;
            std::fs::write(path, &data)?;
        }
        Ok(())
    }

    /// Get the catalog (used by API to expose model list).
    pub fn catalog(&self) -> &[ModelEntry] {
        &self.catalog.models
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
            .filter(|m| self.is_enabled(&m.id))
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
    /// Checks per-tenant preferences first, then falls back to auto-routing.
    /// Returns a `RoutedModel` with a ready-to-use provider, or falls back
    /// to the first available model in the catalog.
    pub fn route(
        &self,
        tenant_id: &str,
        message: &str,
    ) -> Result<RoutedModel> {
        let caps = classify_task(message);
        let cap_names: Vec<&str> = caps.iter().map(|c| c.as_str()).collect();
        debug!(message = %message, tenant = %tenant_id, capabilities = ?cap_names, "Classified task");

        // Check user preference for the primary capability
        let preferred = caps.first().and_then(|cap| {
            self.get_pref(tenant_id, cap.as_str())
        });

        let entry = if let Some(pref_model_id) = preferred {
            // Try to find the preferred model in the catalog
            let preferred_entry = self.catalog.models.iter().find(|m| m.id == pref_model_id)
                .and_then(|m| {
                    if std::env::var(&m.api_key_env).is_ok() {
                        Some(m)
                    } else {
                        None
                    }
                });
            match preferred_entry {
                Some(e) => {
                    info!(model = %e.display, "Using user-preferred model");
                    Some(e)
                }
                None => {
                    // Preferred model not available, fall back to auto
                    debug!("Preferred model {pref_model_id} not available, falling back to auto");
                    None
                }
            }
        } else {
            None
        };

        let entry = entry.or_else(|| {
            self.find_model(&caps, None)
        })
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
            "zhipu" => {
                // Zhipu / Z.ai — OpenAI-compatible API
                let key = std::env::var(&entry.api_key_env)
                    .with_context(|| format!("{} not set", entry.api_key_env))?;
                std::env::set_var("OPENAI_API_KEY", &key);
                // Route through openai provider with custom base URL set via env
                std::env::set_var("OPENAI_BASE_URL", "https://api.z.ai/v1");
                multi.set_model(&format!("openai:{}", entry.id))
                    .with_context(|| format!("Failed to set model openai:{}", entry.id))?;
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

    // ── Per-tenant model preferences ──

    /// Set the path for persisting preferences (and load existing ones).
    pub fn with_prefs_path(mut self, path: PathBuf) -> Self {
        self.prefs_path = Some(path.clone());
        if let Err(e) = self.load_prefs() {
            debug!(error = %e, "No existing model prefs found, starting fresh");
        }
        self
    }

    /// Set a preference: for `tenant`, use `model_id` for `capability`.
    pub fn set_pref(&mut self, tenant: &str, capability: &str, model_id: &str) -> Result<()> {
        // Validate that the model exists in the catalog
        if !self.catalog.models.iter().any(|m| m.id == model_id) {
            return Err(anyhow::anyhow!("Unknown model: {model_id}"));
        }
        self.user_prefs
            .entry(tenant.to_string())
            .or_default()
            .insert(capability.to_lowercase(), model_id.to_string());
        self.save_prefs()?;
        info!(tenant = %tenant, capability = %capability, model = %model_id, "Model preference set");
        Ok(())
    }

    /// Get a model preference for a tenant + capability.
    pub fn get_pref(&self, tenant: &str, capability: &str) -> Option<&str> {
        self.user_prefs
            .get(tenant)
            .and_then(|caps| caps.get(&capability.to_lowercase()))
            .map(|s| s.as_str())
    }

    /// Clear preferences for a tenant. If `capability` is Some, clear only
    /// that capability. If None, clear all preferences for the tenant.
    pub fn clear_pref(
        &mut self,
        tenant: &str,
        capability: Option<&str>,
    ) -> Result<()> {
        match capability {
            Some(cap) => {
                if let Some(caps) = self.user_prefs.get_mut(tenant) {
                    caps.remove(&cap.to_lowercase());
                }
                info!(tenant = %tenant, capability = %cap, "Cleared model preference");
            }
            None => {
                self.user_prefs.remove(tenant);
                info!(tenant = %tenant, "Cleared all model preferences");
            }
        }
        self.save_prefs()?;
        Ok(())
    }

    /// List all preferences for a tenant.
    pub fn list_prefs(&self, tenant: &str) -> HashMap<String, String> {
        self.user_prefs
            .get(tenant)
            .cloned()
            .unwrap_or_default()
    }

    /// Load preferences from the prefs_path file.
    fn load_prefs(&mut self) -> Result<()> {
        if let Some(ref path) = self.prefs_path {
            if path.exists() {
                let data = std::fs::read_to_string(path)?;
                self.user_prefs = toml::from_str(&data)?;
                debug!(path = %path.display(), "Loaded model preferences");
            }
        }
        Ok(())
    }

    /// Persist preferences to the prefs_path file.
    fn save_prefs(&self) -> Result<()> {
        if let Some(ref path) = self.prefs_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let data = toml::to_string_pretty(&self.user_prefs)?;
            std::fs::write(path, &data)?;
        }
        Ok(())
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
