use crate::error::Result;
use async_trait::async_trait;
use crate::task::CompiledTask;
use crate::usage::UsageReport;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Context passed to a task executor. The future Jcode adapter implements
/// [`TaskExecutor`] using the existing `JcodeBridge` and `SessionHandle`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub repository: PathBuf,
    pub run_directory: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub summary: String,
    pub changed_files: Vec<PathBuf>,
    pub proposed_next_tasks: Vec<ProposedTask>,
    pub usage: UsageReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedTask {
    pub title: String,
    pub goal: String,
    pub reason: String,
    pub priority: u32,
    pub dependencies: Vec<String>,
    pub acceptance_criteria: Vec<String>,
}

#[async_trait]
pub trait TaskExecutor: Send + Sync {
    async fn execute(
        &self,
        task: &CompiledTask,
        context: &ExecutionContext,
    ) -> Result<ExecutionResult>;
}
