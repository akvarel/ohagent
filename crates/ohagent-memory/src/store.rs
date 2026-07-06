//! SQLite-backed memory store.
//!
//! Stores memory entries, embeddings, and conversation summaries.
//! Schema: memory_entries + memory_embeddings + conversation_summaries.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{debug, info};

use crate::models::{ConversationSummary, MemoryConfig, MemoryEntry, MemorySource, RollingSummary, TopicRef};
use crate::Result;

/// The memory store — thread-safe wrapper around an SQLite connection.
pub struct MemoryStore {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    config: MemoryConfig,
}

impl MemoryStore {
    /// Open or create the memory database.
    pub fn open(config: MemoryConfig) -> Result<Self> {
        let db_path = shellexpand::tilde(&config.db_path).to_string();
        let path = PathBuf::from(&db_path);

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path)?;

        // Enable WAL mode for concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        let store = Self {
            conn: Mutex::new(conn),
            config,
        };
        store.init_schema()?;
        info!(path = %db_path, "Memory store opened");
        Ok(store)
    }

    /// Create tables if they don't exist.
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS memory_entries (
                id              TEXT PRIMARY KEY,
                tenant_id       TEXT NOT NULL,
                session_id      TEXT NOT NULL,
                content         TEXT NOT NULL,
                source          TEXT NOT NULL DEFAULT 'conversation',
                importance      REAL NOT NULL DEFAULT 0.5,
                tags            TEXT NOT NULL DEFAULT '[]',
                created_at      TEXT NOT NULL,
                last_accessed_at TEXT NOT NULL,
                access_count    INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS memory_embeddings (
                entry_id        TEXT PRIMARY KEY REFERENCES memory_entries(id) ON DELETE CASCADE,
                embedding       BLOB NOT NULL
            );

            CREATE TABLE IF NOT EXISTS conversation_summaries (
                session_id      TEXT NOT NULL,
                tenant_id       TEXT NOT NULL,
                summary         TEXT NOT NULL,
                key_points      TEXT NOT NULL DEFAULT '[]',
                decisions       TEXT NOT NULL DEFAULT '[]',
                start_time      TEXT NOT NULL,
                end_time        TEXT NOT NULL,
                message_count   INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (session_id, tenant_id)
            );

            CREATE INDEX IF NOT EXISTS idx_entries_tenant ON memory_entries(tenant_id);
            CREATE INDEX IF NOT EXISTS idx_entries_session ON memory_entries(session_id);
            CREATE INDEX IF NOT EXISTS idx_entries_created ON memory_entries(created_at);

            CREATE TABLE IF NOT EXISTS rolling_summaries (
                session_id          TEXT NOT NULL,
                tenant_id           TEXT NOT NULL,
                compressed_history  TEXT NOT NULL DEFAULT '',
                topic_index         TEXT NOT NULL DEFAULT '[]',
                tokens_compressed   INTEGER NOT NULL DEFAULT 0,
                iteration_count     INTEGER NOT NULL DEFAULT 0,
                last_message_idx    INTEGER NOT NULL DEFAULT 0,
                last_compressed_at  TEXT NOT NULL,
                PRIMARY KEY (session_id, tenant_id)
            );
            CREATE INDEX IF NOT EXISTS idx_rolling_tenant ON rolling_summaries(tenant_id);
            ",
        )?;
        debug!("Memory schema initialized");
        Ok(())
    }

    // ── CRUD: Memory Entries ──

    /// Insert a new memory entry with its embedding.
    pub fn insert(&self, entry: &MemoryEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tags_json = serde_json::to_string(&entry.tags)?;
        conn.execute(
            "INSERT OR REPLACE INTO memory_entries (id, tenant_id, session_id, content, source, importance, tags, created_at, last_accessed_at, access_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry.id,
                entry.tenant_id,
                entry.session_id,
                entry.content,
                entry.source.to_string(),
                entry.importance,
                tags_json,
                entry.created_at.to_rfc3339(),
                entry.last_accessed_at.to_rfc3339(),
                entry.access_count,
            ],
        )?;

        if let Some(ref emb) = entry.embedding {
            let blob = serialize_embedding(emb);
            conn.execute(
                "INSERT OR REPLACE INTO memory_embeddings (entry_id, embedding) VALUES (?1, ?2)",
                params![entry.id, blob],
            )?;
        }
        debug!(id = %entry.id, "Memory entry inserted");
        Ok(())
    }

    /// Retrieve a memory entry by ID.
    pub fn get(&self, id: &str) -> Result<Option<MemoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, session_id, content, source, importance, tags, created_at, last_accessed_at, access_count FROM memory_entries WHERE id = ?1"
        )?;

        let result = stmt.query_row(params![id], |row| {
            Ok(Self::row_to_entry(row))
        });

        match result {
            Ok(entry) => {
                // Update access metadata inline (can't call record_access — we hold the mutex)
                conn.execute(
                    "UPDATE memory_entries SET last_accessed_at = ?1, access_count = access_count + 1 WHERE id = ?2",
                    params![Utc::now().to_rfc3339(), id],
                )?;
                Ok(Some(entry))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List entries for a tenant, optionally filtered by session.
    pub fn list_by_tenant(
        &self,
        tenant_id: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let query = if let Some(_sid) = session_id {
            format!(
                "SELECT id, tenant_id, session_id, content, source, importance, tags, created_at, last_accessed_at, access_count
                 FROM memory_entries WHERE tenant_id = ?1 AND session_id = ?2 ORDER BY created_at DESC LIMIT {limit}"
            )
        } else {
            format!(
                "SELECT id, tenant_id, session_id, content, source, importance, tags, created_at, last_accessed_at, access_count
                 FROM memory_entries WHERE tenant_id = ?1 ORDER BY created_at DESC LIMIT {limit}"
            )
        };

        let mut stmt = conn.prepare(&query)?;
        let rows: Vec<rusqlite::Result<MemoryEntry>> = if let Some(sid) = session_id {
            stmt.query_map(params![tenant_id, sid], |row| Ok(Self::row_to_entry(row)))?
                .collect()
        } else {
            stmt.query_map(params![tenant_id], |row| Ok(Self::row_to_entry(row)))?
                .collect()
        };

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// Get all entries that have embeddings (for vector search).
    pub fn entries_with_embeddings(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<(MemoryEntry, Vec<f32>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT e.id, e.tenant_id, e.session_id, e.content, e.source, e.importance, e.tags, e.created_at, e.last_accessed_at, e.access_count, m.embedding
             FROM memory_entries e
             JOIN memory_embeddings m ON e.id = m.entry_id
             WHERE e.tenant_id = ?1"
        )?;

        let rows = stmt.query_map(params![tenant_id], |row| {
            let entry = Self::row_to_entry_ref(row)?;
            let emb_blob: Vec<u8> = row.get(10)?;
            let embedding = deserialize_embedding(&emb_blob);
            Ok((entry, embedding))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Delete a memory entry.
    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM memory_entries WHERE id = ?1", params![id])?;
        debug!(id = %id, "Memory entry deleted");
        Ok(())
    }

    // ── Conversation Summaries ──

    /// Save a conversation summary.
    pub fn save_summary(&self, summary: &ConversationSummary) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let key_points_json = serde_json::to_string(&summary.key_points)?;
        let decisions_json = serde_json::to_string(&summary.decisions)?;

        conn.execute(
            "INSERT OR REPLACE INTO conversation_summaries (session_id, tenant_id, summary, key_points, decisions, start_time, end_time, message_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                summary.session_id,
                summary.tenant_id,
                summary.summary,
                key_points_json,
                decisions_json,
                summary.start_time.to_rfc3339(),
                summary.end_time.to_rfc3339(),
                summary.message_count,
            ],
        )?;
        debug!(session_id = %summary.session_id, "Conversation summary saved");
        Ok(())
    }

    /// Retrieve a conversation summary.
    pub fn get_summary(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<Option<ConversationSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT session_id, tenant_id, summary, key_points, decisions, start_time, end_time, message_count
             FROM conversation_summaries WHERE tenant_id = ?1 AND session_id = ?2"
        )?;

        let result = stmt.query_row(params![tenant_id, session_id], |row| {
            let key_points: String = row.get(3)?;
            let decisions: String = row.get(4)?;
            Ok(ConversationSummary {
                session_id: row.get(0)?,
                tenant_id: row.get(1)?,
                summary: row.get(2)?,
                key_points: serde_json::from_str(&key_points).unwrap_or_default(),
                decisions: serde_json::from_str(&decisions).unwrap_or_default(),
                start_time: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                end_time: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                message_count: row.get(7)?,
            })
        });

        match result {
            Ok(summary) => Ok(Some(summary)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    // ── Rolling Summaries ──

    /// Save (upsert) a rolling summary.
    pub fn save_rolling_summary(&self, rs: &RollingSummary) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let topic_index_json = serde_json::to_string(&rs.topic_index)?;
        conn.execute(
            "INSERT OR REPLACE INTO rolling_summaries (session_id, tenant_id, compressed_history, topic_index, tokens_compressed, iteration_count, last_message_idx, last_compressed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                rs.session_id,
                rs.tenant_id,
                rs.compressed_history,
                topic_index_json,
                rs.tokens_compressed as i64,
                rs.iteration_count,
                rs.last_message_idx as i64,
                rs.last_compressed_at.to_rfc3339(),
            ],
        )?;
        debug!(session_id = %rs.session_id, tokens = rs.tokens_compressed, "Rolling summary saved");
        Ok(())
    }

    /// Retrieve a rolling summary.
    pub fn get_rolling_summary(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<Option<RollingSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT session_id, tenant_id, compressed_history, topic_index, tokens_compressed, iteration_count, last_message_idx, last_compressed_at
             FROM rolling_summaries WHERE tenant_id = ?1 AND session_id = ?2"
        )?;

        let result = stmt.query_row(params![tenant_id, session_id], |row| {
            let topic_index_str: String = row.get(3)?;
            Ok(RollingSummary {
                session_id: row.get(0)?,
                tenant_id: row.get(1)?,
                compressed_history: row.get(2)?,
                topic_index: serde_json::from_str(&topic_index_str).unwrap_or_default(),
                tokens_compressed: row.get::<_, i64>(4)? as u64,
                iteration_count: row.get::<_, i64>(5)? as u32,
                last_message_idx: row.get::<_, i64>(6)? as usize,
                last_compressed_at: parse_datetime(&row.get::<_, String>(7).unwrap_or_default()),
            })
        });

        match result {
            Ok(rs) => Ok(Some(rs)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete a rolling summary.
    pub fn delete_rolling_summary(&self, tenant_id: &str, session_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM rolling_summaries WHERE tenant_id = ?1 AND session_id = ?2",
            params![tenant_id, session_id],
        )?;
        debug!(session_id = %session_id, "Rolling summary deleted");
        Ok(())
    }

    // ── Utilities ──

    /// Number of entries for a tenant.
    pub fn count(&self, tenant_id: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_entries WHERE tenant_id = ?1",
            params![tenant_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Delete all entries for a tenant.
    pub fn clear_tenant(&self, tenant_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM memory_entries WHERE tenant_id = ?1",
            params![tenant_id],
        )?;
        info!(tenant_id = %tenant_id, "Tenant memory cleared");
        Ok(())
    }

    // ── Row parsing helpers ──

    fn row_to_entry(row: &rusqlite::Row) -> MemoryEntry {
        Self::row_to_entry_ref(row).unwrap_or_else(|_| MemoryEntry {
            id: String::new(),
            tenant_id: String::new(),
            session_id: String::new(),
            content: String::new(),
            source: MemorySource::Conversation,
            importance: 0.0,
            tags: vec![],
            embedding: None,
            created_at: Utc::now(),
            last_accessed_at: Utc::now(),
            access_count: 0,
        })
    }

    fn row_to_entry_ref(
        row: &rusqlite::Row,
    ) -> rusqlite::Result<MemoryEntry> {
        let tags_str: String = row.get(5).unwrap_or_else(|_| "[]".into());
        Ok(MemoryEntry {
            id: row.get(0).unwrap_or_default(),
            tenant_id: row.get(1).unwrap_or_default(),
            session_id: row.get(2).unwrap_or_default(),
            content: row.get(3).unwrap_or_default(),
            source: {
                let s: String = row.get(4).unwrap_or_else(|_| "conversation".into());
                match s.as_str() {
                    "explicit" => MemorySource::Explicit,
                    "learned" => MemorySource::Learned,
                    "nudge" => MemorySource::Nudge,
                    _ => MemorySource::Conversation,
                }
            },
            importance: row.get(6).unwrap_or(0.5),
            tags: serde_json::from_str(&tags_str).unwrap_or_default(),
            embedding: None,
            created_at: parse_datetime(&row.get::<_, String>(7).unwrap_or_default()),
            last_accessed_at: parse_datetime(&row.get::<_, String>(8).unwrap_or_default()),
            access_count: row.get(9).unwrap_or(0),
        })
    }
}

// ── Embedding serialization helpers ──

fn serialize_embedding(vec: &[f32]) -> Vec<u8> {
    vec.iter()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

fn deserialize_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect()
}

fn parse_datetime(s: &str) -> DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
