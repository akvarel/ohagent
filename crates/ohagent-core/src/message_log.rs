//! Message log — stores LLM prompts and responses with gzip compression.
//!
//! ## Design
//!
//! Each `complete()` call stores the full message array (prompt) as a gzipped
//! JSON blob. The assistant response (collected from `TextDelta` events) is
//! stored as a separate row.
//!
//! Messages are grouped by `session_hash` (derived from the first messages
//! of the conversation) for easy reconstruction.
//!
//! ## Compression
//!
//! All content is gzip-compressed before storage to save space.

use std::sync::Mutex;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// A single message stored in the log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggedMessage {
    pub id: String,
    pub tenant_id: String,
    pub session_hash: String,
    pub role: String,        // "user" for prompts, "assistant" for responses
    pub turn_seq: u32,
    pub content_json: String, // decompressed JSON
    pub token_estimate: u32,
    pub archived: bool,
    pub created_at: String,
}

/// Full message log engine.
pub struct MessageLog {
    db: Mutex<rusqlite::Connection>,
    enabled: Mutex<bool>,
}

impl MessageLog {
    /// Open (or create) the message log database.
    pub fn open(path: &str) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS message_log (
                id              TEXT PRIMARY KEY,
                tenant_id       TEXT NOT NULL,
                session_hash    TEXT NOT NULL,
                role            TEXT NOT NULL,
                turn_seq        INTEGER NOT NULL,
                content_gz      BLOB NOT NULL,
                token_estimate  INTEGER NOT NULL DEFAULT 0,
                archived        INTEGER NOT NULL DEFAULT 0,
                created_at      TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_msglog_tenant ON message_log(tenant_id, archived);
            CREATE INDEX IF NOT EXISTS idx_msglog_session ON message_log(session_hash, turn_seq);
            CREATE TABLE IF NOT EXISTS tenant_logging_prefs (
                tenant_id   TEXT PRIMARY KEY,
                enabled     INTEGER NOT NULL DEFAULT 1
            );"
        )?;
        Ok(Self {
            db: Mutex::new(conn),
            enabled: Mutex::new(true),
        })
    }

    /// Whether logging is enabled for a tenant.
    pub fn is_enabled_for(&self, tenant_id: &str) -> bool {
        if !*self.enabled.lock().unwrap() { return false; }
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT enabled FROM tenant_logging_prefs WHERE tenant_id = ?1",
            params![tenant_id],
            |row| row.get::<_, i32>(0),
        )
        .map(|v| v != 0)
        .unwrap_or(true)
    }

    /// Enable or disable logging for a tenant.
    pub fn set_enabled(&self, tenant_id: &str, enabled: bool) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO tenant_logging_prefs (tenant_id, enabled) VALUES (?1, ?2)
             ON CONFLICT(tenant_id) DO UPDATE SET enabled = ?2",
            params![tenant_id, enabled as i32],
        )?;
        Ok(())
    }

    /// Gzip-compress a string.
    fn gzip(data: &str) -> Result<Vec<u8>> {
        let mut e = GzEncoder::new(Vec::new(), Compression::best());
        use std::io::Write;
        e.write_all(data.as_bytes())?;
        e.finish().map_err(|e| anyhow::anyhow!("gzip: {e}"))
    }

    /// Gzip-decompress.
    fn gunzip(data: &[u8]) -> Result<String> {
        let mut d = GzDecoder::new(data);
        use std::io::Read;
        let mut s = String::new();
        d.read_to_string(&mut s)?;
        Ok(s)
    }

    /// Log a set of messages (the prompt sent to the LLM).
    ///
    /// Stores the full message array as a gzipped JSON blob.
    pub fn log_messages(
        &self,
        tenant_id: &str,
        session_hash: &str,
        role: &str,
        turn_seq: u32,
        messages: &[serde_json::Value],
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let json = serde_json::to_string(messages)?;
        let tokens = (json.len() / 4).max(1) as u32;
        let gz = Self::gzip(&json)?;

        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO message_log
             (id, tenant_id, session_hash, role, turn_seq, content_gz, token_estimate, archived, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,0,?8)",
            params![id, tenant_id, session_hash, role, turn_seq, gz, tokens, now],
        )?;

        tracing::debug!(
            tenant = %tenant_id,
            session = %session_hash,
            turn = turn_seq,
            role = %role,
            tokens = tokens,
            "Message logged"
        );

        Ok(id)
    }

    /// List message log entries for a tenant (paginated, most recent first).
    pub fn list(
        &self,
        tenant_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<LoggedMessage>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, tenant_id, session_hash, role, turn_seq,
                    content_gz, token_estimate, archived, created_at
             FROM message_log
             WHERE tenant_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2 OFFSET ?3",
        )?;

        let rows = stmt
            .query_map(params![tenant_id, limit, offset], |row| {
                let gz: Vec<u8> = row.get(5)?;
                let json = Self::gunzip(&gz).unwrap_or_else(|_| "[compressed]".into());
                Ok(LoggedMessage {
                    id: row.get(0)?,
                    tenant_id: row.get(1)?,
                    session_hash: row.get(2)?,
                    role: row.get(3)?,
                    turn_seq: row.get(4)?,
                    content_json: json,
                    token_estimate: row.get(6)?,
                    archived: row.get::<_, i32>(7)? != 0,
                    created_at: row.get(8)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Mark entries as archived (after S3 upload).
    pub fn mark_archived(&self, ids: &[String]) -> Result<()> {
        let db = self.db.lock().unwrap();
        for id in ids {
            db.execute(
                "UPDATE message_log SET archived = 1 WHERE id = ?1",
                params![id],
            )?;
        }
        Ok(())
    }

    /// Delete archived entries older than `days`.
    pub fn cleanup_archived(&self, older_than_days: i32) -> Result<usize> {
        let db = self.db.lock().unwrap();
        let cutoff = chrono::Utc::now() - chrono::Duration::days(older_than_days as i64);
        let deleted = db.execute(
            "DELETE FROM message_log WHERE archived = 1 AND created_at < ?1",
            params![cutoff.to_rfc3339()],
        )?;
        Ok(deleted)
    }

    /// Get entries ready for archiving (not archived, older than `days`).
    pub fn ready_for_archive(&self, older_than_days: i32) -> Result<Vec<LoggedMessage>> {
        let db = self.db.lock().unwrap();
        let cutoff = chrono::Utc::now() - chrono::Duration::days(older_than_days as i64);
        let mut stmt = db.prepare(
            "SELECT id, tenant_id, session_hash, role, turn_seq,
                    content_gz, token_estimate, archived, created_at
             FROM message_log
             WHERE archived = 0 AND created_at < ?1
             ORDER BY tenant_id, session_hash, turn_seq",
        )?;

        let rows = stmt
            .query_map(params![cutoff.to_rfc3339()], |row| {
                let gz: Vec<u8> = row.get(5)?;
                let json = Self::gunzip(&gz).unwrap_or_else(|_| "[compressed]".into());
                Ok(LoggedMessage {
                    id: row.get(0)?,
                    tenant_id: row.get(1)?,
                    session_hash: row.get(2)?,
                    role: row.get(3)?,
                    turn_seq: row.get(4)?,
                    content_json: json,
                    token_estimate: row.get(6)?,
                    archived: row.get::<_, i32>(7)? != 0,
                    created_at: row.get(8)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Global enable/disable.
    pub fn set_global_enabled(&self, enabled: bool) {
        *self.enabled.lock().unwrap() = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gzip_roundtrip() {
        let data = r#"{"role":"user","content":"hello world"}"#;
        let gz = MessageLog::gzip(data).unwrap();
        assert!(gz.len() < data.len() + 40);
        let back = MessageLog::gunzip(&gz).unwrap();
        assert_eq!(data, back);
    }

    #[test]
    fn test_open_and_log() {
        let log = MessageLog::open(":memory:").unwrap();
        assert!(log.is_enabled_for("t1"));

        let msgs = vec![
            serde_json::json!({"role":"system","content":"You are helpful."}),
            serde_json::json!({"role":"user","content":"Hello"}),
        ];
        let id = log.log_messages("t1", "s1", "user", 1, &msgs).unwrap();
        assert!(!id.is_empty());

        let entries = log.list("t1", 10, 0).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_multiple_turns() {
        let log = MessageLog::open(":memory:").unwrap();

        log.log_messages("t1", "s1", "user", 1,
            &[serde_json::json!({"role":"user","content":"q1"})]).unwrap();
        log.log_messages("t1", "s1", "assistant", 1,
            &[serde_json::json!({"role":"assistant","content":"a1"})]).unwrap();
        log.log_messages("t1", "s1", "user", 2,
            &[serde_json::json!({"role":"user","content":"q2"})]).unwrap();

        let entries = log.list("t1", 10, 0).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_tenant_prefs() {
        let log = MessageLog::open(":memory:").unwrap();
        assert!(log.is_enabled_for("t1"));
        log.set_enabled("t1", false).unwrap();
        assert!(!log.is_enabled_for("t1"));
        log.set_enabled("t1", true).unwrap();
        assert!(log.is_enabled_for("t1"));
    }

    #[test]
    fn test_archive_flow() {
        let log = MessageLog::open(":memory:").unwrap();

        let id = log.log_messages("t1", "s1", "user", 1,
            &[serde_json::json!({"role":"user","content":"old"})]).unwrap();

        // Entries older than 0 days should be ready
        let ready = log.ready_for_archive(0).unwrap();
        assert!(!ready.is_empty());

        log.mark_archived(&[id]).unwrap();

        let list = log.list("t1", 10, 0).unwrap();
        assert!(list[0].archived);
    }
}
