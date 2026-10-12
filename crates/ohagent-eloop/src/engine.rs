use crate::compiler::TaskCompiler;
use crate::context::RepositoryContextCollector;
use crate::error::{EloopError, Result};
use crate::policy::ExecutionPolicy;
use crate::storage::RunStore;
use crate::task::{CompiledTask, TaskCritique};
use crate::usage::UsageReport;
use crate::verifier::TaskVerifier;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalRequest {
    pub goal: String,
    pub repository: PathBuf,
    pub policy: ExecutionPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanResult {
    pub run_directory: PathBuf,
    pub task: CompiledTask,
    pub critique: TaskCritique,
}

pub struct EloopEngine<C, T, V, S> {
    context_collector: C,
    compiler: T,
    verifier: V,
    store: S,
}

impl<C, T, V, S> EloopEngine<C, T, V, S>
where
    C: RepositoryContextCollector,
    T: TaskCompiler,
    V: TaskVerifier,
    S: RunStore,
{
    pub fn new(context_collector: C, compiler: T, verifier: V, store: S) -> Self {
        Self {
            context_collector,
            compiler,
            verifier,
            store,
        }
    }

    pub async fn plan_goal(&self, request: GoalRequest) -> Result<PlanResult> {
        if request.goal.trim().is_empty() {
            return Err(EloopError::EmptyGoal);
        }
        let context = self.context_collector.collect(&request.repository)?;
        let task = self
            .compiler
            .compile(&request.goal, &context, &request.policy)
            .await?;
        let critique = self.verifier.critique(&task);
        if !critique.valid {
            let details = critique
                .issues
                .iter()
                .filter(|issue| issue.fatal)
                .map(|issue| format!("{}: {}", issue.code, issue.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(EloopError::InvalidTask(details));
        }

        // The bootstrap compiler is deterministic and consumes no model tokens.
        let usage = UsageReport::default();
        let run_directory = self.store.create_plan_run(
            &request.goal,
            &context,
            &task,
            &critique,
            &usage,
            &request.policy,
        )?;

        Ok(PlanResult {
            run_directory,
            task,
            critique,
        })
    }
}
