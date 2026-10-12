use crate::context::RepositoryContext;
use crate::error::{io_error, EloopError, Result};
use crate::policy::ExecutionPolicy;
use crate::task::{CompiledTask, TaskCritique};
use crate::usage::UsageReport;
use chrono::Utc;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub trait RunStore: Send + Sync {
    fn create_plan_run(
        &self,
        goal: &str,
        context: &RepositoryContext,
        task: &CompiledTask,
        critique: &TaskCritique,
        usage: &UsageReport,
        policy: &ExecutionPolicy,
    ) -> Result<PathBuf>;
}

#[derive(Debug, Clone)]
pub struct FileRunStore {
    root: PathBuf,
}

impl FileRunStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl RunStore for FileRunStore {
    fn create_plan_run(
        &self,
        goal: &str,
        context: &RepositoryContext,
        task: &CompiledTask,
        critique: &TaskCritique,
        usage: &UsageReport,
        policy: &ExecutionPolicy,
    ) -> Result<PathBuf> {
        fs::create_dir_all(&self.root).map_err(|source| io_error(&self.root, source))?;
        let timestamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let run_name = format!("{timestamp}-{}", short_id(task.id));
        let final_path = self.root.join(run_name);
        if final_path.exists() {
            return Err(EloopError::RunAlreadyExists(final_path));
        }

        let temp_path = self.root.join(format!(".tmp-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_path).map_err(|source| io_error(&temp_path, source))?;

        let goal_document = serde_json::json!({
            "schema_version": 1,
            "goal": goal,
            "repository": context.repository.clone(),
            "created_at": Utc::now(),
            "policy": policy,
        });

        write_json(&temp_path.join("goal.json"), &goal_document)?;
        write_json(&temp_path.join("context.json"), context)?;
        write_json(&temp_path.join("task.json"), task)?;
        write_text(
            &temp_path.join("task.md"),
            &task.render_markdown(context),
        )?;
        write_json(&temp_path.join("critic.json"), critique)?;
        write_json(&temp_path.join("token-usage.json"), usage)?;
        write_text(
            &temp_path.join("token-usage.md"),
            &usage.render_markdown(),
        )?;

        fs::rename(&temp_path, &final_path).map_err(|source| io_error(&final_path, source))?;
        Ok(final_path)
    }
}

fn short_id(id: Uuid) -> String {
    let value = id.simple().to_string();
    value[..8].to_string()
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).map_err(|source| io_error(path, source))
}

fn write_text(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content).map_err(|source| io_error(path, source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{GitContext, RepositoryContext};
    use crate::task::{AcceptanceCriterion, RiskLevel, VerificationCommand};
    use chrono::Utc;

    #[test]
    fn persists_complete_plan_run_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repository = dir.path().join("repo");
        fs::create_dir(&repository).expect("repo");
        let context = RepositoryContext {
            repository: repository.clone(),
            project_name: "repo".into(),
            detected_languages: vec!["Rust".into()],
            build_systems: vec!["cargo".into()],
            verification_commands: vec!["cargo test".into()],
            instructions: Vec::new(),
            tree: Vec::new(),
            git: GitContext {
                available: false,
                branch: None,
                head: None,
                status_porcelain: Vec::new(),
                recent_commits: Vec::new(),
            },
        };
        let task = CompiledTask {
            schema_version: 1,
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            title: "demo".into(),
            goal: "demo".into(),
            background: "demo".into(),
            repository,
            allowed_paths: vec![PathBuf::from(".")],
            exclusions: Vec::new(),
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "AC-1".into(),
                description: "works".into(),
                mandatory: true,
            }],
            verification_commands: vec![VerificationCommand {
                command: "cargo test".into(),
                purpose: "tests".into(),
                mandatory: true,
                timeout_seconds: 30,
            }],
            risk: RiskLevel::Low,
            policy: ExecutionPolicy::default(),
            expected_outputs: Vec::new(),
            compiler_notes: Vec::new(),
        };
        let store = FileRunStore::new(dir.path().join("runs"));
        let run = store
            .create_plan_run(
                "demo",
                &context,
                &task,
                &TaskCritique {
                    valid: true,
                    issues: Vec::new(),
                },
                &UsageReport::default(),
                &ExecutionPolicy::default(),
            )
            .expect("create run");

        for filename in [
            "goal.json",
            "context.json",
            "task.json",
            "task.md",
            "critic.json",
            "token-usage.json",
            "token-usage.md",
        ] {
            assert!(run.join(filename).is_file(), "missing {filename}");
        }
    }
}
