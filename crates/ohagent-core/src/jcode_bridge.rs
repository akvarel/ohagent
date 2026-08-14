//! Jcode Bridge — programmatic API for embedding Jcode through the public SDK.
//!
//! ohAgent owns tenancy, routing, and gateway concerns. Jcode owns session
//! execution and built-in tools through its public SDK/runtime boundary.

use crate::model_router::ModelRouter;
use crate::tools::ToolRegistry;
use jcode_base::mcp::SharedMcpPool;
use jcode_provider_core::Provider as ProviderTrait;
use jcode_sdk::{JcodeClient, LaunchOptions, LaunchedInstance, RunOptions, SessionInfo};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use tracing::info;

/// Error type for Jcode bridge operations.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("Jcode session error: {0}")]
    Session(String),
    #[error("Jcode message processing error: {0}")]
    Message(String),
    #[error("Provider error: {0}")]
    Provider(String),
}

/// Bridge-level runtime configuration.
#[derive(Debug, Clone, Default)]
pub struct JcodeBridgeConfig {
    /// Persistent root for tenant-private Jcode homes. Env fallback: OHAGENT_JCODE_RUNTIME_ROOT.
    pub runtime_root: Option<PathBuf>,
    /// Jcode binary used for private SDK runtimes. Env fallback: OHAGENT_JCODE_BINARY.
    pub jcode_binary: Option<PathBuf>,
}

/// Configuration for creating a new agent session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Explicit tenant or security-domain identifier. Never used raw in filesystem paths.
    pub tenant_id: String,
    /// Optional model override.
    pub model: Option<String>,
    /// Safe absolute workspace path for the agent.
    pub working_dir: Option<String>,
    /// Unsupported by the SDK bridge. Rejected instead of silently ignored.
    pub selfdev: bool,
    /// Unsupported by the SDK bridge. Rejected instead of silently ignored.
    pub report_back_to: Option<String>,
}

/// A handle to a Jcode SDK session.
#[derive(Clone)]
pub struct SessionHandle {
    pub session_id: String,
    tenant_id: String,
    client: Arc<JcodeClient>,
}

impl SessionHandle {
    pub async fn send_message(&self, content: &str) -> Result<(), BridgeError> {
        self.send_message_with_images(content, Vec::new())
            .await
            .map(drop)
    }

    /// Send a text message with images and return assistant text collected by the SDK.
    pub async fn send_message_with_images(
        &self,
        content: &str,
        images: Vec<(String, String)>,
    ) -> Result<String, BridgeError> {
        let client = Arc::clone(&self.client);
        let session_id = self.session_id.clone();
        let content = content.to_string();
        tokio::task::spawn_blocking(move || {
            client.run(
                &session_id,
                &content,
                RunOptions {
                    images,
                    on_event: None,
                    auto_approve: false,
                },
            )
        })
        .await
        .map_err(|e| BridgeError::Message(e.to_string()))?
        .map(|turn| turn.text)
        .map_err(|e| BridgeError::Message(e.to_string()))
    }

    /// Send a soft interrupt signal to stop the current agent operation.
    pub async fn interrupt(&self) -> Result<(), BridgeError> {
        let client = Arc::clone(&self.client);
        let session_id = self.session_id.clone();
        tokio::task::spawn_blocking(move || {
            client.soft_interrupt(&session_id, "ohAgent gateway interrupt", true)
        })
        .await
        .map_err(|e| BridgeError::Message(e.to_string()))?
        .map_err(|e| BridgeError::Message(e.to_string()))
    }

    /// Cancel the current session turn.
    pub async fn cancel(&self) -> Result<(), BridgeError> {
        let client = Arc::clone(&self.client);
        let session_id = self.session_id.clone();
        tokio::task::spawn_blocking(move || client.cancel(&session_id))
            .await
            .map_err(|e| BridgeError::Message(e.to_string()))?
            .map_err(|e| BridgeError::Message(e.to_string()))
    }

    /// Compatibility shim. Jcode SDK owns built-in tool execution.
    pub async fn send_message_with_tools(&self, content: &str) -> Result<String, BridgeError> {
        self.send_message_with_images(content, Vec::new()).await
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }
}

struct TenantRuntime {
    _instance: LaunchedInstance,
    client: Arc<JcodeClient>,
}

/// Main bridge between ohAgent and Jcode.
pub struct JcodeBridge {
    sessions: Arc<RwLock<HashMap<String, (String, Arc<JcodeClient>)>>>,
    runtimes: Arc<RwLock<HashMap<String, Arc<TenantRuntime>>>>,
    provider: Arc<dyn ProviderTrait>,
    router: Option<Arc<Mutex<ModelRouter>>>,
    tool_registry: Arc<ToolRegistry>,
    mcp_pool: Option<Arc<SharedMcpPool>>,
    config: JcodeBridgeConfig,
}

impl JcodeBridge {
    pub fn new(provider: Arc<dyn ProviderTrait>) -> Self {
        info!("Initializing Jcode SDK bridge");
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            runtimes: Arc::new(RwLock::new(HashMap::new())),
            provider,
            router: None,
            tool_registry: Arc::new(ToolRegistry::new()),
            mcp_pool: None,
            config: JcodeBridgeConfig::default(),
        }
    }

    pub fn with_config(mut self, config: JcodeBridgeConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_router(mut self, router: Arc<Mutex<ModelRouter>>) -> Self {
        self.router = Some(router);
        self
    }

    /// Retained for OpenAI/ws provider paths. Gateway session execution no longer invokes it.
    pub fn with_tools(mut self, registry: ToolRegistry) -> Self {
        self.tool_registry = Arc::new(registry);
        self
    }

    pub fn with_mcp_pool(mut self, pool: Arc<SharedMcpPool>) -> Self {
        self.mcp_pool = Some(pool);
        self
    }

    pub fn tools(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }

    pub fn provider_name(&self) -> String {
        self.provider.display_name()
    }

    /// Get a reference to the underlying provider for direct OpenAI/API paths.
    pub fn provider(&self) -> &Arc<dyn ProviderTrait> {
        &self.provider
    }

    pub fn route_message(&self, tenant_id: &str, message: &str) -> String {
        match &self.router {
            Some(router) => router
                .lock()
                .unwrap()
                .route(tenant_id, message)
                .map(|routed| routed.display_name)
                .unwrap_or_else(|_| self.provider.display_name()),
            None => self.provider.display_name(),
        }
    }

    pub fn validate_session_config(&self, config: &SessionConfig) -> Result<(), BridgeError> {
        let mut unsupported = Vec::new();
        if config.selfdev {
            unsupported.push("selfdev");
        }
        if config.report_back_to.is_some() {
            unsupported.push("report_back_to");
        }
        if !unsupported.is_empty() {
            return Err(BridgeError::Session(format!(
                "unsupported SDK session options: {}",
                unsupported.join(", ")
            )));
        }
        if config.tenant_id.trim().is_empty() {
            return Err(BridgeError::Session("tenant_id must be explicit".into()));
        }
        if let Some(working_dir) = &config.working_dir {
            validate_safe_absolute_path(working_dir, "working_dir")?;
        }
        Ok(())
    }

    pub fn runtime_home_for_tenant(&self, tenant_id: &str) -> Result<PathBuf, BridgeError> {
        if tenant_id.trim().is_empty() {
            return Err(BridgeError::Session("tenant_id must be explicit".into()));
        }
        let root = self.runtime_root();
        let hash = stable_hash_hex(tenant_id);
        Ok(root.join(hash))
    }

    pub fn session_scope_key(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<String, BridgeError> {
        if tenant_id.trim().is_empty() {
            return Err(BridgeError::Session("tenant_id must be explicit".into()));
        }
        if session_id.trim().is_empty() {
            return Err(BridgeError::Session("session_id must be explicit".into()));
        }
        Ok(format!("{}:{session_id}", stable_hash_hex(tenant_id)))
    }

    pub async fn create_session(
        &self,
        config: SessionConfig,
    ) -> Result<SessionHandle, BridgeError> {
        self.validate_session_config(&config)?;
        let runtime = self.runtime_for_tenant(&config).await?;
        let client = Arc::clone(&runtime.client);
        let working_dir = config.working_dir.clone();
        let session: SessionInfo =
            tokio::task::spawn_blocking(move || client.create_session(working_dir))
                .await
                .map_err(|e| BridgeError::Session(e.to_string()))?
                .map_err(|e| BridgeError::Session(e.to_string()))?;

        if let Some(model) = config.model.clone() {
            let client = Arc::clone(&runtime.client);
            let session_id = session.session_id.clone();
            tokio::task::spawn_blocking(move || client.set_model(&session_id, &model))
                .await
                .map_err(|e| BridgeError::Session(e.to_string()))?
                .map_err(|e| BridgeError::Session(e.to_string()))?;
        }

        let session_id = session.session_id;
        let scoped_key = self.session_scope_key(&config.tenant_id, &session_id)?;
        self.sessions.write().await.insert(
            scoped_key,
            (config.tenant_id.clone(), Arc::clone(&runtime.client)),
        );

        Ok(SessionHandle {
            session_id,
            tenant_id: config.tenant_id,
            client: Arc::clone(&runtime.client),
        })
    }

    pub async fn get_session(&self, tenant_id: &str, session_id: &str) -> Option<SessionHandle> {
        let key = self.session_scope_key(tenant_id, session_id).ok()?;
        let sessions = self.sessions.read().await;
        let (stored_tenant_id, client) = sessions.get(&key)?.clone();
        Some(SessionHandle {
            session_id: session_id.to_string(),
            tenant_id: stored_tenant_id,
            client,
        })
    }

    pub async fn list_sessions(&self, tenant_id: &str) -> Vec<String> {
        let prefix = match self.session_scope_key(tenant_id, "_") {
            Ok(key) => key.trim_end_matches('_').to_string(),
            Err(_) => return Vec::new(),
        };
        self.sessions
            .read()
            .await
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix).map(ToString::to_string))
            .collect()
    }

    pub async fn archive_session(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<(), BridgeError> {
        let handle = self
            .get_session(tenant_id, session_id)
            .await
            .ok_or_else(|| BridgeError::Session(format!("session {session_id} not found")))?;
        tokio::task::spawn_blocking(move || handle.client.archive_session(&handle.session_id))
            .await
            .map_err(|e| BridgeError::Session(e.to_string()))?
            .map_err(|e| BridgeError::Session(e.to_string()))
    }

    pub async fn detach_session(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<(), BridgeError> {
        let handle = self
            .get_session(tenant_id, session_id)
            .await
            .ok_or_else(|| BridgeError::Session(format!("session {session_id} not found")))?;
        tokio::task::spawn_blocking(move || handle.client.detach_session(&handle.session_id))
            .await
            .map_err(|e| BridgeError::Session(e.to_string()))?
            .map_err(|e| BridgeError::Session(e.to_string()))
    }

    pub async fn drop_session(&self, tenant_id: &str, session_id: &str) -> Result<(), BridgeError> {
        let key = self.session_scope_key(tenant_id, session_id)?;
        self.sessions.write().await.remove(&key);
        Ok(())
    }

    async fn runtime_for_tenant(
        &self,
        config: &SessionConfig,
    ) -> Result<Arc<TenantRuntime>, BridgeError> {
        if let Some(existing) = self.runtimes.read().await.get(&config.tenant_id).cloned() {
            return Ok(existing);
        }

        let tenant_id = config.tenant_id.clone();
        let jcode_home = self.runtime_home_for_tenant(&tenant_id)?;
        let binary = self.jcode_binary();
        let inherited_working_dir = config.working_dir.as_ref().map(PathBuf::from);

        let runtime = tokio::task::spawn_blocking(move || {
            let mut launch = LaunchOptions {
                jcode_home: Some(jcode_home),
                working_dir: inherited_working_dir,
                inherit_logins: false,
                binary,
                client_name: "ohagent-core".to_string(),
                ..LaunchOptions::default()
            };
            launch.env.insert("JCODE_INHERIT_LOGINS".into(), "0".into());
            let instance = jcode_sdk::launch_instance(&launch)?;
            let client = JcodeClient::connect(jcode_sdk::ConnectOptions {
                socket_path: Some(instance.socket_path.clone()),
                client_name: "ohagent-core".to_string(),
                ..jcode_sdk::ConnectOptions::default()
            })?;
            Ok::<_, jcode_sdk::Error>(TenantRuntime {
                _instance: instance,
                client: Arc::new(client),
            })
        })
        .await
        .map_err(|e| BridgeError::Session(e.to_string()))?
        .map_err(|e| BridgeError::Session(e.to_string()))?;

        let runtime = Arc::new(runtime);
        let mut runtimes = self.runtimes.write().await;
        Ok(runtimes
            .entry(config.tenant_id.clone())
            .or_insert_with(|| Arc::clone(&runtime))
            .clone())
    }

    fn runtime_root(&self) -> PathBuf {
        self.config
            .runtime_root
            .clone()
            .or_else(|| std::env::var_os("OHAGENT_JCODE_RUNTIME_ROOT").map(PathBuf::from))
            .unwrap_or_else(|| std::env::temp_dir().join("ohagent").join("jcode-runtimes"))
    }

    fn jcode_binary(&self) -> Option<PathBuf> {
        self.config
            .jcode_binary
            .clone()
            .or_else(|| std::env::var_os("OHAGENT_JCODE_BINARY").map(PathBuf::from))
    }
}

fn validate_safe_absolute_path(path: &str, label: &str) -> Result<(), BridgeError> {
    let path = Path::new(path);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(BridgeError::Session(format!(
            "{label} must be an absolute path without traversal"
        )));
    }
    Ok(())
}

fn stable_hash_hex(value: &str) -> String {
    format!("rt-{:x}", Sha256::digest(value.as_bytes()))
}
