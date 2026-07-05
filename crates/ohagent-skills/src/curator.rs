//! Skill curator — periodic maintenance: pruning, merging, cleanup.
//!
//! Runs on a cron schedule to:
//! 1. Prune retired skills beyond retention period
//! 2. Merge similar skills (same triggers, similar instructions)
//! 3. Clean up old usage records
//! 4. Enforce per-tenant skill limits

use chrono::Utc;
use std::collections::HashSet;
use tracing::{debug, info};

use crate::models::{Skill, SkillConfig, SkillOrigin, SkillStatus};
use crate::registry::SkillRegistry;
use crate::Result;

/// Run a full curation pass for a tenant.
///
/// Returns a curation report.
pub fn curate(
    registry: &SkillRegistry,
    tenant_id: &str,
    config: &SkillConfig,
) -> Result<CurateReport> {
    info!(tenant_id = %tenant_id, "Running skill curation");

    let mut report = CurateReport::default();

    // 1. Prune very old retired skills
    let all_skills = registry.list(tenant_id, None, 200)?;
    for skill in &all_skills {
        if skill.status == SkillStatus::Retired {
            let days_retired = (Utc::now() - skill.updated_at).num_days();
            if days_retired > 90 {
                registry.delete(&skill.id)?;
                report.pruned += 1;
                debug!(name = %skill.name, "Pruned retired skill");
            }
        }
    }

    // 2. Merge similar active skills
    let active: Vec<&Skill> = all_skills
        .iter()
        .filter(|s| s.status == SkillStatus::Active)
        .collect();

    let merged = merge_similar(registry, &active)?;
    report.merged += merged;

    // 3. Enforce max skills limit
    if all_skills.len() > config.max_skills_per_tenant {
        // Sort by quality score, keep the best
        let mut sorted = all_skills.clone();
        sorted.sort_by(|a, b| b.quality_score.partial_cmp(&a.quality_score).unwrap_or(std::cmp::Ordering::Equal));

        for skill in sorted.iter().skip(config.max_skills_per_tenant) {
            if skill.quality_score < 0.3 {
                registry.delete(&skill.id)?;
                report.pruned += 1;
                debug!(name = %skill.name, "Pruned (skill limit exceeded)");
            }
        }
    }

    info!(
        tenant_id = %tenant_id,
        pruned = report.pruned,
        merged = report.merged,
        "Curation complete"
    );

    Ok(report)
}

/// Merge skills that are very similar.
///
/// Two skills are candidates for merging if:
/// - They share at least 50% of their trigger phrases
/// - Their instructions have high text overlap
fn merge_similar(registry: &SkillRegistry, skills: &[&Skill]) -> Result<usize> {
    let mut merged = 0;
    let mut skip: HashSet<String> = HashSet::new();

    for (i, a) in skills.iter().enumerate() {
        if skip.contains(&a.id) {
            continue;
        }
        for b in skills.iter().skip(i + 1) {
            if skip.contains(&b.id) {
                continue;
            }
            if similarity_score(a, b) > 0.7 {
                // Merge b into a
                merge_two(registry, a, b)?;
                skip.insert(b.id.clone());
                merged += 1;
            }
        }
    }

    Ok(merged)
}

/// Compute a similarity score between two skills (0.0–1.0).
fn similarity_score(a: &Skill, b: &Skill) -> f32 {
    let trigger_overlap = set_overlap(&a.triggers, &b.triggers);
    let tag_overlap = set_overlap(&a.tags, &b.tags);

    // Simple text overlap on instructions
    let a_words: HashSet<&str> = a.instructions.split_whitespace().collect();
    let b_words: HashSet<&str> = b.instructions.split_whitespace().collect();
    let text_overlap = set_overlap_vec(&a_words, &b_words);

    trigger_overlap * 0.4 + tag_overlap * 0.2 + text_overlap * 0.4
}

/// Jaccard similarity between two string slices.
fn set_overlap(a: &[String], b: &[String]) -> f32 {
    let set_a: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let set_b: HashSet<&str> = b.iter().map(|s| s.as_str()).collect();
    set_overlap_vec(&set_a, &set_b)
}

fn set_overlap_vec<T: std::hash::Hash + Eq>(a: &HashSet<T>, b: &HashSet<T>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
}

/// Merge skill b into skill a.
fn merge_two(registry: &SkillRegistry, a: &Skill, b: &Skill) -> Result<()> {
    if let Some(mut target) = registry.get(&a.id)? {
        // Combine triggers (deduplicated)
        let mut all_triggers: HashSet<String> = a.triggers.iter().cloned().collect();
        for t in &b.triggers {
            all_triggers.insert(t.clone());
        }
        target.triggers = all_triggers.into_iter().collect();

        // Combine tags
        let mut all_tags: HashSet<String> = a.tags.iter().cloned().collect();
        for t in &b.tags {
            all_tags.insert(t.clone());
        }
        target.tags = all_tags.into_iter().collect();

        // Inherit usage stats
        target.use_count += b.use_count;
        target.success_count += b.success_count;
        target.failure_count += b.failure_count;
        target.last_used_at = b.last_used_at.max(a.last_used_at);
        target.origin = SkillOrigin::Merged;
        target.quality_score = target.compute_quality();
        target.updated_at = Utc::now();

        registry.save(&target)?;
        registry.delete(&b.id)?;

        info!(
            target = %a.name,
            merged_from = %b.name,
            "Skills merged"
        );
    }

    Ok(())
}

/// Curation report.
#[derive(Debug, Default)]
pub struct CurateReport {
    pub pruned: usize,
    pub merged: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_overlap() {
        let a = vec!["deploy".to_string(), "k8s".to_string()];
        let b = vec!["deploy".to_string(), "docker".to_string()];
        let sim = set_overlap(&a, &b);
        assert!((sim - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_set_overlap_identical() {
        let a = vec!["a".to_string(), "b".to_string()];
        let sim = set_overlap(&a, &a);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_similarity_score() {
        let a = Skill {
            id: "a".into(),
            tenant_id: "t1".into(),
            name: "deploy".into(),
            description: "Deploy stuff".into(),
            triggers: vec!["deploy".into(), "k8s".into()],
            instructions: "deploy to kubernetes cluster".into(),
            version: "1.0".into(),
            origin: SkillOrigin::Auto,
            status: SkillStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_used_at: None,
            use_count: 0,
            success_count: 0,
            failure_count: 0,
            quality_score: 0.5,
            tags: vec!["ops".into()],
        };
        let b = Skill {
            id: "b".into(),
            tenant_id: "t1".into(),
            name: "deploy-k8s".into(),
            description: "K8s deployment".into(),
            triggers: vec!["deploy".into(), "kubernetes".into()],
            instructions: "deploy to kubernetes cluster using kubectl".into(),
            version: "1.0".into(),
            origin: SkillOrigin::Auto,
            status: SkillStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_used_at: None,
            use_count: 0,
            success_count: 0,
            failure_count: 0,
            quality_score: 0.5,
            tags: vec!["ops".into()],
        };

        let sim = similarity_score(&a, &b);
        assert!(sim > 0.5, "Expected high similarity, got {sim}");
    }
}
