//! Jcode Bridge — programmatic API for embedding Jcode's agent loop.
//!
//! This module wraps Jcode's internal headless session API into a clean,
//! ergonomic interface that ohAgent can use from daemon, gateway, and cron.

use jcode_agent_runtime::SoftInterruptSource;
use jcode_app_core::{
    agent::Agent,
    server::{
        headless::create_headless_session,
        client_lifecycle::process_message_streaming_mpsc,
        state::SessionInterruptQueues,
        SessionAgents, SwarmMember,
    },
};
use jcode_provider_core::Provider as ProviderTrait;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
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

/// Configuration for creating a new agent session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Optional model override
    pub model: Option<String>,
    /// Working directory for the agent
    pub working_dir: Option<String>,
    /// Whether to enable self-dev tools
    pub selfdev: bool,
    /// Session ID to report results back to (for swarm delegation)
    pub report_back_to: Option<String>,
}

/// A handle to a running Jcode agent session.
///
/// Created via `JcodeBridge::create_session()`.
pub struct SessionHandle {
    pub session_id: String,
    agent: Arc<Mutex<Agent>>,
}

impl SessionHandle {
    /// Send a text message to this agent.
    ///
    /// The message is processed through Jcode's agent loop.
    /// Returns `Ok(())` when the agent has finished processing.
    pub async fn send_message(&self, content: &str) -> Result<(), BridgeError> {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        let agent = Arc::clone(&self.agent);
        let text = content.to_string();

        // Spawn processing in background
        let handle = tokio::spawn(async move {
            process_message_streaming_mpsc(
                agent,
                &text,
                Vec::new(),
                None,
                event_tx,
            )
            .await
        });

        // Drain events while processing
        while event_rx.recv().await.is_some() {}

        handle
            .await
            .map_err(|e| BridgeError::Message(e.to_string()))?
            .map_err(|e| BridgeError::Message(e.to_string()))?;

        Ok(())
    }

    /// Send a soft interrupt signal to stop the current agent operation.
    pub async fn interrupt(&self) {
        let agent = self.agent.lock().await;
        agent.soft_interrupt_queue()
            .lock()
            .unwrap()
            .push(jcode_agent_runtime::SoftInterruptMessage {
                content: "ohAgent gateway interrupt".to_string(),
                urgent: true,
                source: SoftInterruptSource::User,
            });
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

/// Main bridge between ohAgent and Jcode.
///
/// Manages the lifecycle of Jcode sessions.
/// Provider is configured externally and passed in.
pub struct JcodeBridge {
    sessions: SessionAgents,
    global_session_id: Arc<RwLock<String>>,
    provider: Arc<dyn ProviderTrait>,
    swarm_members: Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_coordinators: Arc<RwLock<HashMap<String, String>>>,
    swarm_plans: Arc<RwLock<HashMap<String, jcode_app_core::plan::VersionedPlan>>>,
    soft_interrupt_queues: SessionInterruptQueues,
}

impl JcodeBridge {
    /// Create a new Jcode bridge with the given provider.
    ///
    /// The provider must implement `jcode_provider_core::Provider`.
    /// Use Jcode's provider resolution or create a custom provider externally.
    pub fn new(provider: Arc<dyn ProviderTrait>) -> Self {
        info!("Initializing Jcode bridge");
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            global_session_id: Arc::new(RwLock::new(String::new())),
            provider,
            swarm_members: Arc::new(RwLock::new(HashMap::new())),
            swarms_by_id: Arc::new(RwLock::new(HashMap::new())),
            swarm_coordinators: Arc::new(RwLock::new(HashMap::new())),
            swarm_plans: Arc::new(RwLock::new(HashMap::new())),
            soft_interrupt_queues: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new headless agent session.
    pub async fn create_session(
        &self,
        config: SessionConfig,
    ) -> Result<SessionHandle, BridgeError> {
        let command = config.working_dir.as_deref().unwrap_or("");

        let result_json = create_headless_session(
            &self.sessions,
            &self.global_session_id,
            &self.provider,
            command,
            &self.swarm_members,
            &self.swarms_by_id,
            &self.swarm_coordinators,
            &self.swarm_plans,
            &self.soft_interrupt_queues,
            config.selfdev,
            config.model.clone(),
            None, // provider_key_override
            None, // route_api_method_override
            None, // mcp_pool
            config.report_back_to.clone(),
        )
        .await
        .map_err(|e| BridgeError::Session(e.to_string()))?;

        // Parse session info from result
        let info: serde_json::Value =
            serde_json::from_str(&result_json)
                .map_err(|e| BridgeError::Session(format!("Failed to parse session info: {e}")))?;

        let session_id = info["session_id"]
            .as_str()
            .ok_or_else(|| BridgeError::Session("Missing session_id in response".into()))?
            .to_string();

        // Retrieve the agent from sessions
        let sessions_guard = self.sessions.read().await;
        let agent = sessions_guard
            .get(&session_id)
            .cloned()
            .ok_or_else(|| BridgeError::Session(format!("Session {session_id} not found after creation")))?;

        info!(session_id = %session_id, "Created Jcode session");

        Ok(SessionHandle {
            session_id,
            agent,
        })
    }

    /// Get an existing session by ID.
    pub async fn get_session(&self, session_id: &str) -> Option<SessionHandle> {
        let sessions = self.sessions.read().await;
        let agent = sessions.get(session_id)?.clone();

        Some(SessionHandle {
            session_id: session_id.to_string(),
            agent,
        })
    }

    /// List all active session IDs.
    pub async fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }
}
