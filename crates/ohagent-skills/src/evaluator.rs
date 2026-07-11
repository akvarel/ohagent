//! Skill evaluator — tracks skill usage and computes quality scores.
//!
//! After each skill invocation, records the result and updates
//! the skill's quality score. Periodically promotes/demotes skills
//! based on their performance.

use chrono::Utc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::models::{SkillConfig, SkillStatus, SkillUsage};
use crate::registry::SkillRegistry;
use crate::Result;

/// Record a successful skill invocation.
pub fn record_success(
    registry: &SkillRegistry,
    skill_id: &str,
    session_id: &str,
    tenant_id: &str,
    duration_secs: Option<f64>,
) -> Result<()> {
    let usage = SkillUsage {
        id: Uuid::new_v4().to_string(),
        skill_id: skill_id.to_string(),
        session_id: session_id.to_string(),
        tenant_id: tenant_id.to_string(),
        success: true,
        rating: None,
        duration_secs,
        timestamp: Utc::now(),
    };
    registry.record_usage(&usage)?;
    debug!(skill_id = %skill_id, "Skill success recorded");
    Ok(())
}

/// Record a failed skill invocation.
pub fn record_failure(
    registry: &SkillRegistry,
    skill_id: &str,
    session_id: &str,
    tenant_id: &str,
    duration_secs: Option<f64>,
) -> Result<()> {
    let usage = SkillUsage {
        id: Uuid::new_v4().to_string(),
        skill_id: skill_id.to_string(),
        session_id: session_id.to_string(),
        tenant_id: tenant_id.to_string(),
        success: false,
        rating: None,
        duration_secs,
        timestamp: Utc::now(),
    };
    registry.record_usage(&usage)?;
    debug!(skill_id = %skill_id, "Skill failure recorded");
    Ok(())
}

/// Periodically evaluate and update all skill statuses.
///
/// - Promotes Proposed → Active when meeting quality threshold
/// - Disables skills that consistently perform poorly
/// - Retires skills that haven't been used in a long time
pub fn periodic_evaluation(
    registry: &SkillRegistry,
    tenant_id: &str,
    config: &SkillConfig,
) -> Result<EvaluationReport> {
    info!(tenant_id = %tenant_id, "Running periodic skill evaluation");

    let skills = registry.list(tenant_id, None, config.max_skills_per_tenant)?;
    let mut report = EvaluationReport::default();

    for skill in &skills {
        let new_score = skill.compute_quality();

        match skill.status {
            SkillStatus::Proposed => {
                if skill.use_count >= config.auto_promote_min_uses {
                    if let Some(mut updated) = registry.get(&skill.id)? {
                        updated.status = SkillStatus::Active;
                        updated.quality_score = new_score;
                        updated.updated_at = Utc::now();
                        registry.save(&updated)?;
                        report.promoted += 1;
                        info!(name = %skill.name, "Skill promoted to Active");
                    }
                }
            }

            SkillStatus::Active => {
                // Update quality score
                if (new_score - skill.quality_score).abs() > 0.01 {
                    if let Some(mut updated) = registry.get(&skill.id)? {
                        updated.quality_score = new_score;
                        updated.updated_at = Utc::now();
                        registry.save(&updated)?;
                    }
                }

                if skill.should_retire() {
                    if let Some(mut updated) = registry.get(&skill.id)? {
                        updated.status = SkillStatus::Retired;
                        updated.updated_at = Utc::now();
                        registry.save(&updated)?;
                        report.retired += 1;
                        warn!(name = %skill.name, "Skill retired");
                    }
                } else if new_score < config.min_quality_score && skill.use_count >= 5 {
                    if let Some(mut updated) = registry.get(&skill.id)? {
                        updated.status = SkillStatus::Disabled;
                        updated.updated_at = Utc::now();
                        registry.save(&updated)?;
                        report.disabled += 1;
                        info!(name = %skill.name, score = new_score, "Skill disabled (low quality)");
                    }
                } else {
                    report.active += 1;
                }
            }

            SkillStatus::Disabled => {
                // Check if it improved
                if new_score >= config.min_quality_score {
                    if let Some(mut updated) = registry.get(&skill.id)? {
                        updated.status = SkillStatus::Active;
                        updated.quality_score = new_score;
                        updated.updated_at = Utc::now();
                        registry.save(&updated)?;
                        report.reactivated += 1;
                        info!(name = %skill.name, "Skill reactivated");
                    }
                } else {
                    report.disabled += 1;
                }
            }

            SkillStatus::Retired => {
                report.retired += 1;
            }
        }
    }

    info!(
        tenant_id = %tenant_id,
        total = skills.len(),
        active = report.active,
        promoted = report.promoted,
        disabled = report.disabled,
        retired = report.retired,
        reactivated = report.reactivated,
        "Evaluation complete"
    );

    Ok(report)
}

/// Result of a periodic evaluation run.
#[derive(Debug, Default)]
pub struct EvaluationReport {
    pub active: usize,
    pub promoted: usize,
    pub disabled: usize,
    pub retired: usize,
    pub reactivated: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Skill, SkillConfig, SkillOrigin, SkillStatus};
    use crate::registry::SkillRegistry;

    fn test_config() -> SkillConfig {
        SkillConfig {
            db_path: ":memory:".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_record_success_and_recompute() {
        let reg = SkillRegistry::open(test_config()).unwrap();
        let skill = Skill {
            id: Uuid::new_v4().to_string(),
            tenant_id: "t1".into(),
            name: "test-skill".into(),
            description: "test".into(),
            triggers: vec![],
            instructions: "test".into(),
            version: "0.1.0".into(),
            origin: SkillOrigin::Auto,
            status: SkillStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_used_at: None,
            use_count: 0,
            success_count: 0,
            failure_count: 0,
            quality_score: 0.5,
            tags: vec![],
            pinned: false,
        };
        reg.save(&skill).unwrap();

        record_success(&reg, &skill.id, "s1", "t1", Some(1.0)).unwrap();
        record_success(&reg, &skill.id, "s2", "t1", Some(2.0)).unwrap();

        let updated = reg.get(&skill.id).unwrap().unwrap();
        assert_eq!(updated.use_count, 2);
        assert_eq!(updated.success_count, 2);
        assert!(updated.quality_score > 0.5); // Success should boost score
    }

    #[test]
    fn test_periodic_evaluation_promotes() {
        let reg = SkillRegistry::open(test_config()).unwrap();
        let skill = Skill {
            id: Uuid::new_v4().to_string(),
            tenant_id: "t1".into(),
            name: "to-promote".into(),
            description: "test".into(),
            triggers: vec![],
            instructions: "test".into(),
            version: "0.1.0".into(),
            origin: SkillOrigin::Auto,
            status: SkillStatus::Proposed,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_used_at: None,
            use_count: 0,
            success_count: 0,
            failure_count: 0,
            quality_score: 0.5,
            tags: vec![],
            pinned: false,
        };
        reg.save(&skill).unwrap();

        // Record enough successes to meet auto_promote_min_uses
        for i in 0..3 {
            record_success(&reg, &skill.id, &format!("s{i}"), "t1", None).unwrap();
        }

        let config = test_config();
        let report = periodic_evaluation(&reg, "t1", &config).unwrap();
        assert_eq!(report.promoted, 1);

        let updated = reg.get(&skill.id).unwrap().unwrap();
        assert_eq!(updated.status, SkillStatus::Active);
    }
}
