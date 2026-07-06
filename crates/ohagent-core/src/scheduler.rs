//! Cron scheduler — autonomous background tasks.
//!
//! ohAgent can schedule and run recurring tasks:
//! - Reminders ("напомни в 15:00 проверить почту")
//! - Daily reports ("каждое утро в 9:00 пришли статистику")
//! - Periodic checks ("каждые 30 минут проверяй статус сервера")
//!
//! ## Architecture
//!
//! ```text
//! User: "напомни через 10 минут" → Scheduler.add(tenant, delay, message)
//!                                              ↓
//!                                     tokio::spawn(sleep + fire)
//!                                              ↓
//!                                     PushService.send(tenant, message)
//! ```

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tracing::{info, warn};

/// A scheduled job waiting to fire.
#[derive(Debug, Clone)]
pub struct ScheduledJob {
    pub id: String,
    pub tenant_id: String,
    pub message: String,
    pub fire_at: chrono::DateTime<chrono::Utc>,
}

/// Simple in-memory scheduler for one-shot reminders.
///
/// For persistent cron, use ohagent-cron crate with SQLite storage.
pub struct Scheduler {
    jobs: Arc<Mutex<Vec<ScheduledJob>>>,
    push: Option<Arc<crate::push::PushService>>,
}

impl Scheduler {
    pub fn new(push: Option<Arc<crate::push::PushService>>) -> Self {
        Self {
            jobs: Arc::new(Mutex::new(Vec::new())),
            push,
        }
    }

    /// Schedule a one-shot reminder.
    ///
    /// `delay` is relative to now. Returns the job ID.
    pub fn schedule_in(
        &self,
        tenant_id: &str,
        delay: Duration,
        message: &str,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let fire_at = chrono::Utc::now() + chrono::Duration::from_std(delay).unwrap();

        let job = ScheduledJob {
            id: id.clone(),
            tenant_id: tenant_id.to_string(),
            message: message.to_string(),
            fire_at,
        };

        {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.push(job.clone());
        }

        let push = self.push.clone();
        let tenant = tenant_id.to_string();
        let msg = message.to_string();
        let job_id = id.clone();

        // Only spawn if push is configured and tokio runtime is active
        if push.is_some() {
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                info!(tenant = %tenant, job = %job_id, "Scheduled job firing");

                if let Some(push) = push {
                    let result = push.send(&tenant, &msg).await;
                    if !result.success {
                        warn!(tenant = %tenant, error = ?result.error, "Scheduled push failed");
                    }
                }
            });
        }

        id
    }

    /// Schedule a job at an absolute time.
    pub fn schedule_at(
        &self,
        tenant_id: &str,
        at: chrono::DateTime<chrono::Utc>,
        message: &str,
    ) -> Option<String> {
        let now = chrono::Utc::now();
        if at <= now {
            return None; // already passed
        }
        let delay = (at - now).to_std().unwrap_or(Duration::from_secs(0));
        if delay.is_zero() {
            return None;
        }
        Some(self.schedule_in(tenant_id, delay, message))
    }

    /// List all pending jobs.
    pub fn list_jobs(&self) -> Vec<ScheduledJob> {
        let jobs = self.jobs.lock().unwrap();
        jobs.clone()
    }

    /// Cancel a job by ID.
    pub fn cancel(&self, job_id: &str) -> bool {
        let mut jobs = self.jobs.lock().unwrap();
        let len_before = jobs.len();
        jobs.retain(|j| j.id != job_id);
        jobs.len() < len_before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schedule_and_list() {
        let s = Scheduler::new(None);
        let id = s.schedule_in("t1", Duration::from_secs(3600), "test reminder");
        let jobs = s.list_jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id);
        assert_eq!(jobs[0].message, "test reminder");
    }

    #[test]
    fn test_cancel() {
        let s = Scheduler::new(None);
        let id = s.schedule_in("t1", Duration::from_secs(3600), "test");
        assert!(s.cancel(&id));
        assert!(s.list_jobs().is_empty());
    }

    #[test]
    fn test_schedule_at_past() {
        let s = Scheduler::new(None);
        let past = chrono::Utc::now() - chrono::Duration::hours(1);
        assert!(s.schedule_at("t1", past, "past").is_none());
    }
}
