//! User pairing and authorization for gateway platforms.
//!
//! Before a user can interact with ohAgent, they must be paired.
//!
//! ## Security model
//!
//! Only the **admin** (owner) can generate pairing codes. This prevents
//! self-pairing: a random user cannot `/pair` → `/confirm` and gain access.
//!
//! Flow:
//! 1. Admin sends `/pair` → gets a 6-char code valid for 10 minutes
//! 2. Admin shares the code with the intended user (verbally, message, etc.)
//! 3. User sends `/confirm <code>` → paired, tenant created

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

/// A pending pairing code — NOT tied to a specific user.
/// Any user can redeem it.
#[derive(Debug, Clone)]
struct PendingCode {
    code: String,
    created_by: String,
    expires_at: DateTime<Utc>,
}

/// Manages pairing codes and paired user registries.
///
/// Only the admin (owner) can generate codes. Any user with a valid
/// code can confirm it.
pub struct PairingManager {
    /// Active pairing codes (code → PendingCode).
    pending: DashMap<String, PendingCode>,
    /// Confirmed paired users (user_id → PairedUser).
    paired: DashMap<String, PairedUser>,
    /// The admin user ID — only this user can generate codes.
    admin_user_id: String,
}

impl PairingManager {
    pub fn new(admin_user_id: String) -> Self {
        Self {
            pending: DashMap::new(),
            paired: DashMap::new(),
            admin_user_id,
        }
    }

    /// Whether the given user is the admin.
    pub fn is_admin(&self, user_id: &str) -> bool {
        user_id == self.admin_user_id
    }

    /// Generate a new pairing code. Only callable by the admin.
    ///
    /// Returns a 6-character uppercase code valid for 10 minutes.
    /// The code is NOT tied to a specific user — anyone can redeem it.
    pub fn generate_code(&self, admin_user_id: &str) -> Result<String, String> {
        if admin_user_id != self.admin_user_id {
            return Err("Only the admin can generate pairing codes.".into());
        }

        let code: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(6)
            .map(char::from)
            .collect::<String>()
            .to_uppercase();

        self.pending.insert(
            code.clone(),
            PendingCode {
                code: code.clone(),
                created_by: admin_user_id.to_string(),
                expires_at: Utc::now() + Duration::minutes(10),
            },
        );

        Ok(code)
    }

    /// Confirm a pairing code.
    ///
    /// Any user with a valid code can use it. On success, creates a
    /// tenant-scoped identity and returns the PairedUser.
    pub fn confirm_code(
        &self,
        user_id: &str,
        platform: &str,
        code: &str,
        lang: &str,
    ) -> Result<PairedUser, String> {
        let entry = self
            .pending
            .get(&code.to_uppercase())
            .ok_or_else(|| "Invalid or expired pairing code.".to_string())?;

        if entry.expires_at < Utc::now() {
            self.pending.remove(&code.to_uppercase());
            return Err("Pairing code expired. Ask the admin for a new one.".into());
        }

        // Code is valid — create tenant and pair
        let tenant_id = format!("{}_{}", platform, user_id);

        let paired_user = PairedUser {
            user_id: user_id.to_string(),
            platform: platform.to_string(),
            tenant_id: tenant_id.clone(),
            paired_at: Utc::now(),
            lang: lang.to_string(),
        };

        self.paired.insert(user_id.to_string(), paired_user.clone());

        // One-time use: remove the code
        self.pending.remove(&code.to_uppercase());

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
    }

    /// Number of paired users.
    pub fn paired_count(&self) -> usize {
        self.paired.len()
    }

    /// Update a paired user's language preference.
    pub fn update_lang(&self, user_id: &str, lang: &str) -> bool {
        if let Some(mut user) = self.paired.get_mut(user_id) {
            user.lang = lang.to_string();
            true
        } else {
            false
        }
    }
}
