//! Cron job definitions for ohAgent background tasks.
//!
//! Each job is an agent task scheduled to run at specific times.
//! Jobs are persisted in SQLite for durability across daemon restarts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A scheduled agent task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Unique job identifier.
    pub id: String,
    /// The tenant this job belongs to.
    pub tenant_id: String,
    /// Human-readable name for the job.
    pub name: String,
    /// The task/prompt to execute when the job fires.
    pub task: String,
    /// Cron expression (e.g. "0 9 * * *" for daily at 9 AM).
    /// Supports standard 5-field cron syntax.
    pub cron_expr: String,
    /// Optional skill name to attach to the task.
    pub skill: Option<String>,
    /// Target platform for delivery (e.g. "telegram").
    pub platform: String,
    /// Target chat ID for delivery.
    pub chat_id: String,
    /// Whether this job is enabled.
    pub enabled: bool,
    /// When this job was created.
    pub created_at: DateTime<Utc>,
    /// The last time this job fired successfully.
    pub last_fired_at: Option<DateTime<Utc>>,
    /// Total number of times this job has fired.
    pub fire_count: u64,
    /// Total number of failures.
    pub fail_count: u64,
}

impl CronJob {
    /// Create a new cron job.
    pub fn new(
        tenant_id: impl Into<String>,
        name: impl Into<String>,
        task: impl Into<String>,
        cron_expr: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id: tenant_id.into(),
            name: name.into(),
            task: task.into(),
            cron_expr: cron_expr.into(),
            skill: None,
            platform: "telegram".into(),
            chat_id: String::new(),
            enabled: true,
            created_at: Utc::now(),
            last_fired_at: None,
            fire_count: 0,
            fail_count: 0,
        }
    }

    /// Attach a skill to this job.
    pub fn with_skill(mut self, skill: impl Into<String>) -> Self {
        self.skill = Some(skill.into());
        self
    }

    /// Set the delivery target.
    pub fn with_delivery(mut self, platform: impl Into<String>, chat_id: impl Into<String>) -> Self {
        self.platform = platform.into();
        self.chat_id = chat_id.into();
        self
    }

    /// Disable this job.
    pub fn disable(mut self) -> Self {
        self.enabled = false;
        self
    }
}
