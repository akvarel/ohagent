//! Cron scheduler for ohAgent background tasks.
//!
//! Manages cron jobs stored in SQLite. Runs a tick loop that checks
//! for due jobs and delivers them via the push notification service.

use std::sync::Arc;
use chrono::Utc;
use rusqlite::Connection;
use tracing::{info, warn};

use crate::job::CronJob;

/// A persistent cron scheduler backed by SQLite.
pub struct CronScheduler {
    db: Connection,
    push: Option<Arc<dyn PushNotifier + Send + Sync>>,
}

/// Trait for delivering scheduled task results.
pub trait PushNotifier {
    fn send(&self, tenant_id: &str, chat_id: &str, message: &str);
}

/// SQLite-backed implementation using the daemon's push service.
impl PushNotifier for Arc<ohagent_core::push::PushService> {
    fn send(&self, tenant_id: &str, _chat_id: &str, message: &str) {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let push = Arc::clone(self);
            let tenant = tenant_id.to_string();
            let msg = message.to_string();
            handle.spawn(async move {
                // PushService::send is async. Call via deref on Arc.
                let ps: &ohagent_core::push::PushService = &push;
                let _ = ps.send(&tenant, &msg).await;
            });
        } else {
            warn!("Cron tick: no tokio runtime available for push");
        }
    }
}

impl CronScheduler {
    /// Open or create the cron database.
    pub fn open(db_path: &str, push: Option<Arc<dyn PushNotifier + Send + Sync>>) -> Result<Self, Box<dyn std::error::Error>> {
        let db = Connection::open(db_path)?;

        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS cron_jobs (
                id TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL,
                name TEXT NOT NULL,
                task TEXT NOT NULL,
                cron_expr TEXT NOT NULL,
                skill TEXT,
                platform TEXT NOT NULL DEFAULT 'telegram',
                chat_id TEXT NOT NULL DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                last_fired_at TEXT,
                fire_count INTEGER NOT NULL DEFAULT 0,
                fail_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_cron_tenant ON cron_jobs(tenant_id);
            CREATE INDEX IF NOT EXISTS idx_cron_enabled ON cron_jobs(enabled);",
        )?;

        info!("Cron scheduler initialized ({db_path})");
        Ok(Self { db, push })
    }

    /// Add a new cron job.
    pub fn add_job(&self, job: CronJob) -> Result<(), Box<dyn std::error::Error>> {
        self.db.execute(
            "INSERT INTO cron_jobs (id, tenant_id, name, task, cron_expr, skill, platform, chat_id, enabled, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                job.id,
                job.tenant_id,
                job.name,
                job.task,
                job.cron_expr,
                job.skill,
                job.platform,
                job.chat_id,
                job.enabled as i32,
                job.created_at.to_rfc3339(),
            ],
        )?;
        info!(job = %job.name, cron = %job.cron_expr, "Cron job added");
        Ok(())
    }

    /// Add a simple reminder that fires at a specific hour each day.
    pub fn add_daily_reminder(
        &self,
        tenant_id: &str,
        chat_id: &str,
        hour: u32,
        minute: u32,
        message: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let job = CronJob::new(tenant_id, format!("daily_{hour:02}{minute:02}"), message, 
            format!("{minute} {hour} * * *"))
            .with_delivery("telegram", chat_id);
        self.add_job(job)
    }

    /// Get all enabled jobs for a tenant.
    pub fn get_enabled(&self, tenant_id: &str) -> Result<Vec<CronJob>, Box<dyn std::error::Error>> {
        let mut stmt = self.db.prepare(
            "SELECT id, tenant_id, name, task, cron_expr, skill, platform, chat_id, enabled, created_at, last_fired_at, fire_count, fail_count
             FROM cron_jobs WHERE tenant_id = ?1 AND enabled = 1",
        )?;
        let jobs = stmt.query_map(rusqlite::params![tenant_id], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                tenant_id: row.get(1)?,
                name: row.get(2)?,
                task: row.get(3)?,
                cron_expr: row.get(4)?,
                skill: row.get(5)?,
                platform: row.get(6)?,
                chat_id: row.get(7)?,
                enabled: row.get::<_, i32>(8)? != 0,
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?).unwrap().with_timezone(&Utc),
                last_fired_at: row.get::<_, Option<String>>(10)?.map(|s| chrono::DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                fire_count: row.get(11)?,
                fail_count: row.get(12)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    /// Delete a cron job.
    pub fn delete_job(&self, id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.db.execute("DELETE FROM cron_jobs WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    /// Check for due jobs and fire them.
    /// This is called periodically by the daemon's main loop.
    pub fn tick(&self) {
        let now = Utc::now();

        match self.get_all_enabled() {
            Ok(jobs) => {
                for mut job in jobs {
                    if !is_due(&job, &now) {
                        continue;
                    }

                    let message = match job.skill.as_ref() {
                        Some(s) => format!("*Scheduled task:* {}\n\nSkill: `{s}`\n\n{}", job.name, job.task),
                        None => format!("*Scheduled task:* {}\n\n{}", job.name, job.task),
                    };

                    if let Some(ref push) = self.push {
                        push.send(&job.tenant_id, &job.chat_id, &message);
                        info!(job = %job.name, tenant = %job.tenant_id, "Cron job fired");
                    }

                    job.fire_count += 1;
                    job.last_fired_at = Some(now);
                    let _ = self.db.execute(
                        "UPDATE cron_jobs SET fire_count = ?1, last_fired_at = ?2 WHERE id = ?3",
                        rusqlite::params![job.fire_count, job.last_fired_at.map(|t| t.to_rfc3339()), job.id],
                    );
                }
            }
            Err(e) => {
                warn!(error = %e, "Cron tick failed");
            }
        }
    }

    fn get_all_enabled(&self) -> Result<Vec<CronJob>, Box<dyn std::error::Error>> {
        let mut stmt = self.db.prepare(
            "SELECT id, tenant_id, name, task, cron_expr, skill, platform, chat_id, enabled, created_at, last_fired_at, fire_count, fail_count
             FROM cron_jobs WHERE enabled = 1",
        )?;
        let jobs = stmt.query_map([], |row| {
            Ok(CronJob {
                id: row.get(0)?,
                tenant_id: row.get(1)?,
                name: row.get(2)?,
                task: row.get(3)?,
                cron_expr: row.get(4)?,
                skill: row.get(5)?,
                platform: row.get(6)?,
                chat_id: row.get(7)?,
                enabled: row.get::<_, i32>(8)? != 0,
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?).unwrap().with_timezone(&Utc),
                last_fired_at: row.get::<_, Option<String>>(10)?.map(|s| chrono::DateTime::parse_from_rfc3339(&s).unwrap().with_timezone(&Utc)),
                fire_count: row.get(11)?,
                fail_count: row.get(12)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }
}

/// Check if a cron job is due at the given time.
/// Supports 5-field cron: minute hour day month weekday.
fn is_due(job: &CronJob, now: &chrono::DateTime<Utc>) -> bool {
    let parts: Vec<&str> = job.cron_expr.split_whitespace().collect();
    if parts.len() != 5 {
        return false;
    }

    let fields = [
        (now.format("%M").to_string(), parts[0].to_string()), // minute
        (now.format("%H").to_string(), parts[1].to_string()), // hour
        (now.format("%d").to_string(), parts[2].to_string()), // day
        (now.format("%m").to_string(), parts[3].to_string()), // month
        (now.format("%u").to_string(), parts[4].to_string()), // weekday (1=Mon, 7=Sun)
    ];

    for (current, expr) in &fields {
        if !cron_matches(current, expr) {
            return false;
        }
    }

    // Don't fire if already fired this minute
    if let Some(last) = job.last_fired_at {
        if last.format("%Y-%m-%d %H:%M").to_string() == now.format("%Y-%m-%d %H:%M").to_string() {
            return false;
        }
    }

    true
}

/// Check if a value matches a cron field expression.
/// Supports: "*" (any), "N" (exact), "N,M" (list), "*/N" (step).
fn cron_matches(value: &str, expr: &str) -> bool {
    if expr == "*" { return true; }

    for part in expr.split(',') {
        let part = part.trim();

        if let Some(step_str) = part.strip_prefix("*/") {
            let step: u32 = step_str.parse().unwrap_or(1);
            let val: u32 = value.parse().unwrap_or(0);
            if step > 0 && val % step == 0 { return true; }
        } else {
            let val: u32 = value.parse().unwrap_or(0);
            let target: u32 = part.parse().unwrap_or(u32::MAX);
            if val == target { return true; }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_matches_star() {
        assert!(cron_matches("42", "*"));
        assert!(cron_matches("0", "*"));
    }

    #[test]
    fn test_cron_matches_exact() {
        assert!(cron_matches("42", "42"));
        assert!(!cron_matches("42", "0"));
    }

    #[test]
    fn test_cron_matches_list() {
        assert!(cron_matches("0", "0,30"));
        assert!(cron_matches("30", "0,30"));
        assert!(!cron_matches("15", "0,30"));
    }

    #[test]
    fn test_cron_matches_step() {
        assert!(cron_matches("0", "*/15"));
        assert!(cron_matches("15", "*/15"));
        assert!(cron_matches("30", "*/15"));
        assert!(!cron_matches("20", "*/15"));
    }

    #[test]
    fn test_is_due() {
        let job = CronJob::new("test", "daily", "test task", "0 9 * * *");
        let now = chrono::DateTime::parse_from_rfc3339("2026-07-08T09:00:00Z").unwrap().with_timezone(&Utc);
        assert!(is_due(&job, &now));

        let now2 = chrono::DateTime::parse_from_rfc3339("2026-07-08T09:01:00Z").unwrap().with_timezone(&Utc);
        assert!(!is_due(&job, &now2));
    }
}
