use crate::context::RepositoryContext;
use crate::policy::ExecutionPolicy;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub description: String,
    pub mandatory: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationCommand {
    pub command: String,
    pub purpose: String,
    pub mandatory: bool,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledTask {
    pub schema_version: u32,
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub title: String,
    pub goal: String,
    pub background: String,
    pub repository: PathBuf,
    pub allowed_paths: Vec<PathBuf>,
    pub exclusions: Vec<String>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub verification_commands: Vec<VerificationCommand>,
    pub risk: RiskLevel,
    pub policy: ExecutionPolicy,
    pub expected_outputs: Vec<String>,
    pub compiler_notes: Vec<String>,
}

impl CompiledTask {
    pub fn render_markdown(&self, context: &RepositoryContext) -> String {
        let mut output = String::new();
        output.push_str(&format!("# {}\n\n", self.title));
        output.push_str("## Goal\n\n");
        output.push_str(&self.goal);
        output.push_str("\n\n## Background\n\n");
        output.push_str(&self.background);
        output.push_str("\n\n## Repository context\n\n");
        output.push_str(&format!("- Repository: `{}`\n", self.repository.display()));
        output.push_str(&format!("- Languages: {}\n", context.detected_languages.join(", ")));
        output.push_str(&format!("- Build systems: {}\n", context.build_systems.join(", ")));
        if context.git.available {
            output.push_str(&format!(
                "- Git branch: `{}`\n",
                context.git.branch.as_deref().unwrap_or("detached")
            ));
            output.push_str(&format!(
                "- Working tree changes: {}\n",
                context.git.status_porcelain.len()
            ));
        }

        output.push_str("\n## Allowed scope\n\n");
        for path in &self.allowed_paths {
            output.push_str(&format!("- `{}`\n", path.display()));
        }

        output.push_str("\n## Explicit exclusions\n\n");
        for exclusion in &self.exclusions {
            output.push_str(&format!("- {exclusion}\n"));
        }

        output.push_str("\n## Acceptance criteria\n\n");
        for criterion in &self.acceptance_criteria {
            output.push_str(&format!(
                "- [{}] **{}** — {}\n",
                if criterion.mandatory { " " } else { "~" },
                criterion.id,
                criterion.description
            ));
        }

        output.push_str("\n## Required verification\n\n");
        for verification in &self.verification_commands {
            output.push_str(&format!(
                "```bash\n{}\n```\n\nPurpose: {}. Mandatory: `{}`. Timeout: `{}s`.\n\n",
                verification.command,
                verification.purpose,
                verification.mandatory,
                verification.timeout_seconds
            ));
        }

        output.push_str("## Safety policy\n\n");
        output.push_str(&format!(
            "- Production deployment allowed: `{}`\n- Release publication allowed: `{}`\n- Data migration allowed: `{}`\n- Destructive actions allowed: `{}`\n- Git push allowed: `{}`\n",
            self.policy.deployment_allowed,
            self.policy.release_publication_allowed,
            self.policy.data_migration_allowed,
            self.policy.destructive_actions_allowed,
            self.policy.git_push_allowed
        ));

        output.push_str("\n## Required outputs\n\n");
        for expected in &self.expected_outputs {
            output.push_str(&format!("- `{expected}`\n"));
        }

        output.push_str("\n## Completion rule\n\n");
        output.push_str(
            "Do not declare the task complete unless every mandatory acceptance criterion has direct evidence and every mandatory verification command has a recorded exit code. If verification cannot run, report `UNVERIFIED` rather than `PASS`. Do not perform deployment, release publication, data migration, destructive actions, or git push unless explicitly permitted above.\n",
        );
        output
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CritiqueIssue {
    pub code: String,
    pub message: String,
    pub fatal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskCritique {
    pub valid: bool,
    pub issues: Vec<CritiqueIssue>,
}
