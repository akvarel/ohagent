//! Component health tracking for ohAgent daemon.
//!
//! Provides a shared registry where subsystems register their status.
//! The `/health` endpoint reads this registry to report per-component readiness.
//!
//! # Thread safety
//!
//! `HealthRegistry` is `Clone`-safe: clones share the same inner table.
//! Components update their status concurrently via `Arc<HealthRegistry>`.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Health status of a single component.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HealthStatus {
    /// Component started and passed its last check.
    Healthy,
    /// Component is still initialising.
    Starting,
    /// Component failed (non-fatal — daemon continues without it).
    Degraded,
    /// Component is disabled by configuration.
    Disabled,
    /// Component encountered a critical error.
    Unhealthy,
}

impl HealthStatus {
    /// Returns `true` if the status is Healthy or Starting.
    pub fn is_ok(&self) -> bool {
        matches!(self, HealthStatus::Healthy | HealthStatus::Starting)
    }

    /// Returns `true` if the daemon should consider itself fully operational.
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }
}

/// A single component health snapshot.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ComponentHealth {
    pub status: HealthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub uptime_seconds: u64,
}

impl ComponentHealth {
    fn new(status: HealthStatus) -> Self {
        Self {
            status,
            message: None,
            last_error: None,
            uptime_seconds: 0,
        }
    }
}

/// Shared health registry — components register by name and update their status.
///
/// # Example
///
/// ```ignore
/// let registry = HealthRegistry::new();
/// registry.register("provider", "LLM provider");
/// registry.set_healthy("provider", "DeepSeek V4-Flash connected");
/// ```
#[derive(Debug, Clone)]
pub struct HealthRegistry {
    inner: Arc<RwLock<HashMap<String, ComponentHealth>>>,
    start_time: Arc<RwLock<chrono::DateTime<chrono::Utc>>>,
}

impl Default for HealthRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthRegistry {
    /// Create a new empty health registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            start_time: Arc::new(RwLock::new(chrono::Utc::now())),
        }
    }

    /// Register a component with initial `Starting` status.
    ///
    /// Panics if the component is already registered.
    pub fn register(&self, name: &str, message: &str) {
        let mut map = self.inner.write().expect("health registry lock");
        let prev = map.insert(
            name.to_string(),
            ComponentHealth {
                status: HealthStatus::Starting,
                message: Some(message.to_string()),
                last_error: None,
                uptime_seconds: 0,
            },
        );
        assert!(prev.is_none(), "component '{name}' registered twice");
    }

    /// Mark a component as healthy.
    pub fn set_healthy(&self, name: &str, message: &str) {
        self.update(name, HealthStatus::Healthy, message, None);
    }

    /// Mark a component as degraded (non-fatal failure).
    pub fn set_degraded(&self, name: &str, message: &str) {
        self.update(name, HealthStatus::Degraded, message, None);
    }

    /// Mark a component as degraded with an error detail.
    pub fn set_degraded_with_error(&self, name: &str, message: &str, error: &str) {
        self.update(name, HealthStatus::Degraded, message, Some(error));
    }

    /// Mark a component as disabled.
    pub fn set_disabled(&self, name: &str) {
        self.update(name, HealthStatus::Disabled, "disabled by configuration", None);
    }

    /// Mark a component as unhealthy (fatal).
    pub fn set_unhealthy(&self, name: &str, error: &str) {
        self.update(name, HealthStatus::Unhealthy, error, Some(error));
    }

    fn update(&self, name: &str, status: HealthStatus, message: &str, error: Option<&str>) {
        let started = *self.start_time.read().expect("health start_time lock");
        let uptime = (chrono::Utc::now() - started).num_seconds().max(0) as u64;

        let mut map = self.inner.write().expect("health registry lock");
        if let Some(entry) = map.get_mut(name) {
            entry.status = status;
            entry.message = Some(message.to_string());
            entry.last_error = error.map(|s| s.to_string());
            entry.uptime_seconds = uptime;
        } else {
            // Auto-register if not pre-registered
            map.insert(
                name.to_string(),
                ComponentHealth {
                    status,
                    message: Some(message.to_string()),
                    last_error: error.map(|s| s.to_string()),
                    uptime_seconds: uptime,
                },
            );
        }
    }

    /// Get a snapshot of all component health.
    pub fn snapshot(&self) -> HashMap<String, ComponentHealth> {
        let started = *self.start_time.read().expect("health start_time lock");
        let uptime = (chrono::Utc::now() - started).num_seconds().max(0) as u64;

        let map = self.inner.read().expect("health registry lock");
        let mut snap: HashMap<String, ComponentHealth> = map.clone();
        // Update all uptimes to current
        for (_, entry) in snap.iter_mut() {
            entry.uptime_seconds = uptime;
        }
        snap
    }

    /// Returns `true` if all registered components are healthy.
    pub fn all_healthy(&self) -> bool {
        let map = self.inner.read().expect("health registry lock");
        map.values().all(|c| c.status == HealthStatus::Healthy)
    }

    /// Returns `true` if no component is Unhealthy (Starting / Degraded is acceptable).
    pub fn is_operational(&self) -> bool {
        let map = self.inner.read().expect("health registry lock");
        map.values()
            .all(|c| matches!(c.status, HealthStatus::Healthy | HealthStatus::Starting | HealthStatus::Degraded | HealthStatus::Disabled))
    }

    /// Overall daemon health: all critical components healthy.
    pub fn daemon_status(&self) -> HealthStatus {
        let map = self.inner.read().expect("health registry lock");
        if map.is_empty() {
            return HealthStatus::Starting;
        }
        if map.values().any(|c| c.status == HealthStatus::Unhealthy) {
            return HealthStatus::Unhealthy;
        }
        if map.values().all(|c| c.status == HealthStatus::Healthy || c.status == HealthStatus::Disabled) {
            return HealthStatus::Healthy;
        }
        if map.values().any(|c| c.status == HealthStatus::Degraded) {
            return HealthStatus::Degraded;
        }
        HealthStatus::Starting
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_lifecycle() {
        let h = HealthRegistry::new();
        h.register("provider", "LLM provider");
        h.register("memory", "Memory engine");

        assert_eq!(h.daemon_status(), HealthStatus::Starting);

        h.set_healthy("provider", "DeepSeek connected");
        h.set_healthy("memory", "SQLite ready");

        assert_eq!(h.daemon_status(), HealthStatus::Healthy);

        h.set_degraded("memory", "Embedding model unavailable");
        assert_eq!(h.daemon_status(), HealthStatus::Degraded);
    }

    #[test]
    fn test_all_healthy() {
        let h = HealthRegistry::new();
        h.register("a", "A");
        h.register("b", "B");
        assert!(!h.all_healthy());

        h.set_healthy("a", "OK");
        h.set_healthy("b", "OK");
        assert!(h.all_healthy());
    }

    #[test]
    fn test_is_operational() {
        let h = HealthRegistry::new();
        h.register("a", "A");
        h.set_degraded("a", "partial");
        assert!(h.is_operational());

        h.set_unhealthy("a", "crashed");
        assert!(!h.is_operational());
    }

    #[test]
    fn test_auto_register_on_set() {
        let h = HealthRegistry::new();
        h.set_healthy("auto", "auto registered");
        let snap = h.snapshot();
        assert!(snap.contains_key("auto"));
    }
}
