use crate::context::RepositoryContext;
use async_trait::async_trait;
use crate::error::{EloopError, Result};
use crate::policy::ExecutionPolicy;
use crate::task::{
    AcceptanceCriterion, CompiledTask, RiskLevel, VerificationCommand,
};
use chrono::Utc;
use std::path::PathBuf;
use uuid::Uuid;

#[async_trait]
pub trait TaskCompiler: Send + Sync {
    async fn compile(
        &self,
        goal: &str,
        context: &RepositoryContext,
        policy: &ExecutionPolicy,
    ) -> Result<CompiledTask>;
}

/// Deterministic bootstrap compiler used by the first `eloop plan` vertical slice.
///
/// A model-backed compiler can later replace this implementation while keeping
/// the same strict task contract and verifier.
#[derive(Debug, Default)]
pub struct TemplateTaskCompiler;

#[async_trait]
impl TaskCompiler for TemplateTaskCompiler {
    async fn compile(
        &self,
        goal: &str,
        context: &RepositoryContext,
        policy: &ExecutionPolicy,
    ) -> Result<CompiledTask> {
        let goal = goal.trim();
        if goal.is_empty() {
            return Err(EloopError::EmptyGoal);
        }

        let title = title_from_goal(goal);
        let verification_commands = context
            .verification_commands
            .iter()
            .map(|command| VerificationCommand {
                command: command.clone(),
                purpose: purpose_for_command(command),
                mandatory: true,
                timeout_seconds: 1_800,
            })
            .collect::<Vec<_>>();

        let mut notes = Vec::new();
        if verification_commands.is_empty() {
            notes.push(
                "No build system was detected; a model-backed compiler must discover explicit verification commands before execution."
                    .to_string(),
            );
        }
        if !context.git.status_porcelain.is_empty() {
            notes.push(format!(
                "Repository starts dirty with {} changed or untracked paths; preserve unrelated changes.",
                context.git.status_porcelain.len()
            ));
        }

        Ok(CompiledTask {
            schema_version: 1,
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            title,
            goal: goal.to_string(),
            background: background_from_context(context),
            repository: context.repository.clone(),
            allowed_paths: vec![PathBuf::from(".")],
            exclusions: vec![
                "Do not deploy or restart production workloads.".to_string(),
                "Do not publish releases or push images/packages.".to_string(),
                "Do not modify secrets, credentials, or Vault configuration.".to_string(),
                "Do not delete or migrate production data.".to_string(),
                "Do not commit unrelated pre-existing working tree changes.".to_string(),
            ],
            acceptance_criteria: vec![
                AcceptanceCriterion {
                    id: "AC-1".to_string(),
                    description: "The requested outcome is implemented within the allowed repository scope."
                        .to_string(),
                    mandatory: true,
                },
                AcceptanceCriterion {
                    id: "AC-2".to_string(),
                    description: "Regression coverage demonstrates the failure before the fix or protects the changed behavior."
                        .to_string(),
                    mandatory: true,
                },
                AcceptanceCriterion {
                    id: "AC-3".to_string(),
                    description: "All detected mandatory formatting, lint, test, and build commands pass with recorded evidence."
                        .to_string(),
                    mandatory: true,
                },
                AcceptanceCriterion {
                    id: "AC-4".to_string(),
                    description: "The final report distinguishes completed, failed, and unverified claims and lists remaining risks."
                        .to_string(),
                    mandatory: true,
                },
            ],
            verification_commands,
            risk: infer_risk(goal),
            policy: policy.clone(),
            expected_outputs: vec![
                "result.json".to_string(),
                "report.md".to_string(),
                "verification.json".to_string(),
                "changes.patch".to_string(),
                "token-usage.json".to_string(),
                "token-usage.md".to_string(),
            ],
            compiler_notes: notes,
        })
    }
}

fn title_from_goal(goal: &str) -> String {
    let first_line = goal.lines().next().unwrap_or(goal).trim();
    let mut title = first_line.trim_end_matches(&['.', '!', '?'][..]).to_string();
    if title.chars().count() > 96 {
        title = title.chars().take(93).collect::<String>() + "...";
    }
    if title.is_empty() {
        "Engineering task".to_string()
    } else {
        title
    }
}

fn background_from_context(context: &RepositoryContext) -> String {
    let mut parts = vec![format!(
        "This task targets the `{}` repository.",
        context.project_name
    )];
    if !context.detected_languages.is_empty() {
        parts.push(format!(
            "Detected languages: {}.",
            context.detected_languages.join(", ")
        ));
    }
    if !context.build_systems.is_empty() {
        parts.push(format!(
            "Detected build systems: {}.",
            context.build_systems.join(", ")
        ));
    }
    if context.git.available {
        parts.push(format!(
            "Git branch is `{}` at `{}`.",
            context.git.branch.as_deref().unwrap_or("detached"),
            context.git.head.as_deref().unwrap_or("unknown")
        ));
    }
    parts.join(" ")
}

fn purpose_for_command(command: &str) -> String {
    if command.contains("fmt") || command.contains("gofmt") {
        "Verify formatting".to_string()
    } else if command.contains("clippy") || command.contains("vet") || command.contains("lint") {
        "Verify static analysis".to_string()
    } else if command.contains("test") || command.contains("pytest") {
        "Run regression tests".to_string()
    } else if command.contains("build") || command.contains("compile") {
        "Verify compilation/build".to_string()
    } else {
        "Verify repository health".to_string()
    }
}

fn infer_risk(goal: &str) -> RiskLevel {
    let lower = goal.to_lowercase();
    if ["production data", "delete", "migration", "secrets", "vault"]
        .iter()
        .any(|word| lower.contains(word))
    {
        RiskLevel::Critical
    } else if ["deploy", "kubernetes", "k8s", "release", "database"]
        .iter()
        .any(|word| lower.contains(word))
    {
        RiskLevel::High
    } else if ["refactor", "architecture", "concurrency", "redis", "network"]
        .iter()
        .any(|word| lower.contains(word))
    {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{GitContext, RepositoryContext};

    fn context() -> RepositoryContext {
        RepositoryContext {
            repository: PathBuf::from("/tmp/demo"),
            project_name: "demo".to_string(),
            detected_languages: vec!["Rust".to_string()],
            build_systems: vec!["cargo".to_string()],
            verification_commands: vec!["cargo test --workspace".to_string()],
            instructions: Vec::new(),
            tree: Vec::new(),
            git: GitContext {
                available: true,
                branch: Some("main".to_string()),
                head: Some("abc".to_string()),
                status_porcelain: Vec::new(),
                recent_commits: Vec::new(),
            },
        }
    }

    #[tokio::test]
    async fn compiles_short_goal_into_contract() {
        let task = TemplateTaskCompiler
            .compile(
                "Harden Redis connection lifecycle",
                &context(),
                &ExecutionPolicy::default(),
            )
            .await
            .expect("compile");
        assert_eq!(task.title, "Harden Redis connection lifecycle");
        assert_eq!(task.verification_commands.len(), 1);
        assert_eq!(task.risk, RiskLevel::Medium);
    }

    #[tokio::test]
    async fn rejects_empty_goal() {
        let error = TemplateTaskCompiler
            .compile("  ", &context(), &ExecutionPolicy::default())
            .await
            .expect_err("must reject empty goal");
        assert!(matches!(error, EloopError::EmptyGoal));
    }
}
