use serde::{Deserialize, Serialize};

/// Safety policy applied to generated engineering tasks and future campaigns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionPolicy {
    pub deployment_allowed: bool,
    pub release_publication_allowed: bool,
    pub data_migration_allowed: bool,
    pub destructive_actions_allowed: bool,
    pub git_push_allowed: bool,
    pub require_clean_git: bool,
    pub stop_on_unverified: bool,
    pub max_tasks: u32,
    pub max_failures: u32,
    pub max_cost_usd: Option<f64>,
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            deployment_allowed: false,
            release_publication_allowed: false,
            data_migration_allowed: false,
            destructive_actions_allowed: false,
            git_push_allowed: false,
            require_clean_git: false,
            stop_on_unverified: true,
            max_tasks: 10,
            max_failures: 2,
            max_cost_usd: None,
        }
    }
}
