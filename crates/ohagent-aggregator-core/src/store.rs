//! SQLite store — shared between API key manager and billing tracker.

use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct AggregatorStore {
    db: Arc<Mutex<Connection>>,
}

impl AggregatorStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("DB open: {e}"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS api_keys (
                id TEXT PRIMARY KEY, prefix TEXT NOT NULL, key_hash TEXT NOT NULL UNIQUE,
                customer_id TEXT NOT NULL, tier TEXT NOT NULL DEFAULT 'free',
                monthly_token_limit INTEGER NOT NULL DEFAULT 0, active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL, last_used_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_keys_hash ON api_keys(key_hash);
            CREATE INDEX IF NOT EXISTS idx_keys_customer ON api_keys(customer_id);
            CREATE TABLE IF NOT EXISTS usage_records (
                id TEXT PRIMARY KEY, api_key_id TEXT NOT NULL, customer_id TEXT NOT NULL,
                provider TEXT NOT NULL, model_id TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL DEFAULT 0, completion_tokens INTEGER NOT NULL DEFAULT 0,
                our_cost_eur REAL NOT NULL DEFAULT 0.0, customer_cost_eur REAL NOT NULL DEFAULT 0.0,
                margin_eur REAL NOT NULL DEFAULT 0.0, timestamp TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_usage_key ON usage_records(api_key_id, timestamp);
            CREATE INDEX IF NOT EXISTS idx_usage_customer ON usage_records(customer_id, timestamp);"
        ).map_err(|e| format!("DB schema: {e}"))?;
        Ok(Self { db: Arc::new(Mutex::new(conn)) })
    }

    pub fn db(&self) -> Arc<Mutex<Connection>> { Arc::clone(&self.db) }
}
