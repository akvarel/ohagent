//! Database migration system for ohAgent.
//!
//! Simple versioned migrations stored in code.
//! Each migration has a unique version number and an up (forward) script.
//! Migrations are applied in order on startup.
//!
//! Tracks applied migrations in a `_migrations` table within each SQLite database.

use rusqlite::Connection;
use tracing::info;

/// A single database migration.
pub struct Migration {
    /// Unique version number (1-based, applied in ascending order)
    pub version: i64,
    /// Human-readable description
    pub description: &'static str,
    /// SQL to apply (CREATE TABLE, ALTER TABLE, etc.)
    pub up: &'static str,
}

/// Apply all pending migrations to a SQLite database.
pub fn run(conn: &Connection) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    // Ensure migrations tracking table exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        [],
    )?;

    // Get already-applied versions
    let mut stmt = conn.prepare("SELECT version FROM _migrations ORDER BY version")?;
    let applied: Vec<i64> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut new_migrations = Vec::new();

    for migration in all_migrations() {
        if applied.contains(&migration.version) {
            continue;
        }

        info!(
            version = migration.version,
            description = migration.description,
            "Applying migration"
        );

        conn.execute_batch(migration.up)?;
        conn.execute(
            "INSERT INTO _migrations (version, description) VALUES (?1, ?2)",
            rusqlite::params![migration.version, migration.description],
        )?;

        new_migrations.push(migration.version);
    }

    if new_migrations.is_empty() {
        info!("All migrations up to date");
    } else {
        info!(
            count = new_migrations.len(),
            "Applied new migrations"
        );
    }

    Ok(new_migrations)
}

/// All migrations in version order.
fn all_migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            description: "Create skills table",
            up: "CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                tenant_id TEXT NOT NULL DEFAULT 'default',
                status TEXT NOT NULL DEFAULT 'proposed',
                quality_score REAL NOT NULL DEFAULT 0.0,
                usage_count INTEGER NOT NULL DEFAULT 0,
                instructions TEXT,
                triggers TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_used_at TEXT
            )",
        },
        Migration {
            version: 2,
            description: "Create memory table",
            up: "CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL DEFAULT 'default',
                content TEXT NOT NULL,
                source_type TEXT NOT NULL DEFAULT 'conversation',
                importance REAL NOT NULL DEFAULT 0.5,
                embedding BLOB,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_accessed_at TEXT
            )",
        },
        Migration {
            version: 3,
            description: "Create usage tracking table",
            up: "CREATE TABLE IF NOT EXISTS usage_records (
                id TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                cost_usd_micros INTEGER NOT NULL DEFAULT 0,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        },
        Migration {
            version: 4,
            description: "Create message log table",
            up: "CREATE TABLE IF NOT EXISTS message_log (
                id TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT,
                prompt_gzip BLOB NOT NULL,
                response_gzip BLOB NOT NULL,
                prompt_tokens INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        },
        Migration {
            version: 5,
            description: "Create message log prefs table",
            up: "CREATE TABLE IF NOT EXISTS message_log_prefs (
                tenant_id TEXT PRIMARY KEY,
                enabled INTEGER NOT NULL DEFAULT 1
            )",
        },
        Migration {
            version: 6,
            description: "Create pairing codes table",
            up: "CREATE TABLE IF NOT EXISTS pairing_codes (
                code TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL,
                platform TEXT NOT NULL DEFAULT 'telegram',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                expires_at TEXT NOT NULL,
                used INTEGER NOT NULL DEFAULT 0
            )",
        },
        Migration {
            version: 7,
            description: "Add indexes for common queries",
            up: "
                CREATE INDEX IF NOT EXISTS idx_skills_tenant_status ON skills(tenant_id, status);
                CREATE INDEX IF NOT EXISTS idx_memories_tenant ON memories(tenant_id);
                CREATE INDEX IF NOT EXISTS idx_usage_tenant_date ON usage_records(tenant_id, created_at);
                CREATE INDEX IF NOT EXISTS idx_message_log_tenant_date ON message_log(tenant_id, created_at);
            ",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations_are_ordered() {
        let migrations = all_migrations();
        for i in 1..migrations.len() {
            assert!(
                migrations[i].version > migrations[i - 1].version,
                "Migrations must be ordered by version"
            );
        }
    }

    #[test]
    fn test_migrations_apply() {
        let conn = Connection::open_in_memory().unwrap();
        let applied = run(&conn).unwrap();
        assert_eq!(applied.len(), 7);

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"skills".to_string()));
        assert!(tables.contains(&"memories".to_string()));
        assert!(tables.contains(&"usage_records".to_string()));
        assert!(tables.contains(&"message_log".to_string()));
        assert!(tables.contains(&"message_log_prefs".to_string()));
        assert!(tables.contains(&"pairing_codes".to_string()));
        assert!(tables.contains(&"_migrations".to_string()));

        // Re-run should apply nothing new
        let applied2 = run(&conn).unwrap();
        assert!(applied2.is_empty());
    }
}
