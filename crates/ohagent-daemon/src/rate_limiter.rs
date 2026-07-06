//! Per-tenant rate limiter — sliding window, in-memory.
//!
//! Rate limiting protects the LLM provider from abuse and controls costs.
//! Default: 30 requests per minute per tenant.
//!
//! Uses a simple sliding window with atomic counters.
//! For production, replace with a Redis/DragonflyDB-backed implementation.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Configuration for the rate limiter.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Max requests per window per tenant
    pub max_requests: u32,
    /// Window duration
    pub window: Duration,
    /// Ban duration for tenants that exceed the limit
    pub ban_duration: Duration,
    /// Maximum tracked tenants (evict oldest if exceeded)
    pub max_tenants: usize,
    /// Tenants exempt from rate limiting (e.g., admin)
    pub exempt_tenants: Vec<String>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 30,
            window: Duration::from_secs(60),
            ban_duration: Duration::from_secs(300), // 5 min ban
            max_tenants: 10000,
            exempt_tenants: vec![],
        }
    }
}

impl RateLimitConfig {
    /// Load from environment variables.
    pub fn from_env() -> Self {
        let max_requests = std::env::var("RATE_LIMIT_MAX_REQUESTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        let window_secs = std::env::var("RATE_LIMIT_WINDOW_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        let ban_secs = std::env::var("RATE_LIMIT_BAN_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);

        Self {
            max_requests,
            window: Duration::from_secs(window_secs),
            ban_duration: Duration::from_secs(ban_secs),
            max_tenants: 10000,
            exempt_tenants: vec![],
        }
    }
}

/// State for one tenant's rate limit window.
#[derive(Debug, Clone)]
struct TenantBucket {
    /// Unix timestamp (ms) of window start
    window_start: i64,
    /// Request count in current window
    count: u32,
    /// If banned, the Instant when the ban expires
    banned_until: Option<Instant>,
}

/// Per-tenant rate limiter.
pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: DashMap<String, TenantBucket>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given config.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: DashMap::new(),
        }
    }

    /// Check if a tenant is allowed to make a request.
    ///
    /// Returns `true` if allowed, `false` if rate limited.
    pub fn check(&self, tenant_id: &str) -> bool {
        // Exempt tenants
        if self.config.exempt_tenants.iter().any(|t| t == tenant_id) {
            return true;
        }

        let now = Instant::now();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let window_ms = self.config.window.as_millis() as i64;

        // Get or create bucket
        let mut entry = self.buckets.entry(tenant_id.to_string()).or_insert_with(|| {
            TenantBucket {
                window_start: now_ms,
                count: 0,
                banned_until: None,
            }
        });

        let bucket = entry.value_mut();

        // Check ban
        if let Some(banned_until) = bucket.banned_until {
            if now < banned_until {
                tracing::warn!(
                    tenant = %tenant_id,
                    "Rate limited (banned)"
                );
                return false;
            }
            // Ban expired
            bucket.banned_until = None;
        }

        // Check if window has expired — reset if so
        if now_ms - bucket.window_start > window_ms {
            bucket.window_start = now_ms;
            bucket.count = 0;
        }

        // Check limit
        if bucket.count >= self.config.max_requests {
            tracing::warn!(
                tenant = %tenant_id,
                count = bucket.count,
                max = self.config.max_requests,
                "Rate limited (exceeded)"
            );
            bucket.banned_until = Some(now + self.config.ban_duration);
            return false;
        }

        bucket.count += 1;
        true
    }

    /// Get remaining requests in the current window for a tenant.
    pub fn remaining(&self, tenant_id: &str) -> u32 {
        self.buckets
            .get(tenant_id)
            .map(|b| self.config.max_requests.saturating_sub(b.count))
            .unwrap_or(self.config.max_requests)
    }

    /// Reset rate limit for a specific tenant.
    pub fn reset(&self, tenant_id: &str) {
        self.buckets.remove(tenant_id);
    }

    /// Get the number of tracked tenants.
    pub fn tracked_tenants(&self) -> usize {
        self.buckets.len()
    }

    /// Evict stale entries (windows older than 2x the config window).
    pub fn evict_stale(&self) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let cutoff = now_ms - (self.config.window.as_millis() as i64 * 2);
        self.buckets.retain(|_, b| b.window_start > cutoff);
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_allows_requests() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_requests: 5,
            window: Duration::from_secs(60),
            ban_duration: Duration::from_secs(1),
            max_tenants: 100,
            exempt_tenants: vec![],
        });

        let tenant = "test_tenant";
        for _ in 0..5 {
            assert!(limiter.check(tenant), "Should allow request");
        }
        // 6th request should be blocked
        assert!(!limiter.check(tenant), "Should block after limit");
    }

    #[test]
    fn test_rate_limiter_exempt() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_requests: 1,
            window: Duration::from_secs(60),
            ban_duration: Duration::from_secs(1),
            max_tenants: 100,
            exempt_tenants: vec!["admin".into()],
        });

        // Admin is exempt
        for _ in 0..100 {
            assert!(limiter.check("admin"), "Admin should never be limited");
        }
    }

    #[test]
    fn test_remaining_counter() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_requests: 10,
            ..Default::default()
        });

        assert_eq!(limiter.remaining("test"), 10);
        limiter.check("test");
        assert_eq!(limiter.remaining("test"), 9);
    }
}
