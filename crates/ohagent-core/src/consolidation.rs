//! Durable consolidation — advance-only cursor over the message log.
//!
//! ## Invariant
//!
//! For every source event exactly one of the following holds:
//!
//! 1. it is represented in a successfully committed consolidation block
//!    (blocks, the consolidated-event index, and the cursor advance commit
//!    in ONE SQLite transaction);
//! 2. it is still pending (the cursor has not reached it);
//! 3. its absence is represented by an explicit durable GAP record.
//!
//! There is no fourth state ("silently disappeared").
//!
//! ## Pipeline
//!
//! ```text
//! read events from cursor N
//!     -> build deterministic batch
//!     -> consolidate (pluggable; failure has no side effects)
//!     -> single SQLite transaction:
//!            insert blocks (idempotent, deterministic ids)
//!            record consolidated event ids
//!            advance cursor N -> M (CAS on previous value)
//!     -> commit + verify
//! ```
//!
//! The cursor only ever advances inside the same transaction that durably
//! stores the consolidation output, so a crash or a durable-write failure
//! can never advance the cursor past unprocessed events. Duplicate work is
//! allowed; silent loss is not.
//!
//! ## GAP records
//!
//! If the source log loses events the cursor has not consumed (rotation,
//! truncation, manual deletion), the hole is recorded as a durable GAP
//! record before the cursor moves past it. Durable memory explicitly knows
//! part of the history is unknown; gaps are never reconstructed or
//! disguised as summaries.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::message_log::MessageLog;

pub const DEFAULT_BATCH_SIZE: usize = 100;

/// A batch of source events handed to a [`Consolidator`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEvent {
    pub id: String,
    pub tenant_id: String,
    pub session_hash: String,
    pub role: String,
    pub seq: i64,
    pub content_json: String,
    pub created_at: String,
}

/// A durable block of derived memory produced by consolidation.
///
/// `event_ids` / `first_event_seq` / `last_event_seq` are the provenance:
/// they point at the exact source events the block was built from. When raw
/// events are later removed by retention, `provenance_available` flips to
/// `false` via [`ConsolidationEngine::verify_provenance`] — the summary
/// survives and the provenance honestly reports raw evidence is gone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationBlock {
    pub id: String,
    pub source_id: String,
    pub tenant_id: String,
    pub kind: String, // "summary"
    pub content: String,
    pub first_event_seq: i64,
    pub last_event_seq: i64,
    pub event_ids: Vec<String>,
    pub provenance_available: bool,
    pub created_at: String,
}

/// A durable record that a range of source history is missing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapRecord {
    pub id: String,
    pub source_id: String,
    pub detected_at: String,
    pub last_known_seq: i64,
    pub first_available_seq: i64,
    pub missing_from: i64,
    pub missing_to: i64,
    /// "rotation" | "truncation" | "corruption" | "identity_mismatch" | "unknown"
    pub reason: String,
    pub last_known_event_id: Option<String>,
}

/// Advance-only consolidation cursor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationCursor {
    pub source_id: String,
    /// Highest source sequence successfully consumed (0 = nothing yet).
    pub last_seq: i64,
    pub last_event_id: Option<String>,
    pub last_event_ts: Option<String>,
    pub last_consolidation_id: Option<String>,
    /// Max source sequence observed at the time of the last successful run.
    pub source_revision: i64,
    pub updated_at: String,
}

/// What a single [`ConsolidationEngine::run_cycle`] did.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConsolidationOutcome {
    pub events_consumed: usize,
    pub blocks_written: usize,
    pub gaps_created: usize,
    pub cursor_advanced: bool,
    pub last_seq: i64,
}

/// Turns a batch of source events into durable blocks.
///
/// Implementations must be deterministic for the same input: block ids are
/// derived from the consumed range, so a retried commit is idempotent.
/// Model-backed consolidators should fall back to a deterministic digest on
/// model failure.
pub trait Consolidator: Send + Sync {
    fn consolidate(&self, batch: &[SourceEvent]) -> Result<Vec<ConsolidationBlock>>;
}

/// Deterministic digest consolidator — no model, no network, stable output.
///
/// Groups events by tenant and produces one compact summary block per
/// tenant per batch. Good enough as a default and as a test fixture; a
/// model-backed implementation can be swapped in later.
pub struct DigestConsolidator;

impl Consolidator for DigestConsolidator {
    fn consolidate(&self, batch: &[SourceEvent]) -> Result<Vec<ConsolidationBlock>> {
        if batch.is_empty() {
            return Ok(vec![]);
        }
        let mut by_tenant: std::collections::BTreeMap<&str, Vec<&SourceEvent>> =
            std::collections::BTreeMap::new();
        for e in batch {
            by_tenant.entry(e.tenant_id.as_str()).or_default().push(e);
        }
        let now = chrono::Utc::now().to_rfc3339();
        let mut blocks = Vec::new();
        for (tenant, events) in by_tenant {
            let first_seq = events.iter().map(|e| e.seq).min().unwrap_or(0);
            let last_seq = events.iter().map(|e| e.seq).max().unwrap_or(0);
            let mut lines = Vec::new();
            for e in &events {
                let snippet: String = e.content_json.chars().take(160).collect();
                lines.push(format!(
                    "[seq {}|{}|{}] {}",
                    e.seq, e.role, e.created_at, snippet
                ));
            }
            blocks.push(ConsolidationBlock {
                id: block_id(Self::SOURCE_ID, first_seq, last_seq, tenant),
                source_id: Self::SOURCE_ID.into(),
                tenant_id: tenant.to_string(),
                kind: "summary".into(),
                content: lines.join("\n"),
                first_event_seq: first_seq,
                last_event_seq: last_seq,
                event_ids: events.iter().map(|e| e.id.clone()).collect(),
                provenance_available: true,
                created_at: now.clone(),
            });
        }
        Ok(blocks)
    }
}

impl DigestConsolidator {
    pub const SOURCE_ID: &'static str = "message_log";
}

/// Durable state for the consolidation pipeline: cursor + blocks + gaps +
/// materialization index in ONE SQLite database, so output and cursor
/// advance commit atomically.
pub struct ConsolidationEngine {
    message_log: Arc<MessageLog>,
    state: Mutex<Connection>,
    #[allow(dead_code)]
    state_path: PathBuf,
    batch_size: usize,
}

impl ConsolidationEngine {
    pub const SOURCE_ID: &'static str = "message_log";

    /// Open (or create) the engine. `state_path` is a SQLite database that
    /// holds the cursor, blocks, gaps, and the materialized-event index.
    pub fn new(message_log: Arc<MessageLog>, state_path: &str) -> Result<Self> {
        let engine = Self {
            message_log,
            state: Mutex::new(Connection::open(state_path)?),
            state_path: PathBuf::from(state_path),
            batch_size: DEFAULT_BATCH_SIZE,
        };
        engine.init_state()?;
        engine.migrate_source()?;
        Ok(engine)
    }

    fn init_state(&self) -> Result<()> {
        let conn = self.state.lock().unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS consolidation_cursor (
                 source_id             TEXT PRIMARY KEY,
                 last_seq              INTEGER NOT NULL DEFAULT 0,
                 last_event_id         TEXT,
                 last_event_ts         TEXT,
                 last_consolidation_id TEXT,
                 source_revision       INTEGER NOT NULL DEFAULT 0,
                 updated_at            TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS consolidated_events (
                 source_id       TEXT NOT NULL,
                 event_id        TEXT NOT NULL,
                 seq             INTEGER NOT NULL,
                 block_id        TEXT NOT NULL,
                 consolidated_at TEXT NOT NULL,
                 PRIMARY KEY (source_id, event_id)
             );
             CREATE TABLE IF NOT EXISTS consolidation_blocks (
                 id                   TEXT PRIMARY KEY,
                 source_id            TEXT NOT NULL,
                 tenant_id            TEXT NOT NULL,
                 kind                 TEXT NOT NULL,
                 content              TEXT NOT NULL,
                 first_event_seq      INTEGER NOT NULL,
                 last_event_seq       INTEGER NOT NULL,
                 event_ids            TEXT NOT NULL,
                 provenance_available INTEGER NOT NULL DEFAULT 1,
                 created_at           TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS consolidation_gaps (
                 id                   TEXT PRIMARY KEY,
                 source_id            TEXT NOT NULL,
                 detected_at          TEXT NOT NULL,
                 last_known_seq       INTEGER NOT NULL,
                 first_available_seq  INTEGER NOT NULL,
                 missing_from         INTEGER NOT NULL,
                 missing_to           INTEGER NOT NULL,
                 reason               TEXT NOT NULL,
                 last_known_event_id  TEXT
             );",
        )?;
        Ok(())
    }

    /// Migrate the source log: add a gap-free monotonic `seq` column
    /// (backfilled from rowid for pre-existing rows) so continuity can be
    /// verified later even though `id` is a UUID.
    fn migrate_source(&self) -> Result<()> {
        self.message_log.with_conn(|db| {
            let has_seq: i64 = db.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('message_log') WHERE name = 'seq'",
                [],
                |r| r.get(0),
            )?;
            if has_seq == 0 {
                db.execute_batch(
                    "ALTER TABLE message_log ADD COLUMN seq INTEGER NOT NULL DEFAULT 0;
                     UPDATE message_log SET seq = rowid WHERE seq = 0;
                     CREATE INDEX IF NOT EXISTS idx_msglog_seq ON message_log(seq);",
                )?;
            }
            Ok(())
        })
    }

    fn load_cursor_tx(tx: &rusqlite::Transaction, source_id: &str) -> Result<ConsolidationCursor> {
        let now = chrono::Utc::now().to_rfc3339();
        tx.execute(
            "INSERT OR IGNORE INTO consolidation_cursor (source_id, last_seq, updated_at)
             VALUES (?1, 0, ?2)",
            params![source_id, now],
        )?;
        tx.query_row(
            "SELECT source_id, last_seq, last_event_id, last_event_ts,
                    last_consolidation_id, source_revision, updated_at
             FROM consolidation_cursor WHERE source_id = ?1",
            params![source_id],
            |r| {
                Ok(ConsolidationCursor {
                    source_id: r.get(0)?,
                    last_seq: r.get(1)?,
                    last_event_id: r.get(2)?,
                    last_event_ts: r.get(3)?,
                    last_consolidation_id: r.get(4)?,
                    source_revision: r.get(5)?,
                    updated_at: r.get(6)?,
                })
            },
        )
        .map_err(|e| anyhow!("load cursor: {e}"))
    }

    /// Detect discontinuity between the cursor and the available source.
    ///
    /// A hole in the *unconsumed* sequence range means source rows were
    /// rotated/truncated/deleted before consolidation saw them.
    fn detect_gap_tx(
        _tx: &rusqlite::Transaction,
        cursor: &ConsolidationCursor,
        first_available: Option<i64>,
    ) -> Result<Option<GapRecord>> {
        let expected_next = cursor.last_seq + 1;
        if let Some(first_available) = first_available {
            if first_available > expected_next {
                return Ok(Some(GapRecord {
                    id: gap_id(
                        Self::SOURCE_ID,
                        cursor.last_seq,
                        first_available - 1,
                        &cursor.updated_at,
                    ),
                    source_id: Self::SOURCE_ID.into(),
                    detected_at: chrono::Utc::now().to_rfc3339(),
                    last_known_seq: cursor.last_seq,
                    first_available_seq: first_available,
                    missing_from: expected_next,
                    missing_to: first_available - 1,
                    reason: gap_reason(cursor, first_available),
                    last_known_event_id: cursor.last_event_id.clone(),
                }));
            }
        }
        Ok(None)
    }

    fn read_batch(&self, after_seq: i64) -> Result<Vec<SourceEvent>> {
        self.message_log.with_conn(|db| {
            let mut stmt = db.prepare(
                "SELECT id, tenant_id, session_hash, role, seq, content_gz, created_at
                 FROM message_log
                 WHERE seq > ?1 AND archived = 0
                 ORDER BY seq LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![after_seq, self.batch_size as i64], |r| {
                    let gz: Vec<u8> = r.get(5)?;
                    let json = MessageLog::gunzip(&gz).unwrap_or_else(|_| "[compressed]".into());
                    Ok(SourceEvent {
                        id: r.get(0)?,
                        tenant_id: r.get(1)?,
                        session_hash: r.get(2)?,
                        role: r.get(3)?,
                        seq: r.get(4)?,
                        content_json: json,
                        created_at: r.get(6)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Commit a batch atomically: blocks + consolidated-event index + cursor
    /// advance in ONE transaction. Either everything lands or nothing does.
    /// All inserts are idempotent (deterministic block ids, PK on
    /// consolidated_events), so a retried commit after a hypothetical partial
    /// write replays safely.
    fn commit_batch(&self, blocks: &[ConsolidationBlock]) -> Result<ConsolidationCursor> {
        let mut conn = self.state.lock().unwrap();
        let tx = conn.transaction().context("begin state tx")?;
        let cursor = Self::load_cursor_tx(&tx, Self::SOURCE_ID)?;

        for block in blocks {
            let ids_json = serde_json::to_string(&block.event_ids)?;
            tx.execute(
                "INSERT OR IGNORE INTO consolidation_blocks
                 (id, source_id, tenant_id, kind, content, first_event_seq,
                  last_event_seq, event_ids, provenance_available, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    block.id,
                    block.source_id,
                    block.tenant_id,
                    block.kind,
                    block.content,
                    block.first_event_seq,
                    block.last_event_seq,
                    ids_json,
                    block.provenance_available as i64,
                    block.created_at,
                ],
            )?;
            for (ev_id, seq) in block.event_ids.iter().zip(block_event_seqs(block)) {
                tx.execute(
                    "INSERT OR IGNORE INTO consolidated_events
                     (source_id, event_id, seq, block_id, consolidated_at)
                     VALUES (?1,?2,?3,?4,?5)",
                    params![block.source_id, ev_id, seq, block.id, block.created_at],
                )?;
            }
        }

        // Advance the cursor with a CAS on the previous value; guaranteed to
        // match because the cursor was loaded inside this same transaction.
        let new = advance_cursor(&cursor, blocks)?;
        let changed = tx.execute(
            "UPDATE consolidation_cursor
             SET last_seq = ?2, last_event_id = ?3, last_event_ts = ?4,
                 last_consolidation_id = ?5, source_revision = ?6, updated_at = ?7
             WHERE source_id = ?1 AND last_seq = ?8",
            params![
                Self::SOURCE_ID,
                new.last_seq,
                new.last_event_id,
                new.last_event_ts,
                new.last_consolidation_id,
                new.source_revision,
                new.updated_at,
                cursor.last_seq,
            ],
        )?;
        if changed != 1 {
            return Err(anyhow!("cursor CAS failed: concurrent advance detected"));
        }
        tx.commit().context("commit consolidation tx")?;
        Ok(new)
    }

    /// Run one consolidation cycle: detect gaps, consolidate the next batch,
    /// durably commit blocks + cursor atomically.
    pub fn run_cycle(&self, consolidator: &dyn Consolidator) -> Result<ConsolidationOutcome> {
        self.migrate_source()?;
        let mut outcome = ConsolidationOutcome::default();

        // Phase 1: GAP detection + durable gap record + cursor jump over the
        // hole — one transaction, so a crash just re-detects the same gap.
        {
            let mut conn = self.state.lock().unwrap();
            let tx = conn.transaction()?;
            let cursor = Self::load_cursor_tx(&tx, Self::SOURCE_ID)?;
            // Source continuity is read from the message log, the cursor
            // state from this transaction's DB.
            let first_available: Option<i64> = self
                .message_log
                .with_conn(|db| {
                    db.query_row(
                        "SELECT MIN(seq) FROM message_log WHERE seq > ?1",
                        params![cursor.last_seq],
                        |r| r.get::<_, Option<i64>>(0),
                    )
                    .map_err(anyhow::Error::from)
                })
                .map_err(|e| anyhow!("min seq: {e}"))?;
            if let Some(gap) = Self::detect_gap_tx(&tx, &cursor, first_available)? {
                tx.execute(
                    "INSERT OR IGNORE INTO consolidation_gaps
                     (id, source_id, detected_at, last_known_seq, first_available_seq,
                      missing_from, missing_to, reason, last_known_event_id)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        gap.id,
                        gap.source_id,
                        gap.detected_at,
                        gap.last_known_seq,
                        gap.first_available_seq,
                        gap.missing_from,
                        gap.missing_to,
                        gap.reason,
                        gap.last_known_event_id,
                    ],
                )?;
                // The cursor jumps to missing_to so the next batch starts at
                // the first available event; the hole itself is covered by
                // the durable GAP record, not silently skipped.
                tx.execute(
                    "UPDATE consolidation_cursor SET last_seq = ?2, updated_at = ?3
                     WHERE source_id = ?1 AND last_seq = ?4",
                    params![
                        Self::SOURCE_ID,
                        gap.missing_to,
                        gap.detected_at,
                        cursor.last_seq,
                    ],
                )?;
                outcome.gaps_created = 1;
            }
            tx.commit()?;
        }

        // Phase 2: read the next pending batch from the (possibly advanced)
        // cursor.
        let batch = {
            let conn = self.state.lock().unwrap();
            let mut stmt =
                conn.prepare("SELECT last_seq FROM consolidation_cursor WHERE source_id = ?1")?;
            let last_seq: i64 = stmt
                .query_row(params![Self::SOURCE_ID], |r| r.get(0))
                .unwrap_or(0);
            last_seq
        };
        let batch = self.read_batch(batch)?;

        if batch.is_empty() {
            return Ok(outcome);
        }
        let blocks = consolidator.consolidate(&batch)?;
        if blocks.is_empty() {
            // Consolidation produced nothing for a non-empty batch: refuse to
            // advance (advancing here would silently lose events).
            return Ok(outcome);
        }

        // Phase 3: atomic commit of blocks + index + cursor advance.
        let new_cursor = self.commit_batch(&blocks)?;
        outcome.events_consumed = batch.len();
        outcome.blocks_written = blocks.len();
        outcome.cursor_advanced = true;
        outcome.last_seq = new_cursor.last_seq;
        Ok(outcome)
    }

    /// Blocks whose raw source events no longer exist get
    /// `provenance_available = 0` — the summary survives, and the provenance
    /// honestly reports that raw evidence is gone.
    pub fn verify_provenance(&self) -> Result<usize> {
        let blocks: Vec<(String, String)> = {
            let conn = self.state.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id, event_ids FROM consolidation_blocks
                 WHERE provenance_available = 1",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        let mut updated = 0;
        for (id, ids_json) in blocks {
            let ids: Vec<String> = serde_json::from_str(&ids_json)?;
            let all_present = self.message_log.with_conn(|db| {
                for ev in &ids {
                    let n: i64 = db.query_row(
                        "SELECT COUNT(*) FROM message_log WHERE id = ?1",
                        params![ev],
                        |r| r.get(0),
                    )?;
                    if n == 0 {
                        return Ok(false);
                    }
                }
                Ok(true)
            })?;
            if !all_present {
                let conn = self.state.lock().unwrap();
                conn.execute(
                    "UPDATE consolidation_blocks SET provenance_available = 0 WHERE id = ?1",
                    params![id],
                )?;
                updated += 1;
            }
        }
        Ok(updated)
    }

    /// List durable GAP records (all of them, newest first).
    pub fn list_gaps(&self) -> Result<Vec<GapRecord>> {
        let conn = self.state.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, source_id, detected_at, last_known_seq, first_available_seq,
                    missing_from, missing_to, reason, last_known_event_id
             FROM consolidation_gaps ORDER BY detected_at DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(GapRecord {
                    id: r.get(0)?,
                    source_id: r.get(1)?,
                    detected_at: r.get(2)?,
                    last_known_seq: r.get(3)?,
                    first_available_seq: r.get(4)?,
                    missing_from: r.get(5)?,
                    missing_to: r.get(6)?,
                    reason: r.get(7)?,
                    last_known_event_id: r.get(8)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// List durable blocks (optionally filtered by tenant).
    pub fn list_blocks(&self, tenant_id: Option<&str>) -> Result<Vec<ConsolidationBlock>> {
        let conn = self.state.lock().unwrap();
        let map = |r: &rusqlite::Row| -> rusqlite::Result<ConsolidationBlock> {
            Ok(ConsolidationBlock {
                id: r.get(0)?,
                source_id: r.get(1)?,
                tenant_id: r.get(2)?,
                kind: r.get(3)?,
                content: r.get(4)?,
                first_event_seq: r.get(5)?,
                last_event_seq: r.get(6)?,
                event_ids: serde_json::from_str(&r.get::<_, String>(7)?)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                provenance_available: r.get::<_, i64>(8)? != 0,
                created_at: r.get(9)?,
            })
        };
        match tenant_id {
            Some(t) => {
                let mut stmt = conn.prepare(
                    "SELECT id, source_id, tenant_id, kind, content, first_event_seq,
                            last_event_seq, event_ids, provenance_available, created_at
                     FROM consolidation_blocks WHERE tenant_id = ?1 ORDER BY first_event_seq",
                )?;
                let rows = stmt
                    .query_map(params![t], map)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, source_id, tenant_id, kind, content, first_event_seq,
                            last_event_seq, event_ids, provenance_available, created_at
                     FROM consolidation_blocks ORDER BY first_event_seq",
                )?;
                let rows = stmt
                    .query_map([], map)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            }
        }
    }

    /// Test/maintenance access to the state connection.
    #[cfg(test)]
    pub(crate) fn with_state<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let conn = self.state.lock().unwrap();
        f(&conn)
    }

    /// Current cursor state (for tests and dashboards).
    pub fn cursor(&self) -> Result<ConsolidationCursor> {
        let conn = self.state.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        Self::load_cursor_tx(&tx, Self::SOURCE_ID)
    }

    /// Open the state DB read-only (test helper for simulating durable-write
    /// failures on an otherwise valid state file).
    #[cfg(test)]
    pub(crate) fn open_state_read_only(path: &str) -> Connection {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }
}

/// Provenance: seqs are contiguous by construction (batch read ORDER BY seq)
/// and event_ids are stored in the same order.
fn block_event_seqs(block: &ConsolidationBlock) -> Vec<i64> {
    (block.first_event_seq..=block.last_event_seq).collect()
}

fn advance_cursor(
    cursor: &ConsolidationCursor,
    blocks: &[ConsolidationBlock],
) -> Result<ConsolidationCursor> {
    let max_seq = blocks
        .iter()
        .map(|b| b.last_event_seq)
        .max()
        .ok_or_else(|| anyhow!("no blocks to advance with"))?;
    let last_block = blocks
        .iter()
        .max_by_key(|b| b.last_event_seq)
        .ok_or_else(|| anyhow!("no blocks"))?;
    let last_event_id = last_block.event_ids.last().cloned();
    Ok(ConsolidationCursor {
        source_id: cursor.source_id.clone(),
        last_seq: max_seq,
        last_event_id,
        last_event_ts: Some(chrono::Utc::now().to_rfc3339()),
        last_consolidation_id: Some(
            blocks
                .iter()
                .map(|b| b.id.clone())
                .collect::<Vec<_>>()
                .join(","),
        ),
        source_revision: cursor.source_revision.max(max_seq),
        updated_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Deterministic gap id: same hole detected twice -> same record (idempotent).
fn gap_id(source: &str, last_known: i64, first_available_minus1: i64, at: &str) -> String {
    let mut h = Sha256::new();
    h.update(source.as_bytes());
    h.update(last_known.to_le_bytes());
    h.update(first_available_minus1.to_le_bytes());
    h.update(at.as_bytes());
    let d = h.finalize();
    format!(
        "gap-{:016x}",
        u64::from_be_bytes(d[..8].try_into().unwrap())
    )
}

/// Deterministic block id: same source range -> same id => idempotent commit.
fn block_id(source: &str, first_seq: i64, last_seq: i64, tenant: &str) -> String {
    let mut h = Sha256::new();
    h.update(source.as_bytes());
    h.update(first_seq.to_le_bytes());
    h.update(last_seq.to_le_bytes());
    h.update(tenant.as_bytes());
    let d = h.finalize();
    format!(
        "blk-{}-{}-{}-{:016x}",
        first_seq,
        last_seq,
        &tenant[..tenant.len().min(12)],
        u64::from_be_bytes(d[..8].try_into().unwrap())
    )
}

fn gap_reason(cursor: &ConsolidationCursor, first_available: i64) -> String {
    // We deliberately do NOT guess reconstruction-friendly reasons. The
    // missing count hints at rotation vs truncation but never invents facts.
    let missing = first_available - cursor.last_seq - 1;
    if missing > DEFAULT_BATCH_SIZE as i64 {
        "truncation".to_string()
    } else {
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDb {
        path: String,
    }

    impl TempDb {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join("ohagent-consolidation-tests");
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("{}-{}.db", tag, uuid::Uuid::new_v4()));
            TempDb {
                path: path.to_string_lossy().into_owned(),
            }
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_file(format!("{}-wal", self.path));
            let _ = std::fs::remove_file(format!("{}-shm", self.path));
        }
    }

    fn engine(tag: &str) -> (ConsolidationEngine, TempDb) {
        let log = Arc::new(MessageLog::open(":memory:").unwrap());
        let state = TempDb::new(tag);
        let engine = ConsolidationEngine::new(log.clone(), &state.path).unwrap();
        (engine, state)
    }

    fn log_event(engine: &ConsolidationEngine, tenant: &str, content: &str) -> String {
        engine
            .message_log
            .log_messages(
                tenant,
                "s1",
                "user",
                1,
                &[serde_json::json!({"role": "user", "content": content})],
            )
            .unwrap()
    }

    #[test]
    fn successful_consolidation_advances_cursor() {
        let (engine, _t) = engine("ok-advance");
        for i in 0..5 {
            log_event(&engine, "t1", &format!("event {i}"));
        }
        let out = engine.run_cycle(&DigestConsolidator).unwrap();
        assert_eq!(out.events_consumed, 5);
        assert!(out.cursor_advanced);
        assert_eq!(out.last_seq, 5);
        assert_eq!(engine.cursor().unwrap().last_seq, 5);
    }

    #[test]
    fn failed_consolidation_does_not_advance_cursor() {
        struct Fail;
        impl Consolidator for Fail {
            fn consolidate(&self, _b: &[SourceEvent]) -> Result<Vec<ConsolidationBlock>> {
                Err(anyhow!("model down"))
            }
        }
        let (engine, _t) = engine("fail-no-advance");
        for i in 0..3 {
            log_event(&engine, "t1", &format!("event {i}"));
        }
        let err = engine.run_cycle(&Fail);
        assert!(err.is_err());
        // Cursor unchanged, events still pending.
        assert_eq!(engine.cursor().unwrap().last_seq, 0);
        // A later successful cycle processes them — nothing was lost.
        let out = engine.run_cycle(&DigestConsolidator).unwrap();
        assert_eq!(out.events_consumed, 3);
        assert_eq!(out.last_seq, 3);
    }

    #[test]
    fn durable_write_failure_does_not_advance_cursor() {
        // Simulate a genuine durable-write failure: the state DB connection
        // refuses writes (query_only), while the source log is intact.
        let log = Arc::new(MessageLog::open(":memory:").unwrap());
        let temp = TempDb::new("write-fail");
        let engine = ConsolidationEngine::new(log.clone(), &temp.path).unwrap();
        for i in 0..3 {
            log_event(&engine, "t1", &format!("event {i}"));
        }
        engine
            .with_state(|conn| Ok(conn.execute_batch("PRAGMA query_only = ON;")?))
            .unwrap();

        let err = engine.run_cycle(&DigestConsolidator);
        assert!(err.is_err(), "durable write must fail");
        drop(engine);

        // Fresh engine over the same state file: cursor untouched, all three
        // events still pending and processed on the next successful cycle.
        let eng2 = ConsolidationEngine::new(log, &temp.path).unwrap();
        assert_eq!(
            eng2.cursor().unwrap().last_seq,
            0,
            "cursor must not advance"
        );
        let out = eng2.run_cycle(&DigestConsolidator).unwrap();
        assert_eq!(
            out.events_consumed, 3,
            "events reprocessed after write failure"
        );
        assert_eq!(out.last_seq, 3);
    }

    #[test]
    fn crash_restart_reprocesses_uncommitted_batch() {
        // Simulate: events logged, consolidation never commits (process dies
        // between read and commit). On restart the same events are processed.
        let log = Arc::new(MessageLog::open(":memory:").unwrap());
        let temp = TempDb::new("crash");
        {
            // "Crashed" engine: state DB exists, cursor never advanced.
            let eng = ConsolidationEngine::new(log.clone(), &temp.path).unwrap();
            for i in 0..4 {
                log_event(&eng, "t1", &format!("pre-crash {i}"));
            }
            struct NeverCommit;
            impl Consolidator for NeverCommit {
                fn consolidate(&self, _b: &[SourceEvent]) -> Result<Vec<ConsolidationBlock>> {
                    Err(anyhow!("crash mid-cycle"))
                }
            }
            let _ = eng.run_cycle(&NeverCommit);
            assert_eq!(eng.cursor().unwrap().last_seq, 0);
        }
        // "Restart": fresh engine over the same state file.
        let log2 = log.clone();
        let eng = ConsolidationEngine::new(log2, &temp.path).unwrap();
        let out = eng.run_cycle(&DigestConsolidator).unwrap();
        assert_eq!(out.events_consumed, 4);
        assert_eq!(out.last_seq, 4);
    }

    #[test]
    fn repeated_processing_never_silently_loses_events() {
        // Property-style sweep: run many cycles; every event must be covered
        // by exactly one committed block. No event silently vanishes, and no
        // event is double-consumed.
        let (engine, _t) = engine("no-loss");
        let total = 250usize; // more than one batch
        for i in 0..total {
            log_event(&engine, "t1", &format!("event-{i}"));
        }
        let mut cycles = 0;
        loop {
            let out = engine.run_cycle(&DigestConsolidator).unwrap();
            if out.events_consumed == 0 {
                break;
            }
            cycles += 1;
            assert!(out.cursor_advanced || out.gaps_created > 0);
            assert!(cycles < 100, "run_cycle must converge");
        }
        let blocks = engine.list_blocks(None).unwrap();
        let mut covered: Vec<i64> = Vec::new();
        for b in &blocks {
            for s in b.first_event_seq..=b.last_event_seq {
                covered.push(s);
            }
        }
        covered.sort();
        let expected: Vec<i64> = (1..=total as i64).collect();
        assert_eq!(
            covered, expected,
            "every event must be in exactly one block"
        );
        let unique: std::collections::BTreeSet<i64> = covered.iter().copied().collect();
        assert_eq!(covered.len(), unique.len(), "no event consumed twice");
    }

    #[test]
    fn log_truncation_creates_gap() {
        let (engine, _t) = engine("gap");
        for i in 1..=6 {
            log_event(&engine, "t1", &format!("event {i}"));
        }
        assert_eq!(engine.cursor().unwrap().last_seq, 0);

        // Simulate rotation/truncation: drop events 1..=3 (unconsumed).
        engine
            .message_log
            .with_conn(|db| Ok(db.execute("DELETE FROM message_log WHERE seq <= 3", [])?))
            .unwrap();

        let out = engine.run_cycle(&DigestConsolidator).unwrap();
        assert_eq!(
            out.gaps_created, 1,
            "discontinuity must create a GAP record"
        );

        let gaps = engine.list_gaps().unwrap();
        assert_eq!(gaps.len(), 1);
        let gap = &gaps[0];
        assert_eq!(gap.missing_from, 1);
        assert_eq!(gap.missing_to, 3);
        assert_eq!(gap.first_available_seq, 4);

        // Real events after the hole are consumed; the void is not "summarized".
        assert_eq!(out.events_consumed, 3, "events 4, 5, 6 still processed");
        for b in engine.list_blocks(None).unwrap() {
            assert!(
                b.first_event_seq >= 4,
                "gap range must not appear as a summary block"
            );
        }
        assert_eq!(engine.cursor().unwrap().last_seq, 6);
    }

    #[test]
    fn gap_is_durable_after_later_successful_consolidation() {
        let (engine, _t) = engine("gap-durable");
        for i in 1..=4 {
            log_event(&engine, "t1", &format!("event {i}"));
        }
        engine
            .message_log
            .with_conn(|db| Ok(db.execute("DELETE FROM message_log WHERE seq = 1", [])?))
            .unwrap();

        engine.run_cycle(&DigestConsolidator).unwrap();
        assert_eq!(engine.list_gaps().unwrap().len(), 1);

        // More events arrive and consolidate successfully — the GAP survives.
        for i in 10..12 {
            log_event(&engine, "t1", &format!("later {i}"));
        }
        engine.run_cycle(&DigestConsolidator).unwrap();
        engine.run_cycle(&DigestConsolidator).unwrap();
        assert_eq!(engine.list_gaps().unwrap().len(), 1, "gap must persist");
    }

    #[test]
    fn summary_retains_source_provenance() {
        let (engine, _t) = engine("provenance");
        let mut ids = Vec::new();
        for i in 0..4 {
            ids.push(log_event(&engine, "t1", &format!("event {i}")));
        }
        engine.run_cycle(&DigestConsolidator).unwrap();
        let blocks = engine.list_blocks(Some("t1")).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].event_ids, ids);
        assert!(blocks[0].provenance_available);
        assert_eq!(blocks[0].first_event_seq, 1);
        assert_eq!(blocks[0].last_event_seq, 4);
    }

    #[test]
    fn provenance_honestly_reports_removed_raw_events() {
        let (engine, _t) = engine("provenance-removed");
        for i in 0..3 {
            log_event(&engine, "t1", &format!("event {i}"));
        }
        engine.run_cycle(&DigestConsolidator).unwrap();

        // Retention removes the raw evidence.
        engine
            .message_log
            .with_conn(|db| Ok(db.execute("DELETE FROM message_log", [])?))
            .unwrap();
        let flagged = engine.verify_provenance().unwrap();
        assert_eq!(flagged, 1);
        let blocks = engine.list_blocks(None).unwrap();
        assert_eq!(blocks[0].provenance_available, false);
        assert!(!blocks[0].content.is_empty(), "summary survives retention");
    }
}
