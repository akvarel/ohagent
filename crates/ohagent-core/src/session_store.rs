//! Persistent session store — survives daemon restarts.
//!
//! ## Problem
//!
//! When the daemon restarts, all in-memory conversation state is lost:
//! recent messages, token counts, rolling summary state. Users must
//! start fresh conversations.
//!
//! ## Solution
//!
//! `SessionStore` persists active session metadata to SQLite so that
//! on restart, the daemon can:
//! 1. List active sessions (tenant + session_hash)
//! 2. Load recent messages from the message log
//! 3. Load the rolling summary (already persisted by MemoryStore)
//! 4. Rebuild the layered system prompt and continue the conversation
//!
//! ## Schema
//!
//! ```sql
//! CREATE TABLE active_sessions (
//!     tenant_id     TEXT NOT NULL,
//!     session_hash  TEXT NOT NULL,
//!     last_activity TEXT NOT NULL,  -- ISO8601
//!     message_count INTEGER NOT NULL DEFAULT 0,
//!     total_tokens  INTEGER NOT NULL DEFAULT 0,
//!     project_dir   TEXT NOT NULL DEFAULT '.',
//!     PRIMARY KEY (tenant_id, session_hash)
//! );
//! ```

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use std::sync::Mutex;

/// Active session metadata persisted across daemon restarts.
#[derive(Debug, Clone)]
pub struct ActiveSession {
    pub tenant_id: String,
    pub session_hash: String,
    pub last_activity: DateTime<Utc>,
    pub message_count: u32,
    pub total_tokens: u64,
    pub project_dir: String,
}

/// Persistent session state store backed by SQLite.
pub struct SessionStore {
    db: Mutex<rusqlite::Connection>,
}

impl SessionStore {
    /// Open (or create) the session store database.
    pub fn open(path: &str) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS active_sessions (
                tenant_id     TEXT NOT NULL,
                session_hash  TEXT NOT NULL,
                last_activity TEXT NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0,
                total_tokens  INTEGER NOT NULL DEFAULT 0,
                project_dir   TEXT NOT NULL DEFAULT '.',
                PRIMARY KEY (tenant_id, session_hash)
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_activity
                ON active_sessions(last_activity);"
        )?;
        Ok(Self { db: Mutex::new(conn) })
    }

    /// Record activity for a session (upsert). Call on every message exchange.
    pub fn heartbeat(
        &self,
        tenant_id: &str,
        session_hash: &str,
        message_count: u32,
        total_tokens: u64,
        project_dir: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO active_sessions (tenant_id, session_hash, last_activity, message_count, total_tokens, project_dir)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(tenant_id, session_hash) DO UPDATE SET
                last_activity = excluded.last_activity,
                message_count = excluded.message_count,
                total_tokens   = excluded.total_tokens,
                project_dir    = excluded.project_dir",
            params![tenant_id, session_hash, now, message_count, total_tokens, project_dir],
        )?;
        Ok(())
    }

    /// List all active sessions, most recent first.
    pub fn list_active(&self) -> Result<Vec<ActiveSession>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT tenant_id, session_hash, last_activity, message_count, total_tokens, project_dir
             FROM active_sessions
             ORDER BY last_activity DESC"
        )?;
        let sessions = stmt.query_map([], |row| {
            let last_str: String = row.get(2)?;
            Ok(ActiveSession {
                tenant_id: row.get(0)?,
                session_hash: row.get(1)?,
                last_activity: DateTime::parse_from_rfc3339(&last_str)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                message_count: row.get(3)?,
                total_tokens: row.get(4)?,
                project_dir: row.get(5)?,
            })
        })?.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    /// Get a single session by tenant + hash.
    pub fn get(&self, tenant_id: &str, session_hash: &str) -> Result<Option<ActiveSession>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT tenant_id, session_hash, last_activity, message_count, total_tokens, project_dir
             FROM active_sessions
             WHERE tenant_id = ?1 AND session_hash = ?2"
        )?;
        let mut rows = stmt.query_map(params![tenant_id, session_hash], |row| {
            let last_str: String = row.get(2)?;
            Ok(ActiveSession {
                tenant_id: row.get(0)?,
                session_hash: row.get(1)?,
                last_activity: DateTime::parse_from_rfc3339(&last_str)
                    .unwrap_or_default()
                    .with_timezone(&Utc),
                message_count: row.get(3)?,
                total_tokens: row.get(4)?,
                project_dir: row.get(5)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Delete a session (e.g. user typed /new).
    pub fn delete_session(&self, tenant_id: &str, session_hash: &str) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM active_sessions WHERE tenant_id = ?1 AND session_hash = ?2",
            params![tenant_id, session_hash],
        )?;
        Ok(())
    }

    /// Delete all sessions for a tenant.
    pub fn delete_all_for_tenant(&self, tenant_id: &str) -> Result<usize> {
        let db = self.db.lock().unwrap();
        let count = db.execute(
            "DELETE FROM active_sessions WHERE tenant_id = ?1",
            params![tenant_id],
        )?;
        Ok(count)
    }

    /// Stale cleanup: remove sessions inactive for more than `max_age_days`.
    pub fn cleanup_stale(&self, max_age_days: i64) -> Result<usize> {
        let cutoff = Utc::now() - chrono::Duration::days(max_age_days);
        let cutoff_str = cutoff.to_rfc3339();
        let db = self.db.lock().unwrap();
        let count = db.execute(
            "DELETE FROM active_sessions WHERE last_activity < ?1",
            params![cutoff_str],
        )?;
        if count > 0 {
            tracing::info!(count, max_age_days, "Cleaned up stale sessions");
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> SessionStore {
        SessionStore::open(":memory:").unwrap()
    }

    #[test]
    fn test_heartbeat_and_list() {
        let store = temp_db();
        store.heartbeat("t1", "s1", 5, 1000, ".").unwrap();
        store.heartbeat("t1", "s2", 3, 500, "/tmp").unwrap();
        store.heartbeat("t2", "s1", 1, 100, ".").unwrap();

        let sessions = store.list_active().unwrap();
        assert_eq!(sessions.len(), 3);
    }

    #[test]
    fn test_heartbeat_upsert() {
        let store = temp_db();
        store.heartbeat("t1", "s1", 5, 1000, ".").unwrap();
        store.heartbeat("t1", "s1", 10, 2000, "/proj").unwrap();

        let s = store.get("t1", "s1").unwrap().unwrap();
        assert_eq!(s.message_count, 10);
        assert_eq!(s.total_tokens, 2000);
        assert_eq!(s.project_dir, "/proj");
    }

    #[test]
    fn test_delete_and_get() {
        let store = temp_db();
        store.heartbeat("t1", "s1", 1, 100, ".").unwrap();
        assert!(store.get("t1", "s1").unwrap().is_some());

        store.delete_session("t1", "s1").unwrap();
        assert!(store.get("t1", "s1").unwrap().is_none());
    }

    #[test]
    fn test_delete_all_for_tenant() {
        let store = temp_db();
        store.heartbeat("t1", "s1", 1, 100, ".").unwrap();
        store.heartbeat("t1", "s2", 1, 100, ".").unwrap();
        store.heartbeat("t2", "s1", 1, 100, ".").unwrap();

        let deleted = store.delete_all_for_tenant("t1").unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(store.list_active().unwrap().len(), 1);
    }

    #[test]
    fn test_cleanup_stale() {
        let store = temp_db();
        store.heartbeat("t1", "s1", 1, 100, ".").unwrap();

        // Insert a stale session directly with an old timestamp
        {
            let db = store.db.lock().unwrap();
            db.execute(
                "INSERT OR REPLACE INTO active_sessions (tenant_id, session_hash, last_activity, message_count, total_tokens, project_dir)
                 VALUES ('stale', 's1', '2020-01-01T00:00:00+00:00', 1, 0, '.')",
                [],
            ).unwrap();
        }

        let cleaned = store.cleanup_stale(30).unwrap();
        assert_eq!(cleaned, 1);
        let remaining = store.list_active().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].tenant_id, "t1");
    }
}
