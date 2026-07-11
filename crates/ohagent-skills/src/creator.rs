//! Skill creator — automatic skill generation from conversation patterns.
//!
//! Pipeline:
//! 1. Analyze recent conversation summaries for recurring patterns
//! 2. Cluster similar tasks
//! 3. Generate skill descriptions and instructions
//! 4. Propose as new skills (status: Proposed)

use chrono::Utc;
use tracing::{debug, info};
use uuid::Uuid;

use crate::models::{Skill, SkillConfig, SkillOrigin, SkillStatus};
use crate::registry::SkillRegistry;
use crate::Result;
use ohagent_memory::engine::MemoryEngine;
use ohagent_memory::models::ConversationSummary;

/// Analyze recent conversations and propose new skills.
///
/// Returns the number of new skills proposed.
pub fn propose_skills(
    registry: &SkillRegistry,
    memory: &MemoryEngine,
    tenant_id: &str,
    _config: &SkillConfig,
) -> Result<usize> {
    info!(tenant_id = %tenant_id, "Analyzing conversations for skill patterns");

    // Get recent conversation summaries
    let recent = memory.list(tenant_id, None, 20)?;

    if recent.len() < 2 {
        debug!("Not enough data for pattern extraction");
        return Ok(0);
    }

    // Extract potential patterns from conversation content
    let patterns = extract_patterns(&recent);

    let mut created = 0;

    for pattern in patterns {
        // Skip if too similar to existing skill
        if let Some(_existing) = registry.find_by_name(tenant_id, &pattern.name)? {
            debug!(name = %pattern.name, "Skill already exists, skipping");
            continue;
        }

        let skill = Skill {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant_id.to_string(),
            name: pattern.name,
            description: pattern.description,
            triggers: pattern.triggers,
            instructions: pattern.instructions,
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
            tags: pattern.tags,
            pinned: false,
        };

        registry.save(&skill)?;
        info!(name = %skill.name, "New skill proposed");
        created += 1;
    }

    Ok(created)
}

/// A candidate pattern extracted from conversation analysis.
#[derive(Debug, Clone)]
struct SkillPattern {
    name: String,
    description: String,
    triggers: Vec<String>,
    instructions: String,
    tags: Vec<String>,
}

/// Extract recurring patterns from conversation summaries.
///
/// Uses simple keyword co-occurrence to detect patterns.
/// In production, this would use the LLM for richer extraction.
fn extract_patterns(memories: &[ohagent_memory::models::MemoryEntry]) -> Vec<SkillPattern> {
    // Collect all task keywords from memories
    let mut task_keywords: std::collections::HashMap<String, Vec<&str>> = std::collections::HashMap::new();

    let task_verbs = [
        "deploy", "build", "test", "fix", "add", "remove", "update", "create",
        "configure", "install", "setup", "run", "debug", "optimize", "refactor",
        "migrate", "backup", "restore", "monitor", "scale",
    ];

    for mem in memories {
        let content_lower = mem.content.to_lowercase();
        for verb in &task_verbs {
            if content_lower.contains(verb) {
                // Find the noun phrase after the verb
                if let Some(idx) = content_lower.find(verb) {
                    let after: String = content_lower[idx + verb.len()..]
                        .chars()
                        .take(60)
                        .collect();
                    let _phrase = format!("{verb} {}", after.trim());
                    task_keywords
                        .entry(verb.to_string())
                        .or_default()
                        .push(mem.content.as_str());
                }
            }
        }
    }

    let mut patterns = Vec::new();

    for (verb, examples) in &task_keywords {
        if examples.len() < 2 {
            continue; // Need at least 2 occurrences
        }

        let name = slugify(&format!("{verb}-task"));
        let description = format!("Automatically handle tasks related to: {verb}");
        let triggers = vec![
            format!("{verb}"),
            format!("can you {verb}"),
        ];
        let instructions = build_instructions(verb, examples);
        let tags = vec![verb.clone(), "auto-generated".into()];

        patterns.push(SkillPattern {
            name,
            description,
            triggers,
            instructions,
            tags,
        });
    }

    // Limit to top 5 patterns
    patterns.truncate(5);
    patterns
}

/// Build instructions from example tasks.
fn build_instructions(verb: &str, examples: &[&str]) -> String {
    let mut instructions = format!(
        "When asked to {verb} something:\n\
         1. Understand what needs to be {verb}ed\n\
         2. Check for any existing configuration or constraints\n\
         3. Execute the {verb} operation\n\
         4. Verify the result\n\
         5. Report back to the user\n\n\
         Based on past patterns:\n"
    );

    for (i, example) in examples.iter().take(3).enumerate() {
        let snippet: String = example.chars().take(120).collect();
        instructions.push_str(&format!("  {}. \"{snippet}...\"\n", i + 1));
    }

    instructions
}

/// Convert string to URL-friendly slug.
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Build a prompt for the LLM to generate a richer skill.
///
/// This can be used instead of the simple keyword extraction above
/// for higher-quality skill generation.
pub fn build_skill_generation_prompt(
    summaries: &[ConversationSummary],
) -> String {
    let mut context = String::from(
        "You are a skill extraction system. Based on the following conversation summaries, \
         identify recurring task patterns that could become reusable skills.\n\n\
         For each pattern, provide:\n\
         - name: a short slug\n\
         - description: what the skill does\n\
         - triggers: phrases that indicate the skill should be used\n\
         - instructions: step-by-step guidance\n\n\
         Respond as JSON array:\n\
         [{\"name\":\"...\",\"description\":\"...\",\"triggers\":[\"...\"],\"instructions\":\"...\"}]\n\n\
         Conversation summaries:\n"
    );

    for summary in summaries {
        context.push_str(&format!(
            "- {}: {}\n  Key points: {}\n",
            summary.session_id,
            summary.summary,
            summary.key_points.join(", "),
        ));
    }

    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Deploy to K8s!"), "deploy-to-k8s");
        assert_eq!(slugify("Fix bug #42"), "fix-bug-42");
    }

    #[test]
    fn test_extract_patterns_from_real_content() {
        let memories: Vec<ohagent_memory::models::MemoryEntry> = vec![
            ohagent_memory::models::MemoryEntry {
                id: "m1".into(),
                tenant_id: "t1".into(),
                session_id: "s1".into(),
                content: "Deploy the backend service to Kubernetes".into(),
                source: ohagent_memory::models::MemorySource::Conversation,
                importance: 0.8,
                tags: vec![],
            pinned: false,
                embedding: None,
                created_at: Utc::now(),
                last_accessed_at: Utc::now(),
                access_count: 0,
            },
            ohagent_memory::models::MemoryEntry {
                id: "m2".into(),
                tenant_id: "t1".into(),
                session_id: "s2".into(),
                content: "Deploy the frontend to production".into(),
                source: ohagent_memory::models::MemorySource::Conversation,
                importance: 0.7,
                tags: vec![],
            pinned: false,
                embedding: None,
                created_at: Utc::now(),
                last_accessed_at: Utc::now(),
                access_count: 0,
            },
        ];

        let patterns = extract_patterns(&memories);
        // Should find "deploy" pattern
        assert!(!patterns.is_empty());
        assert!(patterns.iter().any(|p| p.name.contains("deploy")));
    }
}
