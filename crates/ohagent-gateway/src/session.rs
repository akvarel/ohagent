//! Per-chat session management for the gateway.
//!
//! Each chat gets its own Jcode session, tracked by chat_id.
//! Sessions are lazily created on first message and evicted when
//! idle or when the session cache exceeds the maximum size (LRU).

use dashmap::DashMap;
use ohagent_core::jcode_bridge::{JcodeBridge, SessionHandle, SessionConfig};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Default maximum number of cached agent sessions.
const DEFAULT_MAX_SESSIONS: usize = 128;
/// Default idle TTL: evict sessions idle longer than this.
const DEFAULT_IDLE_TTL_SECS: u64 = 3600;

/// A session entry with activity tracking for LRU + TTL eviction.
struct SessionEntry {
    handle: SessionHandle,
    /// Monotonic timestamp in millis since epoch (atomic for concurrent touch).
    last_active: AtomicU64,
}

/// Manages agent sessions per chat with LRU eviction and idle TTL.
///
/// Thread-safe: uses DashMap for concurrent access from multiple
/// platform adapters and Telegram update handlers.
pub struct SessionManager {
    /// Map from platform:chat_id → session entry with metadata.
    sessions: DashMap<String, SessionEntry>,
    bridge: Arc<JcodeBridge>,
    max_sessions: usize,
    idle_ttl: Duration,
}

impl SessionManager {
    pub fn new(bridge: Arc<JcodeBridge>) -> Self {
        Self {
            sessions: DashMap::new(),
            bridge,
            max_sessions: DEFAULT_MAX_SESSIONS,
            idle_ttl: Duration::from_secs(DEFAULT_IDLE_TTL_SECS),
        }
    }

    /// Configure the maximum number of cached sessions.
    pub fn with_max_sessions(mut self, max: usize) -> Self {
        self.max_sessions = max.max(1);
        self
    }

    /// Configure the idle TTL for session eviction.
    pub fn with_idle_ttl_secs(mut self, secs: u64) -> Self {
        self.idle_ttl = Duration::from_secs(secs);
        self
    }

    /// Get or create a session for the given chat.
    ///
    /// Sessions are lazily initialized on first message.
    /// Evicts the least recently used session when the cache is full.
    pub async fn get_or_create(
        &self,
        session_key: &str,
        tenant_id: &str,
    ) -> Result<SessionHandle, Box<dyn std::error::Error + Send + Sync>> {
        // Fast path: existing session
        if let Some(entry) = self.sessions.get(session_key) {
            entry.last_active.store(now_millis(), Ordering::Relaxed);
            return Ok(entry.handle.clone());
        }

        // Evict stale sessions before creating a new one
        self.evict_stale();

        // Check capacity and evict LRU if needed
        if self.sessions.len() >= self.max_sessions {
            self.evict_lru();
        }

        info!(
            session_key = %session_key,
            tenant_id = %tenant_id,
            "Creating new gateway session"
        );

        let config = SessionConfig {
            model: None,
            working_dir: Some(format!("/tmp/ohagent/{tenant_id}")),
            selfdev: false,
            report_back_to: None,
        };

        let handle = self.bridge.create_session(config).await?;
        self.sessions.insert(session_key.to_string(), SessionEntry {
            handle: handle.clone(),
            last_active: AtomicU64::new(now_millis()),
        });

        info!(
            session_key = %session_key,
            session_id = %handle.session_id,
            "Gateway session ready"
        );

        Ok(handle)
    }

    /// Touch a session to update its last_active timestamp (call on every message).
    pub fn touch(&self, session_key: &str) {
        if let Some(entry) = self.sessions.get(session_key) {
            entry.last_active.store(now_millis(), Ordering::Relaxed);
        }
    }

    /// Drop a session for the given chat, starting fresh.
    pub async fn reset(&self, session_key: &str) {
        if let Some((_, entry)) = self.sessions.remove(session_key) {
            info!(
                session_key = %session_key,
                session_id = %entry.handle.session_id,
                "Resetting gateway session"
            );
        }
    }

    /// Check if a session exists for the given chat.
    pub fn exists(&self, session_key: &str) -> bool {
        self.sessions.contains_key(session_key)
    }

    /// Get an existing session handle (returns None if not found).
    pub fn get(&self, session_key: &str) -> Option<SessionHandle> {
        self.sessions.get(session_key).map(|entry| entry.handle.clone())
    }

    /// Get the number of active sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// List all active session keys.
    pub fn list_keys(&self) -> Vec<String> {
        self.sessions.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Evict sessions that have been idle beyond the TTL.
    fn evict_stale(&self) {
        let cutoff = now_millis().saturating_sub(self.idle_ttl.as_millis() as u64);
        let mut evicted = 0usize;

        self.sessions.retain(|key, entry| {
            let expired = entry.last_active.load(Ordering::Relaxed) < cutoff;
            if expired {
                evicted += 1;
                info!(session_key = %key, "Evicted idle session");
            }
            !expired
        });

        if evicted > 0 {
            info!(evicted = evicted, "Evicted idle sessions");
        }
    }

    /// Evict the single least recently used session.
    fn evict_lru(&self) {
        let lru_key = self.sessions.iter()
            .min_by_key(|entry| entry.last_active.load(Ordering::Relaxed))
            .map(|entry| entry.key().clone());

        if let Some(key) = lru_key {
            if let Some((_, entry)) = self.sessions.remove(&key) {
                warn!(
                    session_key = %key,
                    session_id = %entry.handle.session_id,
                    "Evicted LRU session (cache full)"
                );
            }
        }
    }
}

/// Monotonic timestamp in milliseconds for LRU/TTL comparisons.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
