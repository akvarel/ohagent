//! Usage tracker — records and analyzes model invocations.
//!
//! Tracks every model call: which model, what task, token usage,
//! duration, estimated cost. Stores in SQLite alongside other data.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{debug, info};

use crate::model_router::ModelCatalog;

/// A single usage record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: String,
    pub tenant_id: String,
    pub session_id: String,
    pub model_id: String,
    pub model_display: String,
    pub capabilities: String, // JSON array
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub duration_ms: u64,
    pub estimated_cost_usd: f64,
    pub created_at: DateTime<Utc>,
}

/// Aggregated usage stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    pub total_calls: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
    pub by_model: Vec<ModelStats>,
    pub by_day: Vec<DailyStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStats {
    pub model_id: String,
    pub model_display: String,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyStats {
    pub date: String,
    pub calls: u64,
    pub cost_usd: f64,
}

/// Persistent usage tracker.
pub struct UsageTracker {
    conn: Mutex<Connection>,
    catalog: Option<ModelCatalog>,
}

impl UsageTracker {
    /// Open or create the usage database.
    pub fn open(db_path: &str, catalog: Option<ModelCatalog>) -> Result<Self> {
        let path = PathBuf::from(shellexpand::tilde(db_path).as_ref());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        let tracker = Self {
            conn: Mutex::new(conn),
            catalog,
        };
        tracker.init_schema()?;
        info!(path = %db_path, "Usage tracker opened");
        Ok(tracker)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS usage_records (
                id              TEXT PRIMARY KEY,
                tenant_id       TEXT NOT NULL,
                session_id      TEXT NOT NULL,
                model_id        TEXT NOT NULL,
                model_display   TEXT NOT NULL,
                capabilities    TEXT NOT NULL DEFAULT '[]',
                input_tokens    INTEGER NOT NULL DEFAULT 0,
                output_tokens   INTEGER NOT NULL DEFAULT 0,
                duration_ms     INTEGER NOT NULL DEFAULT 0,
                estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
                created_at      TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_usage_tenant ON usage_records(tenant_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_records(model_id);",
        )?;
        Ok(())
    }

    /// Estimate cost based on model tier (rough heuristic).
    pub fn estimate_cost(&self, model_id: &str, input_tokens: u32, output_tokens: u32) -> f64 {
        // Per 1M tokens pricing (approximate, 2026 prices)
        let (input_price, output_price) = match model_id {
            // Low tier
            "deepseek-v4-flash" => (0.14, 0.28),
            "openai-gpt-4o-mini" => (0.15, 0.60),
            "claude-haiku-4-5" => (0.80, 4.00),
            // Medium tier
            "deepseek-v4" => (0.25, 0.50),
            "openai-gpt-4o" => (2.50, 10.00),
            "claude-sonnet-4-6" => (3.00, 15.00),
            // High tier
            "openai-o4-mini" => (1.10, 4.40),
            "claude-opus-4-5" => (15.00, 75.00),
            // Image
            "dall-e-3" => (40.00, 0.0), // flat rate per image
            "flux-1.1-pro" => (4.00, 0.0),
            // Video
            "kling-v1-6" => (20.00, 0.0),
            _ => (1.0, 3.0), // unknown
        };

        let input_cost = (input_tokens as f64 / 1_000_000.0) * input_price;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * output_price;
        (input_cost + output_cost).max(0.0001) // minimum 0.0001 to show it was tracked
    }

    /// Record a model invocation.
    pub fn record(
        &self,
        tenant_id: &str,
        session_id: &str,
        model_id: &str,
        model_display: &str,
        capabilities: &[String],
        input_tokens: u32,
        output_tokens: u32,
        duration_ms: u64,
    ) -> Result<()> {
        let cost = self.estimate_cost(model_id, input_tokens, output_tokens);
        let caps_json = serde_json::to_string(capabilities)?;

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO usage_records (id, tenant_id, session_id, model_id, model_display,
             capabilities, input_tokens, output_tokens, duration_ms, estimated_cost_usd, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                uuid::Uuid::new_v4().to_string(),
                tenant_id,
                session_id,
                model_id,
                model_display,
                caps_json,
                input_tokens,
                output_tokens,
                duration_ms,
                cost,
                Utc::now().to_rfc3339(),
            ],
        )?;

        debug!(
            model = %model_id,
            tokens_in = input_tokens,
            tokens_out = output_tokens,
            cost = cost,
            "Usage recorded"
        );

        Ok(())
    }

    /// Get aggregated stats for a tenant.
    pub fn stats(&self, tenant_id: &str) -> Result<UsageStats> {
        let conn = self.conn.lock().unwrap();

        // Total aggregates
        let total: (u64, u64, u64, f64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(input_tokens),0),
                    COALESCE(SUM(output_tokens),0), COALESCE(SUM(estimated_cost_usd),0)
             FROM usage_records WHERE tenant_id = ?1",
            params![tenant_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;

        // By model
        let mut by_model_stmt = conn.prepare(
            "SELECT model_id, model_display, COUNT(*), COALESCE(SUM(input_tokens),0),
                    COALESCE(SUM(output_tokens),0), COALESCE(SUM(estimated_cost_usd),0)
             FROM usage_records WHERE tenant_id = ?1
             GROUP BY model_id ORDER BY COUNT(*) DESC",
        )?;
        let by_model = by_model_stmt
            .query_map(params![tenant_id], |row| {
                Ok(ModelStats {
                    model_id: row.get(0)?,
                    model_display: row.get(1)?,
                    calls: row.get(2)?,
                    input_tokens: row.get(3)?,
                    output_tokens: row.get(4)?,
                    cost_usd: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // By day (last 30 days)
        let mut by_day_stmt = conn.prepare(
            "SELECT DATE(created_at) as day, COUNT(*), COALESCE(SUM(estimated_cost_usd),0)
             FROM usage_records WHERE tenant_id = ?1
             AND created_at >= DATE('now', '-30 days')
             GROUP BY day ORDER BY day DESC",
        )?;
        let by_day = by_day_stmt
            .query_map(params![tenant_id], |row| {
                Ok(DailyStats {
                    date: row.get(0)?,
                    calls: row.get(1)?,
                    cost_usd: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(UsageStats {
            total_calls: total.0,
            total_input_tokens: total.1,
            total_output_tokens: total.2,
            total_cost_usd: (total.3 * 10000.0).round() / 10000.0,
            by_model,
            by_day,
        })
    }

    /// List recent usage records for a tenant.
    pub fn recent(&self, tenant_id: &str, limit: usize) -> Result<Vec<UsageRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, session_id, model_id, model_display,
                    capabilities, input_tokens, output_tokens, duration_ms,
                    estimated_cost_usd, created_at
             FROM usage_records WHERE tenant_id = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![tenant_id, limit], |row| {
                Ok(UsageRecord {
                    id: row.get(0)?,
                    tenant_id: row.get(1)?,
                    session_id: row.get(2)?,
                    model_id: row.get(3)?,
                    model_display: row.get(4)?,
                    capabilities: row.get(5)?,
                    input_tokens: row.get(6)?,
                    output_tokens: row.get(7)?,
                    duration_ms: row.get(8)?,
                    estimated_cost_usd: row.get(9)?,
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(10)?)
                        .unwrap()
                        .with_timezone(&Utc),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_tracker() {
        let tracker = UsageTracker::open(":memory:", None).unwrap();
        tracker
            .record(
                "t1",
                "s1",
                "deepseek-v4-flash",
                "DeepSeek V4 Flash",
                &["coding".into()],
                1000,
                500,
                1200,
            )
            .unwrap();

        let stats = tracker.stats("t1").unwrap();
        assert_eq!(stats.total_calls, 1);
        assert!(stats.total_cost_usd > 0.0);

        let recent = tracker.recent("t1", 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].model_id, "deepseek-v4-flash");
    }

    #[test]
    fn test_cost_estimation() {
        let tracker = UsageTracker::open(":memory:", None).unwrap();
        let cost = tracker.estimate_cost("claude-opus-4-5", 1000, 1000);
        // Opus: $15/M in, $75/M out → ~$0.09 for 2K tokens
        assert!(cost > 0.05 && cost < 0.20, "Expected ~$0.09, got ${cost}");

        let cheap = tracker.estimate_cost("deepseek-v4-flash", 1000, 1000);
        // DeepSeek Flash: $0.14/M in, $0.28/M out → ~$0.00042
        assert!(cheap < 0.01, "Expected very cheap, got ${cheap}");
    }
}
