//! Per-chat session management for the gateway.
//!
//! Each chat gets its own Jcode session, tracked by chat_id.
//! Sessions are lazily created on first message and persisted across restarts.

use dashmap::DashMap;
use ohagent_core::jcode_bridge::{JcodeBridge, SessionHandle, SessionConfig};
use std::sync::Arc;
use tracing::info;

/// Manages agent sessions per chat.
///
/// Thread-safe: uses DashMap for concurrent access from multiple
/// platform adapters and Telegram update handlers.
pub struct SessionManager {
    /// Map from platform:chat_id → SessionHandle.
    sessions: DashMap<String, SessionHandle>,
    bridge: Arc<JcodeBridge>,
}

impl SessionManager {
    pub fn new(bridge: Arc<JcodeBridge>) -> Self {
        Self {
            sessions: DashMap::new(),
            bridge,
        }
    }

    /// Get or create a session for the given chat.
    ///
    /// Sessions are lazily initialized on first message.
    /// Returns the existing session if one is already active.
    pub async fn get_or_create(
        &self,
        session_key: &str,
        tenant_id: &str,
    ) -> Result<SessionHandle, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(handle) = self.sessions.get(session_key) {
            return Ok(handle.clone());
        }

        info!(
            session_key = %session_key,
            tenant_id = %tenant_id,
            "Creating new gateway session"
        );

        let config = SessionConfig {
            model: None, // Use default from provider
            working_dir: Some(format!("/tmp/ohagent/{tenant_id}")),
            selfdev: false,
            report_back_to: None,
        };

        let handle = self.bridge.create_session(config).await?;
        self.sessions.insert(session_key.to_string(), handle.clone());

        info!(
            session_key = %session_key,
            session_id = %handle.session_id,
            "Gateway session ready"
        );

        Ok(handle)
    }

    /// Drop a session for the given chat, starting fresh.
    pub async fn reset(&self, session_key: &str) {
        if let Some((_, handle)) = self.sessions.remove(session_key) {
            info!(
                session_key = %session_key,
                session_id = %handle.session_id,
                "Resetting gateway session"
            );
            // Session implicitly dropped when handle is removed.
        }
    }

    /// Check if a session exists for the given chat.
    pub fn exists(&self, session_key: &str) -> bool {
        self.sessions.contains_key(session_key)
    }

    /// Get an existing session handle (returns None if not found).
    pub fn get(&self, session_key: &str) -> Option<SessionHandle> {
        self.sessions.get(session_key).map(|entry| entry.clone())
    }

    /// Get the number of active sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// List all active session keys.
    pub fn list_keys(&self) -> Vec<String> {
        self.sessions.iter().map(|entry| entry.key().clone()).collect()
    }
}
