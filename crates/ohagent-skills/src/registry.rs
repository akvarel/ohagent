//! Skill registry — persistent SQLite storage for learned skills.
//!
//! Stores skills and usage events. Scoped per-tenant.

use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{debug, info};

use crate::models::{Skill, SkillConfig, SkillOrigin, SkillStatus, SkillUsage};
use crate::Result;

/// Persistent skill registry.
pub struct SkillRegistry {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    config: SkillConfig,
}

impl SkillRegistry {
    /// Open or create the skills database.
    pub fn open(config: SkillConfig) -> Result<Self> {
        let db_path = shellexpand::tilde(&config.db_path).to_string();
        let path = PathBuf::from(&db_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        let reg = Self {
            conn: Mutex::new(conn),
            config,
        };
        reg.init_schema()?;
        info!(path = %db_path, "Skill registry opened");
        Ok(reg)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS skills (
                id              TEXT PRIMARY KEY,
                tenant_id       TEXT NOT NULL,
                name            TEXT NOT NULL,
                description     TEXT NOT NULL DEFAULT '',
                triggers        TEXT NOT NULL DEFAULT '[]',
                instructions    TEXT NOT NULL DEFAULT '',
                version         TEXT NOT NULL DEFAULT '0.1.0',
                origin          TEXT NOT NULL DEFAULT 'auto',
                status          TEXT NOT NULL DEFAULT 'proposed',
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL,
                last_used_at    TEXT,
                use_count       INTEGER NOT NULL DEFAULT 0,
                success_count   INTEGER NOT NULL DEFAULT 0,
                failure_count   INTEGER NOT NULL DEFAULT 0,
                quality_score   REAL NOT NULL DEFAULT 0.5,
                tags            TEXT NOT NULL DEFAULT '[]'
            );

            CREATE TABLE IF NOT EXISTS skill_usage (
                id              TEXT PRIMARY KEY,
                skill_id        TEXT NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
                session_id      TEXT NOT NULL,
                tenant_id       TEXT NOT NULL,
                success         INTEGER NOT NULL DEFAULT 1,
                rating          INTEGER,
                duration_secs   REAL,
                timestamp       TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_skills_tenant ON skills(tenant_id);
            CREATE INDEX IF NOT EXISTS idx_skills_name ON skills(tenant_id, name);
            CREATE INDEX IF NOT EXISTS idx_usage_skill ON skill_usage(skill_id);
            CREATE INDEX IF NOT EXISTS idx_usage_tenant ON skill_usage(tenant_id);
            ",
        )?;
        // Add pinned column (migration: safe to run on existing databases)
        conn.execute_batch("ALTER TABLE skills ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;")
            .ok();
        debug!("Skill schema initialized");
        Ok(())
    }

    // ── Skill CRUD ──

    /// Insert or update a skill.
    pub fn save(&self, skill: &Skill) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let triggers_json = serde_json::to_string(&skill.triggers)?;
        let tags_json = serde_json::to_string(&skill.tags)?;
        conn.execute(
            "INSERT OR REPLACE INTO skills
             (id, tenant_id, name, description, triggers, instructions, version, origin, status,
              created_at, updated_at, last_used_at, use_count, success_count, failure_count, quality_score, tags, pinned)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            params![
                skill.id, skill.tenant_id, skill.name, skill.description,
                triggers_json, skill.instructions, skill.version, skill.origin.to_string(),
                skill.status.to_string(),
                skill.created_at.to_rfc3339(), skill.updated_at.to_rfc3339(),
                skill.last_used_at.map(|t| t.to_rfc3339()),
                skill.use_count, skill.success_count, skill.failure_count,
                skill.quality_score, tags_json, skill.pinned as i32,
            ],
        )?;
        debug!(id = %skill.id, name = %skill.name, "Skill saved");
        Ok(())
    }

    /// Get a skill by ID.
    pub fn get(&self, id: &str) -> Result<Option<Skill>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, name, description, triggers, instructions, version, origin, status,
                    created_at, updated_at, last_used_at, use_count, success_count, failure_count, quality_score, tags
             FROM skills WHERE id = ?1"
        )?;
        let result = stmt.query_row(params![id], |row| Ok(Self::row_to_skill(row)));
        match result {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Find a skill by tenant + name.
    pub fn find_by_name(&self, tenant_id: &str, name: &str) -> Result<Option<Skill>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, name, description, triggers, instructions, version, origin, status,
                    created_at, updated_at, last_used_at, use_count, success_count, failure_count, quality_score, tags
             FROM skills WHERE tenant_id = ?1 AND name = ?2"
        )?;
        let result = stmt.query_row(params![tenant_id, name], |row| Ok(Self::row_to_skill(row)));
        match result {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List skills for a tenant, filtered by status.
    pub fn list(
        &self,
        tenant_id: &str,
        status: Option<&SkillStatus>,
        limit: usize,
    ) -> Result<Vec<Skill>> {
        let conn = self.conn.lock().unwrap();
        let query = if let Some(_st) = status {
            format!(
                "SELECT id, tenant_id, name, description, triggers, instructions, version, origin, status,
                        created_at, updated_at, last_used_at, use_count, success_count, failure_count, quality_score, tags
                 FROM skills WHERE tenant_id = ?1 AND status = ?2
                 ORDER BY quality_score DESC LIMIT {limit}"
            )
        } else {
            format!(
                "SELECT id, tenant_id, name, description, triggers, instructions, version, origin, status,
                        created_at, updated_at, last_used_at, use_count, success_count, failure_count, quality_score, tags
                 FROM skills WHERE tenant_id = ?1
                 ORDER BY quality_score DESC LIMIT {limit}"
            )
        };

        let mut stmt = conn.prepare(&query)?;
        let rows: Vec<Skill> = if let Some(st) = status {
            stmt.query_map(params![tenant_id, st.to_string()], |row| {
                Ok(Self::row_to_skill(row))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![tenant_id], |row| Ok(Self::row_to_skill(row)))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    /// Return all distinct tenant IDs in the registry.
    pub fn all_tenants(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT DISTINCT tenant_id FROM skills")?;
        let rows = stmt
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(rows)
    }

    /// Delete a skill.
    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM skills WHERE id = ?1", params![id])?;
        debug!(id = %id, "Skill deleted");
        Ok(())
    }

    /// Count skills for a tenant.
    pub fn count(&self, tenant_id: &str, status: Option<&SkillStatus>) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = if let Some(st) = status {
            conn.query_row(
                "SELECT COUNT(*) FROM skills WHERE tenant_id = ?1 AND status = ?2",
                params![tenant_id, st.to_string()],
                |row| row.get(0),
            )?
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM skills WHERE tenant_id = ?1",
                params![tenant_id],
                |row| row.get(0),
            )?
        };
        Ok(count as usize)
    }

    // ── Usage Tracking ──

    /// Record a skill usage event.
    pub fn record_usage(&self, usage: &SkillUsage) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO skill_usage (id, skill_id, session_id, tenant_id, success, rating, duration_secs, timestamp)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                usage.id, usage.skill_id, usage.session_id, usage.tenant_id,
                usage.success as i32, usage.rating, usage.duration_secs,
                usage.timestamp.to_rfc3339(),
            ],
        )?;

        // Update skill counters
        if usage.success {
            conn.execute(
                "UPDATE skills SET use_count = use_count + 1, success_count = success_count + 1,
                 last_used_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![Utc::now().to_rfc3339(), usage.skill_id],
            )?;
        } else {
            conn.execute(
                "UPDATE skills SET use_count = use_count + 1, failure_count = failure_count + 1,
                 last_used_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![Utc::now().to_rfc3339(), usage.skill_id],
            )?;
        }

        // Recompute quality score
        let skill: Skill = conn.query_row(
            "SELECT id, tenant_id, name, description, triggers, instructions, version, origin, status,
                    created_at, updated_at, last_used_at, use_count, success_count, failure_count, quality_score, tags
             FROM skills WHERE id = ?1",
            params![usage.skill_id],
            |row| Ok(Self::row_to_skill(row)),
        )?;
        let new_score = skill.compute_quality();
        conn.execute(
            "UPDATE skills SET quality_score = ?1 WHERE id = ?2",
            params![new_score, usage.skill_id],
        )?;

        debug!(skill_id = %usage.skill_id, success = usage.success, "Usage recorded");
        Ok(())
    }

    /// Get usage stats for a skill.
    pub fn usage_stats(&self, skill_id: &str, limit: usize) -> Result<Vec<SkillUsage>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, skill_id, session_id, tenant_id, success, rating, duration_secs, timestamp
             FROM skill_usage WHERE skill_id = ?1 ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![skill_id, limit as i64], |row| {
            Ok(SkillUsage {
                id: row.get(0)?,
                skill_id: row.get(1)?,
                session_id: row.get(2)?,
                tenant_id: row.get(3)?,
                success: row.get::<_, i32>(4)? != 0,
                rating: row.get(5)?,
                duration_secs: row.get(6)?,
                timestamp: parse_dt(&row.get::<_, String>(7).unwrap_or_default()),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    // ── Helpers ──

    fn row_to_skill(row: &rusqlite::Row) -> Skill {
        let triggers_str: String = row.get(4).unwrap_or_else(|_| "[]".into());
        let tags_str: String = row.get(17).unwrap_or_else(|_| "[]".into());
        Skill {
            id: row.get(0).unwrap_or_default(),
            tenant_id: row.get(1).unwrap_or_default(),
            name: row.get(2).unwrap_or_default(),
            description: row.get(3).unwrap_or_default(),
            triggers: serde_json::from_str(&triggers_str).unwrap_or_default(),
            instructions: row.get(5).unwrap_or_default(),
            version: row.get(6).unwrap_or_default(),
            origin: parse_origin(&row.get::<_, String>(7).unwrap_or_default()),
            status: parse_status(&row.get::<_, String>(8).unwrap_or_default()),
            created_at: parse_dt(&row.get::<_, String>(9).unwrap_or_default()),
            updated_at: parse_dt(&row.get::<_, String>(10).unwrap_or_default()),
            last_used_at: row
                .get::<_, Option<String>>(11)
                .ok()
                .flatten()
                .map(|s| parse_dt(&s)),
            use_count: row.get(12).unwrap_or(0),
            success_count: row.get(13).unwrap_or(0),
            failure_count: row.get(14).unwrap_or(0),
            quality_score: row.get(15).unwrap_or(0.5),
            tags: serde_json::from_str(&tags_str).unwrap_or_default(),
            pinned: row.get::<_, i32>(18).unwrap_or(0) != 0,
        }
    }
}

fn parse_dt(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn parse_origin(s: &str) -> SkillOrigin {
    match s {
        "explicit" => SkillOrigin::Explicit,
        "imported" => SkillOrigin::Imported,
        "merged" => SkillOrigin::Merged,
        _ => SkillOrigin::Auto,
    }
}

fn parse_status(s: &str) -> SkillStatus {
    match s {
        "active" => SkillStatus::Active,
        "disabled" => SkillStatus::Disabled,
        "retired" => SkillStatus::Retired,
        _ => SkillStatus::Proposed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_config() -> SkillConfig {
        SkillConfig {
            db_path: ":memory:".to_string(),
            ..Default::default()
        }
    }

    fn test_skill(tenant: &str, name: &str) -> Skill {
        Skill {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant.into(),
            name: name.into(),
            description: format!("Test skill {name}"),
            triggers: vec![format!("trigger-{name}")],
            instructions: format!("Do {name}"),
            version: "0.1.0".into(),
            origin: SkillOrigin::Auto,
            status: SkillStatus::Proposed,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_used_at: None,
            use_count: 0,
            success_count: 0,
            failure_count: 0,
            quality_score: 0.5,
            tags: vec!["test".into()],
            pinned: false,
        }
    }

    #[test]
    fn test_save_and_get() {
        let reg = SkillRegistry::open(test_config()).unwrap();
        let skill = test_skill("t1", "deploy");
        reg.save(&skill).unwrap();
        let found = reg.get(&skill.id).unwrap().unwrap();
        assert_eq!(found.name, "deploy");
    }

    #[test]
    fn test_find_by_name() {
        let reg = SkillRegistry::open(test_config()).unwrap();
        let skill = test_skill("t1", "backup-db");
        reg.save(&skill).unwrap();
        let found = reg.find_by_name("t1", "backup-db").unwrap().unwrap();
        assert_eq!(found.id, skill.id);
    }

    #[test]
    fn test_record_usage_updates_counters() {
        let reg = SkillRegistry::open(test_config()).unwrap();
        let skill = test_skill("t1", "test-skill");
        reg.save(&skill).unwrap();

        let usage = SkillUsage {
            id: Uuid::new_v4().to_string(),
            skill_id: skill.id.clone(),
            session_id: "s1".into(),
            tenant_id: "t1".into(),
            success: true,
            rating: Some(5),
            duration_secs: Some(2.5),
            timestamp: Utc::now(),
        };
        reg.record_usage(&usage).unwrap();

        let updated = reg.get(&skill.id).unwrap().unwrap();
        assert_eq!(updated.use_count, 1);
        assert_eq!(updated.success_count, 1);
    }

    #[test]
    fn test_list_by_status() {
        let reg = SkillRegistry::open(test_config()).unwrap();
        for i in 0..3 {
            let mut sk = test_skill("t1", &format!("sk-{i}"));
            if i == 0 {
                sk.status = SkillStatus::Active;
            }
            reg.save(&sk).unwrap();
        }
        let proposed = reg.list("t1", Some(&SkillStatus::Proposed), 10).unwrap();
        assert_eq!(proposed.len(), 2);
    }

    #[test]
    fn test_quality_recomputed() {
        let reg = SkillRegistry::open(test_config()).unwrap();
        let mut skill = test_skill("t1", "q-test");
        skill.status = SkillStatus::Active;
        reg.save(&skill).unwrap();

        // Record 2 successes, 1 failure
        for (i, success) in [true, false, true].iter().enumerate() {
            reg.record_usage(&SkillUsage {
                id: Uuid::new_v4().to_string(),
                skill_id: skill.id.clone(),
                session_id: format!("s{i}"),
                tenant_id: "t1".into(),
                success: *success,
                rating: None,
                duration_secs: None,
                timestamp: Utc::now(),
            })
            .unwrap();
        }

        let updated = reg.get(&skill.id).unwrap().unwrap();
        assert_eq!(updated.use_count, 3);
        assert!(updated.quality_score > 0.0);
    }
}
