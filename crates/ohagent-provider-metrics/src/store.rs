//! SQLite storage for price records and speed benchmarks.

use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use crate::models::{PriceRecord, SpeedRecord, PricingModel};

pub struct MetricsStore {
    db: Mutex<Connection>,
}

impl MetricsStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("DB open: {e}"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS prices (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                model_id TEXT NOT NULL,
                pricing_model TEXT NOT NULL DEFAULT 'per_million_tokens',
                input_price_per_unit REAL NOT NULL,
                output_price_per_unit REAL NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                cached_input_price_per_unit REAL,
                context_window INTEGER,
                max_output_tokens INTEGER,
                capabilities TEXT NOT NULL DEFAULT '[]',
                scraped_at TEXT NOT NULL,
                source_url TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_prices_provider ON prices(provider);
            CREATE INDEX IF NOT EXISTS idx_prices_scraped ON prices(scraped_at);

            CREATE TABLE IF NOT EXISTS speeds (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                model_id TEXT NOT NULL,
                ttf_ms INTEGER NOT NULL,
                total_latency_ms INTEGER NOT NULL,
                tokens_per_second REAL NOT NULL,
                p95_latency_ms INTEGER NOT NULL,
                prompt_tokens INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                samples INTEGER NOT NULL DEFAULT 1,
                measured_at TEXT NOT NULL,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_speeds_provider ON speeds(provider);
            CREATE INDEX IF NOT EXISTS idx_speeds_measured ON speeds(measured_at);"
        ).map_err(|e| format!("DB schema: {e}"))?;
        Ok(Self { db: Mutex::new(conn) })
    }

    pub fn upsert_price(&self, record: &PriceRecord) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        let pm_str = serde_json::to_string(&record.pricing_model).unwrap_or_default().trim_matches('"').to_string();
        db.execute(
            "INSERT OR REPLACE INTO prices (id, provider, model_id, pricing_model, input_price_per_unit, output_price_per_unit,
             currency, cached_input_price_per_unit, context_window, max_output_tokens, capabilities, scraped_at, source_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                record.id, record.provider, record.model_id, pm_str,
                record.input_price_per_unit, record.output_price_per_unit,
                record.currency, record.cached_input_price_per_unit,
                record.context_window, record.max_output_tokens,
                serde_json::to_string(&record.capabilities).unwrap_or_default(),
                record.scraped_at.to_rfc3339(), record.source_url,
            ],
        ).map_err(|e| format!("insert price: {e}"))?;
        Ok(())
    }

    pub fn get_latest_prices(&self, provider: &str) -> Result<Vec<PriceRecord>, String> {
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        let mut stmt = db.prepare(
            "SELECT id, provider, model_id, pricing_model, input_price_per_unit, output_price_per_unit,
                    currency, cached_input_price_per_unit, context_window, max_output_tokens,
                    capabilities, scraped_at, source_url
             FROM prices WHERE provider = ?1 ORDER BY scraped_at DESC"
        ).map_err(|e| format!("prepare: {e}"))?;
        let records = stmt.query_map(params![provider], |row| { read_price(row) })
            .map_err(|e| format!("query: {e}"))?;
        records.collect::<Result<Vec<_>, _>>().map_err(|e| format!("collect: {e}"))
    }

    pub fn get_all_latest_prices(&self) -> Result<Vec<PriceRecord>, String> {
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        let mut stmt = db.prepare(
            "SELECT id, provider, model_id, pricing_model, input_price_per_unit, output_price_per_unit,
                    currency, cached_input_price_per_unit, context_window, max_output_tokens,
                    capabilities, MAX(scraped_at) as scraped_at, source_url
             FROM prices GROUP BY provider, model_id ORDER BY provider, model_id"
        ).map_err(|e| format!("prepare: {e}"))?;
        let records = stmt.query_map([], |row| { read_price(row) })
            .map_err(|e| format!("query: {e}"))?;
        records.collect::<Result<Vec<_>, _>>().map_err(|e| format!("collect: {e}"))
    }

    pub fn upsert_speed(&self, record: &SpeedRecord) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        db.execute(
            "INSERT OR REPLACE INTO speeds (id, provider, model_id, ttf_ms, total_latency_ms,
             tokens_per_second, p95_latency_ms, prompt_tokens, completion_tokens, samples, measured_at, error)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![record.id, record.provider, record.model_id,
                record.ttf_ms, record.total_latency_ms, record.tokens_per_second,
                record.p95_latency_ms, record.prompt_tokens, record.completion_tokens,
                record.samples, record.measured_at.to_rfc3339(), record.error,
            ],
        ).map_err(|e| format!("insert speed: {e}"))?;
        Ok(())
    }

    pub fn get_speeds(&self, provider: &str, model_id: &str) -> Result<Vec<SpeedRecord>, String> {
        let db = self.db.lock().map_err(|e| format!("Lock: {e}"))?;
        let mut stmt = db.prepare(
            "SELECT id, provider, model_id, ttf_ms, total_latency_ms, tokens_per_second,
                    p95_latency_ms, prompt_tokens, completion_tokens, samples, measured_at, error
             FROM speeds WHERE provider = ?1 AND model_id = ?2 ORDER BY measured_at DESC LIMIT 5"
        ).map_err(|e| format!("prepare: {e}"))?;
        let records = stmt.query_map(params![provider, model_id], |row| {
            Ok(SpeedRecord {
                id: row.get(0)?, provider: row.get(1)?, model_id: row.get(2)?,
                ttf_ms: row.get(3)?, total_latency_ms: row.get(4)?, tokens_per_second: row.get(5)?,
                p95_latency_ms: row.get(6)?, prompt_tokens: row.get(7)?, completion_tokens: row.get(8)?,
                samples: row.get(9)?,
                measured_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(10)?).unwrap().with_timezone(&Utc),
                error: row.get(11)?,
            })
        }).map_err(|e| format!("query: {e}"))?;
        records.collect::<Result<Vec<_>, _>>().map_err(|e| format!("collect: {e}"))
    }
}

fn read_price(row: &rusqlite::Row) -> rusqlite::Result<PriceRecord> {
    let pm_str: String = row.get(3)?;
    let pricing_model: PricingModel = serde_json::from_str(&format!("\"{}\"", pm_str)).unwrap_or(PricingModel::PerMillionTokens);
    let caps_str: String = row.get(10)?;
    let capabilities: Vec<String> = serde_json::from_str(&caps_str).unwrap_or_default();
    Ok(PriceRecord {
        id: row.get(0)?, provider: row.get(1)?, model_id: row.get(2)?,
        pricing_model,
        input_price_per_unit: row.get(4)?, output_price_per_unit: row.get(5)?,
        currency: row.get(6)?, cached_input_price_per_unit: row.get(7)?,
        context_window: row.get(8)?, max_output_tokens: row.get(9)?,
        capabilities,
        scraped_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(11)?).unwrap().with_timezone(&Utc),
        source_url: row.get(12)?,
    })
}
