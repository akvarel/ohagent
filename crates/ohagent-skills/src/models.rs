//! Skill models — data types for the self-learning skill system.
//!
//! Skills are learned, versioned, reusable behaviors with usage tracking.
//! The system can propose, create, evaluate, and curate skills automatically.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A learned skill — a reusable behavior pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Unique skill ID (UUID v4).
    pub id: String,
    /// Tenant scope.
    pub tenant_id: String,
    /// Skill name (slug, e.g. "deploy-to-k8s").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// When to trigger this skill — key phrases or patterns.
    pub triggers: Vec<String>,
    /// The skill instructions / prompt.
    pub instructions: String,
    /// Semantic version (e.g. "1.0.0").
    pub version: String,
    /// How the skill was created.
    pub origin: SkillOrigin,
    /// Current lifecycle status.
    pub status: SkillStatus,
    /// When the skill was created.
    pub created_at: DateTime<Utc>,
    /// Last time the skill was updated.
    pub updated_at: DateTime<Utc>,
    /// Last time the skill was used.
    pub last_used_at: Option<DateTime<Utc>>,
    /// Total number of times used.
    pub use_count: u32,
    /// Number of successful invocations.
    pub success_count: u32,
    /// Number of failures.
    pub failure_count: u32,
    /// Composite quality score (0.0–1.0).
    pub quality_score: f32,
    /// Tags for categorization.
    pub tags: Vec<String>,
    /// Pinned — curator never archives or prunes pinned skills.
    pub pinned: bool,
}

/// How a skill came to exist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillOrigin {
    /// Created automatically from observed patterns.
    Auto,
    /// Created after user explicitly asked.
    Explicit,
    /// Imported from Jcode skill system (markdown skills).
    Imported,
    /// Merged from multiple similar skills.
    Merged,
}

impl std::fmt::Display for SkillOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillOrigin::Auto => write!(f, "auto"),
            SkillOrigin::Explicit => write!(f, "explicit"),
            SkillOrigin::Imported => write!(f, "imported"),
            SkillOrigin::Merged => write!(f, "merged"),
        }
    }
}

/// Lifecycle status of a skill.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillStatus {
    /// Proposed but not yet confirmed.
    Proposed,
    /// Active and available for use.
    Active,
    /// Temporarily disabled (low quality).
    Disabled,
    /// Permanently retired.
    Retired,
}

impl std::fmt::Display for SkillStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillStatus::Proposed => write!(f, "proposed"),
            SkillStatus::Active => write!(f, "active"),
            SkillStatus::Disabled => write!(f, "disabled"),
            SkillStatus::Retired => write!(f, "retired"),
        }
    }
}

/// A single usage event — records when a skill was invoked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUsage {
    /// Unique event ID.
    pub id: String,
    /// The skill that was used.
    pub skill_id: String,
    /// The session in which it was used.
    pub session_id: String,
    /// Tenant scope.
    pub tenant_id: String,
    /// Whether the invocation was successful.
    pub success: bool,
    /// User rating (None = no rating given, Some(1-5)).
    pub rating: Option<u8>,
    /// How long the skill execution took (seconds).
    pub duration_secs: Option<f64>,
    /// When it happened.
    pub timestamp: DateTime<Utc>,
}

/// A trigger pattern — used to detect when a skill should be suggested.
#[derive(Debug, Clone)]
pub struct SkillTrigger {
    /// The skill that would be triggered.
    pub skill_id: String,
    /// The skill name.
    pub skill_name: String,
    /// The trigger phrase that matched.
    pub matched_phrase: String,
    /// Confidence in this match (0.0–1.0).
    pub confidence: f32,
    /// The skill's quality score factored in.
    pub effective_score: f32,
}

impl Skill {
    /// Compute a quality score from usage statistics.
    pub fn compute_quality(&self) -> f32 {
        let total = self.use_count as f32;
        if total == 0.0 {
            return 0.5; // Neutral for new skills
        }

        let success_rate = if total > 0.0 {
            self.success_count as f32 / total
        } else {
            0.5
        };

        // Newer skills get a small boost
        let age_days = (Utc::now() - self.created_at).num_days().max(0) as f32;
        let novelty_bonus: f32 = if age_days < 7.0 { 0.1 } else { 0.0 };

        // Combine: success_rate * 0.7 + novelty * 0.3
        let score = success_rate * 0.7 + novelty_bonus.max(0.0) * 0.3;
        score.clamp(0.0, 1.0)
    }

    /// Check if the skill should be retired (very low quality, unused).
    pub fn should_retire(&self) -> bool {
        if self.use_count == 0 {
            let days_since_creation = (Utc::now() - self.created_at).num_days();
            return days_since_creation > 30; // Unused for 30 days
        }

        if self.use_count >= 5 && (self.success_count as f32 / self.use_count as f32) < 0.2 {
            return true; // >5 uses with <20% success rate
        }

        false
    }
}

/// Configuration for the skill engine.
#[derive(Debug, Clone)]
pub struct SkillConfig {
    /// Path to the skills database.
    pub db_path: String,
    /// Minimum usage count before auto-promotion from Proposed to Active.
    pub auto_promote_min_uses: u32,
    /// Minimum quality score to keep a skill active.
    pub min_quality_score: f32,
    /// Maximum skills per tenant (older low-quality ones get pruned).
    pub max_skills_per_tenant: usize,
    /// Days of inactivity before skill is considered stale.
    pub stale_days: i64,
}

impl Default for SkillConfig {
    fn default() -> Self {
        Self {
            db_path: "~/.ohagent/skills.db".to_string(),
            auto_promote_min_uses: 3,
            min_quality_score: 0.3,
            max_skills_per_tenant: 50,
            stale_days: 60,
        }
    }
}
