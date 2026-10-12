//! Reporting contracts shared by plan, execution, and campaign stages.

use crate::usage::UsageReport;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskReport {
    pub requested_outcome: String,
    pub summary: String,
    pub completed: Vec<String>,
    pub failed: Vec<String>,
    pub unverified: Vec<String>,
    pub remaining_risks: Vec<String>,
    pub next_task: Option<String>,
    pub usage: UsageReport,
}
