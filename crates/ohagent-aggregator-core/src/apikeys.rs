//! API key management — generation, validation, tier assignment.

use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub prefix: String,
    pub key_hash: String,
    pub customer_id: String,
    pub tier: MarkupTier,
    pub monthly_token_limit: u64,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MarkupTier {
    Free,
    Starter,
    Pro,
    Enterprise,
}

impl MarkupTier {
    pub fn markup(&self) -> f64 {
        match self {
            MarkupTier::Free => 0.0,
            MarkupTier::Starter => 1.20,
            MarkupTier::Pro => 1.30,
            MarkupTier::Enterprise => 1.15,
        }
    }
    pub fn daily_token_limit(&self) -> u64 {
        match self {
            MarkupTier::Free => 1000,
            MarkupTier::Starter => 100_000,
            MarkupTier::Pro => 1_000_000,
            MarkupTier::Enterprise => 0,
        }
    }
    pub fn monthly_cost_eur(&self) -> f64 {
        match self {
            MarkupTier::Free => 0.0,
            MarkupTier::Starter => 19.0,
            MarkupTier::Pro => 99.0,
            MarkupTier::Enterprise => 499.0,
        }
    }
}

pub struct ApiKeyManager {
    db: Arc<Mutex<rusqlite::Connection>>,
}

impl ApiKeyManager {
    pub fn new(db: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self { db }
    }

    pub fn generate(
        &self,
        customer_id: &str,
        tier: MarkupTier,
    ) -> Result<(String, String), String> {
        let id = uuid::Uuid::new_v4().to_string();
        let raw = format!("ohag-{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
        let prefix = raw[..12].to_string();
        use sha2::{Digest, Sha256};
        let hash = hex::encode(Sha256::digest(raw.as_bytes()));
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        db.execute(
            "INSERT INTO api_keys (id, prefix, key_hash, customer_id, tier, monthly_token_limit, active, created_at) VALUES (?1,?2,?3,?4,?5,?6,1,?7)",
            params![id, prefix, hash, customer_id, serde_json::to_string(&tier).unwrap(), tier.daily_token_limit(), Utc::now().to_rfc3339()],
        ).map_err(|e| format!("insert: {e}"))?;
        Ok((raw, prefix))
    }

    pub fn validate(&self, key: &str) -> Result<ApiKey, String> {
        if !key.starts_with("ohag-") {
            return Err("Invalid key format".into());
        }
        use sha2::{Digest, Sha256};
        let hash = hex::encode(Sha256::digest(key.as_bytes()));
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        let mut stmt = db.prepare("SELECT id,prefix,key_hash,customer_id,tier,monthly_token_limit,active,created_at,last_used_at FROM api_keys WHERE key_hash=?1 AND active=1")
            .map_err(|e| format!("prep: {e}"))?;
        let result = stmt
            .query_row(params![hash], |row| {
                let ts: String = row.get(4)?;
                Ok(ApiKey {
                    id: row.get(0)?,
                    prefix: row.get(1)?,
                    key_hash: row.get(2)?,
                    customer_id: row.get(3)?,
                    tier: serde_json::from_str(&ts).unwrap_or(MarkupTier::Free),
                    monthly_token_limit: row.get(5)?,
                    active: row.get(6)?,
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    last_used_at: row.get::<_, Option<String>>(8)?.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|d| d.with_timezone(&Utc))
                    }),
                })
            })
            .map_err(|_| "Invalid or inactive API key".to_string())?;
        db.execute(
            "UPDATE api_keys SET last_used_at=?1 WHERE id=?2",
            params![Utc::now().to_rfc3339(), result.id],
        )
        .ok();
        Ok(result)
    }

    pub fn revoke(&self, key_id: &str) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        db.execute("UPDATE api_keys SET active=0 WHERE id=?1", params![key_id])
            .map_err(|e| format!("revoke: {e}"))?;
        Ok(())
    }
}
