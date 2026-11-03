//! Operator-owned connection profiles and a reloadable, nonsecret three-model policy.
//! This module never persists documents, credentials, provider errors or tenant data.
use crate::{
    validate_extraction, validate_request, DocumentError, ExtractRequest, Extraction,
    OpenAiVisionProvider, PdfRasterizer, VisionConfig, VisionProvider, MAX_RESPONSE_BYTES,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const MAX_REGISTRY_BYTES: u64 = 16 * 1024;
pub const MAX_COMPARISON_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSlot {
    pub id: String,
    pub label: String,
    pub model: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRegistry {
    pub revision: u64,
    pub models: Vec<ModelSlot>,
}

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

impl ModelRegistry {
    pub fn validate(&self) -> Result<(), RegistryError> {
        let mut ids = HashSet::new();
        if self.revision == 0
            || self.models.len() != 3
            || self.models.iter().any(|s| {
                !text(&s.id, 64)
                    || !s
                        .id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                    || !text(&s.label, 128)
                    || !text(&s.model, 256)
                    || !ids.insert(&s.id)
            })
        {
            return Err(RegistryError::Invalid);
        }
        Ok(())
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum RegistryError {
    #[error("documents.invalid_model_registry")]
    Invalid,
    #[error("documents.model_registry_unavailable")]
    Unavailable,
    #[error("documents.model_registry_conflict")]
    Conflict,
    #[error("documents.all_models_disabled")]
    AllDisabled,
}

/// Immutable server-only profile. It deliberately has no Debug or Serialize implementation.
/// The credential reference can only originate in process environment configuration.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionProfile {
    pub id: String,
    pub label: String,
    pub base_url: String,
    pub api_key_env: String,
    pub json_mode: String,
    pub pdf_mode: String,
    pub render_image: Option<String>,
    pub model: String,
    pub enabled: bool,
}

/// Factories receive only the mutable model identifier, never a caller-selected endpoint.
pub trait ModelProviderFactory: Send + Sync {
    fn create(&self, model: &str) -> Result<Arc<dyn VisionProvider>, DocumentError>;
}
impl<F> ModelProviderFactory for F
where
    F: Fn(&str) -> Result<Arc<dyn VisionProvider>, DocumentError> + Send + Sync,
{
    fn create(&self, model: &str) -> Result<Arc<dyn VisionProvider>, DocumentError> {
        self(model)
    }
}

struct ConfiguredFactory {
    base_url: String,
    api_key: Option<String>,
    json_object: bool,
    renderer: Option<PdfRasterizer>,
}
impl ModelProviderFactory for ConfiguredFactory {
    fn create(&self, model: &str) -> Result<Arc<dyn VisionProvider>, DocumentError> {
        let config = VisionConfig {
            base_url: self.base_url.clone(),
            model: model.into(),
            api_key: self.api_key.clone().ok_or(DocumentError::InvalidConfig)?,
        };
        let mut provider = OpenAiVisionProvider::new(config)?;
        if self.json_object {
            provider = provider.with_json_object_mode();
        }
        if let Some(renderer) = &self.renderer {
            provider = provider.with_pdf_rasterizer(renderer.clone());
        }
        Ok(Arc::new(provider))
    }
}

/// A boot-owned service. Persisted state contains only ModelRegistry. Every operation
/// reloads the file, including changes made by another service instance. Locking is
/// nonblocking and shared through a stable sidecar, not the atomically replaced inode.
pub struct ComparisonService {
    factories: HashMap<String, Arc<dyn ModelProviderFactory>>,
    storage: Storage,
}
enum Storage {
    File(PathBuf),
    Memory(std::sync::Mutex<ModelRegistry>),
}

impl ComparisonService {
    /// Read immutable connection profiles and resolve credentials once at boot.
    /// Missing/invalid configuration fails closed, independently of legacy extraction.
    pub fn from_env() -> Result<Self, RegistryError> {
        let raw =
            std::env::var("OHAGENT_DOCUMENT_PROFILES").map_err(|_| RegistryError::Unavailable)?;
        if raw.len() > MAX_REGISTRY_BYTES as usize {
            return Err(RegistryError::Invalid);
        }
        let profiles: Vec<ConnectionProfile> =
            serde_json::from_str(&raw).map_err(|_| RegistryError::Invalid)?;
        let path = std::env::var_os("OHAGENT_DOCUMENT_MODELS_FILE")
            .map(PathBuf::from)
            .ok_or(RegistryError::Unavailable)?;
        Self::from_profiles(profiles, path)
    }

    pub fn from_profiles(
        profiles: Vec<ConnectionProfile>,
        path: PathBuf,
    ) -> Result<Self, RegistryError> {
        let registry = ModelRegistry {
            revision: 1,
            models: profiles
                .iter()
                .map(|p| ModelSlot {
                    id: p.id.clone(),
                    label: p.label.clone(),
                    model: p.model.clone(),
                    enabled: p.enabled,
                })
                .collect(),
        };
        registry.validate()?;
        let mut factories: HashMap<String, Arc<dyn ModelProviderFactory>> = HashMap::new();
        for p in profiles {
            if p.api_key_env.is_empty()
                || p.api_key_env.len() > 128
                || !p
                    .api_key_env
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                return Err(RegistryError::Invalid);
            }
            crate::provider::completion_endpoint(&p.base_url, &p.model)
                .map_err(|_| RegistryError::Invalid)?;
            let key = std::env::var(&p.api_key_env).ok().filter(|key| {
                !key.trim().is_empty() && key.len() <= 4096 && !key.chars().any(char::is_control)
            });
            let json_object = match p.json_mode.as_str() {
                "json_schema" => false,
                "json_object" => true,
                _ => return Err(RegistryError::Invalid),
            };
            let renderer = match p.pdf_mode.as_str() {
                "native" if p.render_image.is_none() => None,
                "podman" => Some(
                    PdfRasterizer::new(p.render_image.ok_or(RegistryError::Invalid)?)
                        .map_err(|_| RegistryError::Invalid)?,
                ),
                _ => return Err(RegistryError::Invalid),
            };
            let factory = ConfiguredFactory {
                base_url: p.base_url,
                api_key: key,
                json_object,
                renderer,
            };
            factories.insert(p.id, Arc::new(factory));
        }
        Self::with_file(registry, factories, path)
    }

    /// Test/in-process injection, with no environment or filesystem mutation.
    pub fn in_memory(
        registry: ModelRegistry,
        factories: HashMap<String, Arc<dyn ModelProviderFactory>>,
    ) -> Result<Self, RegistryError> {
        let service = Self {
            factories,
            storage: Storage::Memory(std::sync::Mutex::new(registry.clone())),
        };
        service.validate(&registry)?;
        Ok(service)
    }

    /// The path and factories are operator-owned, never accepted from an HTTP request.
    pub fn with_file(
        initial: ModelRegistry,
        factories: HashMap<String, Arc<dyn ModelProviderFactory>>,
        path: PathBuf,
    ) -> Result<Self, RegistryError> {
        let service = Self {
            factories,
            storage: Storage::File(path.clone()),
        };
        service.validate(&initial)?;
        let _lock = lock_file(&path)?;
        match read_registry(&path) {
            Ok(registry) => service.validate(&registry)?,
            Err(RegistryError::Unavailable)
                if !path.try_exists().map_err(|_| RegistryError::Unavailable)? =>
            {
                write_registry(&path, &initial)?
            }
            Err(error) => return Err(error),
        }
        Ok(service)
    }

    fn validate(&self, registry: &ModelRegistry) -> Result<(), RegistryError> {
        registry.validate()?;
        if self.factories.len() != 3
            || registry
                .models
                .iter()
                .any(|s| !self.factories.contains_key(&s.id))
        {
            return Err(RegistryError::Invalid);
        }
        Ok(())
    }

    /// Synchronous bounded-size local IO. HTTP callers run it on a blocking worker.
    pub fn snapshot(&self) -> Result<ModelRegistry, RegistryError> {
        let registry = match &self.storage {
            Storage::Memory(value) => value
                .lock()
                .map_err(|_| RegistryError::Unavailable)?
                .clone(),
            Storage::File(path) => {
                let _lock = lock_file(path)?;
                read_registry(path)?
            }
        };
        self.validate(&registry)?;
        Ok(registry)
    }

    /// PUT carries the last observed revision. A successful update increments it.
    /// Exactly the configured IDs must remain. Ordering is mutable candidate priority.
    pub fn update(&self, mut proposed: ModelRegistry) -> Result<ModelRegistry, RegistryError> {
        self.validate(&proposed)?;
        match &self.storage {
            Storage::Memory(value) => {
                let mut current = value.lock().map_err(|_| RegistryError::Unavailable)?;
                advance(&mut proposed, current.revision)?;
                *current = proposed.clone();
            }
            Storage::File(path) => {
                let _lock = lock_file(path)?;
                let current = read_registry(path).map_err(|_| RegistryError::Unavailable)?;
                self.validate(&current)
                    .map_err(|_| RegistryError::Unavailable)?;
                advance(&mut proposed, current.revision)?;
                write_registry(path, &proposed)?;
            }
        }
        Ok(proposed)
    }

    /// Captures an immutable policy before starting at most three concurrent calls.
    /// Each call, including rendering, is bounded to 90 seconds, with no fallback.
    pub async fn compare(
        &self,
        registry: ModelRegistry,
        request: ExtractRequest,
    ) -> Result<ComparisonResponse, RegistryError> {
        self.compare_with_timeout(registry, request, Duration::from_secs(90))
            .await
    }

    /// Shorter deadlines permit deterministic timeout regression tests.
    pub async fn compare_with_timeout(
        &self,
        registry: ModelRegistry,
        request: ExtractRequest,
        timeout: Duration,
    ) -> Result<ComparisonResponse, RegistryError> {
        self.validate(&registry)?;
        if timeout.is_zero()
            || timeout > Duration::from_secs(90)
            || validate_request(&request).is_err()
        {
            return Err(RegistryError::Invalid);
        }
        if !registry.models.iter().any(|s| s.enabled) {
            return Err(RegistryError::AllDisabled);
        }
        let mut tasks = tokio::task::JoinSet::new();
        let mut results = Vec::with_capacity(3);
        for (index, slot) in registry.models.iter().enumerate() {
            results.push(ModelResult {
                id: slot.id.clone(),
                label: slot.label.clone(),
                model: slot.model.clone(),
                status: ModelStatus::Disabled,
                extraction: None,
                error_code: None,
            });
            if slot.enabled {
                // Attribute failures even if factory construction fails or a provider panics.
                results[index].status = ModelStatus::Failed;
                results[index].error_code = Some("provider_failed".into());
                let provider = self.factories[&slot.id].create(&slot.model);
                let request = request.clone();
                tasks.spawn(async move {
                    let outcome = match provider {
                        Ok(provider) => {
                            match tokio::time::timeout(timeout, provider.extract(request)).await {
                                Ok(result) => result.map_err(|e| match e {
                                    DocumentError::InvalidOutput => "invalid_extraction",
                                    DocumentError::ResponseTooLarge => "response_too_large",
                                    _ => "provider_failed",
                                }),
                                Err(_) => Err("provider_timeout"),
                            }
                        }
                        Err(_) => Err("provider_unavailable"),
                    };
                    (index, outcome)
                });
            }
        }
        while let Some(joined) = tasks.join_next().await {
            if let Ok((index, outcome)) = joined {
                let result = &mut results[index];
                match outcome {
                    Ok(mut extraction) => {
                        extraction.model.clone_from(&result.model);
                        if validate_extraction(&extraction).is_err() {
                            result.error_code = Some("invalid_extraction".into());
                        } else if serde_json::to_vec(&extraction)
                            .map_or(true, |bytes| bytes.len() > MAX_RESPONSE_BYTES - 4096)
                        {
                            result.error_code = Some("response_too_large".into());
                        } else {
                            result.status = ModelStatus::Succeeded;
                            result.error_code = None;
                            result.extraction = Some(extraction);
                        }
                    }
                    Err(code) => result.error_code = Some(code.into()),
                }
            }
        }
        let extraction = results.iter().find_map(|r| r.extraction.clone());
        let disagreements = disagreements(&results);
        let complete = results.iter().all(|r| r.status == ModelStatus::Succeeded);
        Ok(ComparisonResponse {
            extraction,
            comparison: Comparison {
                policy_revision: registry.revision,
                complete,
                results,
                disagreements,
            },
        })
    }
}

fn advance(proposed: &mut ModelRegistry, current: u64) -> Result<(), RegistryError> {
    if proposed.revision != current {
        return Err(RegistryError::Conflict);
    }
    proposed.revision = current.checked_add(1).ok_or(RegistryError::Conflict)?;
    Ok(())
}
fn lock_file(path: &Path) -> Result<File, RegistryError> {
    let mut name = path.as_os_str().to_os_string();
    name.push(".lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(PathBuf::from(name))
        .map_err(|_| RegistryError::Unavailable)?;
    file.try_lock().map_err(|_| RegistryError::Unavailable)?;
    Ok(file)
}
fn read_registry(path: &Path) -> Result<ModelRegistry, RegistryError> {
    let file = File::open(path).map_err(|_| RegistryError::Unavailable)?;
    let metadata = file.metadata().map_err(|_| RegistryError::Unavailable)?;
    if !metadata.is_file() || metadata.len() > MAX_REGISTRY_BYTES {
        return Err(RegistryError::Invalid);
    }
    let mut bytes = Vec::new();
    file.take(MAX_REGISTRY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RegistryError::Unavailable)?;
    if bytes.len() > MAX_REGISTRY_BYTES as usize {
        return Err(RegistryError::Invalid);
    }
    serde_json::from_slice(&bytes).map_err(|_| RegistryError::Invalid)
}
fn write_registry(path: &Path, registry: &ModelRegistry) -> Result<(), RegistryError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let bytes = serde_json::to_vec(registry).map_err(|_| RegistryError::Invalid)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| RegistryError::Unavailable)?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| RegistryError::Unavailable)?;
    temporary
        .persist(path)
        .map_err(|_| RegistryError::Unavailable)?;
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|_| RegistryError::Unavailable)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResponse {
    pub extraction: Option<Extraction>,
    pub comparison: Comparison,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    pub policy_revision: u64,
    pub complete: bool,
    pub results: Vec<ModelResult>,
    pub disagreements: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResult {
    pub id: String,
    pub label: String,
    pub model: String,
    pub status: ModelStatus,
    pub extraction: Option<Extraction>,
    pub error_code: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    Succeeded,
    Failed,
    Disabled,
}

fn disagreements(results: &[ModelResult]) -> Vec<String> {
    let values: Vec<_> = results
        .iter()
        .filter_map(|r| r.extraction.as_ref())
        .collect();
    let Some(first) = values.first() else {
        return vec![];
    };
    let mut differences = vec![];
    if values
        .iter()
        .any(|v| v.document_type != first.document_type)
    {
        differences.push("document_type".into());
    }
    let first_fields = serde_json::to_value(&first.fields).expect("typed serializable fields");
    for (name, value) in first_fields.as_object().expect("typed field object") {
        if values.iter().skip(1).any(|v| {
            serde_json::to_value(&v.fields).expect("typed serializable fields")[name] != *value
        }) {
            differences.push(format!("fields.{name}"));
        }
    }
    differences.sort_unstable();
    differences
}
