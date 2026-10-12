use crate::task::{CompiledTask, CritiqueIssue, TaskCritique};
use serde::{Deserialize, Serialize};

pub trait TaskVerifier: Send + Sync {
    fn critique(&self, task: &CompiledTask) -> TaskCritique;
}

#[derive(Debug, Default)]
pub struct ContractVerifier;

impl TaskVerifier for ContractVerifier {
    fn critique(&self, task: &CompiledTask) -> TaskCritique {
        let mut issues = Vec::new();

        if task.goal.trim().is_empty() {
            issues.push(issue("EMPTY_GOAL", "Goal is empty", true));
        }
        if task.acceptance_criteria.is_empty() {
            issues.push(issue(
                "NO_ACCEPTANCE_CRITERIA",
                "Task has no acceptance criteria",
                true,
            ));
        }
        if task.allowed_paths.is_empty() {
            issues.push(issue(
                "NO_SCOPE",
                "Task has no allowed path scope",
                true,
            ));
        }
        if task.verification_commands.is_empty() {
            issues.push(issue(
                "NO_VERIFICATION_COMMANDS",
                "No executable verification commands were discovered",
                true,
            ));
        }

        for verification in &task.verification_commands {
            let command = verification.command.trim();
            if command.is_empty() || command.eq_ignore_ascii_case("auto") {
                issues.push(issue(
                    "INVALID_VERIFICATION_COMMAND",
                    &format!("Invalid verification command: {:?}", verification.command),
                    true,
                ));
            }
        }

        if task.policy.deployment_allowed && task.risk != crate::task::RiskLevel::Critical {
            issues.push(issue(
                "DEPLOYMENT_POLICY_REVIEW",
                "Deployment is allowed; require explicit human review before execution",
                false,
            ));
        }

        TaskCritique {
            valid: !issues.iter().any(|item| item.fatal),
            issues,
        }
    }
}

fn issue(code: &str, message: &str, fatal: bool) -> CritiqueIssue {
    CritiqueIssue {
        code: code.to_string(),
        message: message.to_string(),
        fatal,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationResult {
    pub status: VerificationStatus,
    pub command_results: Vec<CommandVerification>,
    pub scope_violations: Vec<String>,
    pub unverified_claims: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerificationStatus {
    Pass,
    Fail,
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandVerification {
    pub command: String,
    pub exit_code: Option<i32>,
    pub passed: bool,
    pub stdout_path: Option<String>,
    pub stderr_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ExecutionPolicy;
    use crate::task::{AcceptanceCriterion, RiskLevel, VerificationCommand};
    use chrono::Utc;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn task(command: &str) -> CompiledTask {
        CompiledTask {
            schema_version: 1,
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            title: "demo".into(),
            goal: "demo".into(),
            background: "demo".into(),
            repository: PathBuf::from("."),
            allowed_paths: vec![PathBuf::from(".")],
            exclusions: Vec::new(),
            acceptance_criteria: vec![AcceptanceCriterion {
                id: "AC-1".into(),
                description: "works".into(),
                mandatory: true,
            }],
            verification_commands: vec![VerificationCommand {
                command: command.into(),
                purpose: "test".into(),
                mandatory: true,
                timeout_seconds: 30,
            }],
            risk: RiskLevel::Low,
            policy: ExecutionPolicy::default(),
            expected_outputs: Vec::new(),
            compiler_notes: Vec::new(),
        }
    }

    #[test]
    fn rejects_auto_placeholder() {
        let critique = ContractVerifier.critique(&task("auto"));
        assert!(!critique.valid);
        assert_eq!(critique.issues[0].code, "INVALID_VERIFICATION_COMMAND");
    }
}
