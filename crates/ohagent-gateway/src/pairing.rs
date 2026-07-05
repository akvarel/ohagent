//! User pairing and authorization for gateway platforms.
//!
//! Before a user can interact with ohAgent, they must be paired.
//! Pairing creates a tenant-scoped identity and records the mapping
//! from platform user IDs to tenant IDs.

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use rand::Rng;

/// Represents a paired user across any platform.
#[derive(Debug, Clone)]
pub struct PairedUser {
    /// Platform-scoped user ID.
    pub user_id: String,
    /// Platform name (e.g. "telegram").
    pub platform: String,
    /// Tenant identifier.
    pub tenant_id: String,
    /// When the user was paired.
    pub paired_at: DateTime<Utc>,
    /// Language preference.
    pub lang: String,
}

/// A pending pairing code.
#[derive(Debug, Clone)]
struct PendingPairing {
    user_id: String,
    platform: String,
    code: String,
    expires_at: DateTime<Utc>,
}

/// Manages pairing codes and paired user registries.
pub struct PairingManager {
    /// Active pairing codes awaiting confirmation.
    pending: DashMap<String, PendingPairing>,
    /// Confirmed paired users.
    paired: DashMap<String, PairedUser>,
}

impl PairingManager {
    pub fn new() -> Self {
        Self {
            pending: DashMap::new(),
            paired: DashMap::new(),
        }
    }

    /// Generate a new pairing code for a user.
    ///
    /// Returns a 6-digit code valid for 10 minutes.
    pub fn generate_code(&self, user_id: &str, platform: &str) -> String {
        // Remove any existing pending code for this user.
        self.pending.remove(user_id);

        let code: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(6)
            .map(char::from)
            .collect::<String>()
            .to_uppercase();

        self.pending.insert(
            user_id.to_string(),
            PendingPairing {
                user_id: user_id.to_string(),
                platform: platform.to_string(),
                code: code.clone(),
                expires_at: Utc::now() + Duration::minutes(10),
            },
        );

        code
    }

    /// Confirm a pairing code.
    ///
    /// If valid, moves the user from pending to paired and returns the PairedUser.
    pub fn confirm_code(
        &self,
        user_id: &str,
        code: &str,
        lang: &str,
    ) -> Result<PairedUser, String> {
        let entry = self
            .pending
            .get(user_id)
            .ok_or_else(|| "No pairing request found. Use /pair first.".to_string())?;

        if entry.expires_at < Utc::now() {
            self.pending.remove(user_id);
            return Err("Pairing code expired. Use /pair again.".to_string());
        }

        if entry.code != code.to_uppercase() {
            return Err("Invalid pairing code. Check and try again.".to_string());
        }

        // Generate tenant ID from platform + user_id
        let tenant_id = format!("{}_{}", entry.platform, entry.user_id);

        let paired_user = PairedUser {
            user_id: entry.user_id.clone(),
            platform: entry.platform.clone(),
            tenant_id: tenant_id.clone(),
            paired_at: Utc::now(),
            lang: lang.to_string(),
        };

        self.paired.insert(user_id.to_string(), paired_user.clone());

        // Clean up pending
        self.pending.remove(user_id);

        Ok(paired_user)
    }

    /// Check if a user is paired.
    pub fn is_paired(&self, user_id: &str) -> bool {
        self.paired.contains_key(user_id)
    }

    /// Get a paired user by their platform user ID.
    pub fn get(&self, user_id: &str) -> Option<PairedUser> {
        self.paired.get(user_id).map(|entry| entry.clone())
    }

    /// Get the tenant ID for a paired user.
    pub fn tenant_id(&self, user_id: &str) -> Option<String> {
        self.paired
            .get(user_id)
            .map(|entry| entry.tenant_id.clone())
    }

    /// Remove a paired user (unpair).
    pub fn unpair(&self, user_id: &str) {
        self.paired.remove(user_id);
        self.pending.remove(user_id);
    }

    /// Number of paired users.
    pub fn paired_count(&self) -> usize {
        self.paired.len()
    }
}

impl Default for PairingManager {
    fn default() -> Self {
        Self::new()
    }
}
