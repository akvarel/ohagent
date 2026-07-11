//! Integration test: skills lifecycle end-to-end.
//!
//! Tests the full create → evaluate → curate pipeline
//! that the daemon's cron loop executes.

use chrono::Utc;
use uuid::Uuid;

use ohagent_memory::engine::MemoryEngine;
use ohagent_memory::models::{ConversationSummary, MemoryConfig, MemoryEntry, MemorySource};
use ohagent_skills::curator;
use ohagent_skills::creator;
use ohagent_skills::evaluator;
use ohagent_skills::models::{Skill, SkillConfig, SkillOrigin, SkillStatus};
use ohagent_skills::registry::SkillRegistry;

fn memory_config() -> MemoryConfig {
    MemoryConfig {
        db_path: format!("/tmp/ohagent-integration-memory-{}.db", Uuid::new_v4()),
        skip_embeddings: true,
        ..Default::default()
    }
}

fn skills_config() -> SkillConfig {
    SkillConfig {
        db_path: format!("/tmp/ohagent-integration-skills-{}.db", Uuid::new_v4()),
        ..Default::default()
    }
}

/// Full lifecycle: create skills from memory → evaluate → curate.
#[test]
fn test_full_skills_lifecycle() {
    let mem = MemoryEngine::open(memory_config()).expect("open memory");
    let reg = SkillRegistry::open(skills_config()).expect("open skills");
    let tenant = "integration-tenant";
    let now = Utc::now();

    // 1. Seed memory with conversations (MemoryEntries)
    let texts = vec![
        "deploy the application to kubernetes",
        "can you deploy to k8s cluster",
        "deploy the latest build please",
        "run database migration on production",
        "migrate the database schema",
        "run the migration script",
        "generate a PDF report from this data",
        "create a PDF file from the spreadsheet",
        "generate me a PDF please",
        "write unit tests for the auth module",
        "add tests for the login endpoint",
        "write tests for authentication",
    ];

    for text in &texts {
        let entry = MemoryEntry {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant.to_string(),
            session_id: "test-session".to_string(),
            content: text.to_string(),
            source: MemorySource::Conversation,
            importance: 0.7,
            tags: vec![],
            embedding: None,
            created_at: now,
            last_accessed_at: now,
            access_count: 0,
        };
        mem.remember(entry).expect("store memory entry");
    }

    // Also create a conversation summary (needed for the creator to find patterns)
    let summary = ConversationSummary {
        session_id: "test-session".to_string(),
        tenant_id: tenant.to_string(),
        summary: "User asked about deployment automation, database migrations, PDF generation, and unit testing. Complex recurring patterns detected across multiple sessions.".into(),
        key_points: vec![
            "Deployment automation recurring".to_string(),
            "Database migration pattern".to_string(),
            "PDF generation requested multiple times".to_string(),
        ],
        decisions: vec!["Use CI/CD for deployments".to_string()],
        start_time: now - chrono::Duration::hours(1),
        end_time: now,
        message_count: 15,
    };
    mem.summarize(summary).expect("store summary");

    // 2. Verify memory count
    let count = mem.count(tenant).expect("count");
    assert!(count >= 10, "Expected at least 10 memories, got {count}");

    // 3. Propose skills from conversation patterns
    let config = SkillConfig::default();
    let proposed = creator::propose_skills(&reg, &mem, tenant, &config)
        .expect("propose skills");
    assert!(proposed > 0, "Expected at least 1 proposed skill, got {proposed}");

    // 4. Verify skills were created with Proposed status
    let all = reg.list(tenant, None, 50).expect("list skills");
    assert!(!all.is_empty(), "Skills should have been created");
    for s in &all {
        assert_eq!(s.status, SkillStatus::Proposed);
    }

    // 5. Simulate usage: promote first skill by recording successes
    if let Some(skill) = all.first() {
        for i in 0..10 {
            evaluator::record_success(
                &reg,
                &skill.id,
                &format!("session-{i}"),
                tenant,
                Some(1.5),
            )
            .unwrap_or_else(|e| panic!("record success {i}: {e}"));
        }
    }

    // 6. Run periodic evaluation — should promote well-used skills
    let eval_report = evaluator::periodic_evaluation(&reg, tenant, &config)
        .expect("periodic evaluation");
    assert!(
        eval_report.promoted + eval_report.active > 0,
        "Expected some evaluation activity, got promoted={}, active={}",
        eval_report.promoted, eval_report.active
    );

    // Check that at least one skill is now Active
    let all_after = reg.list(tenant, None, 50).expect("list all");
    let has_active = all_after.iter().any(|s| s.status == SkillStatus::Active);
    assert!(has_active, "At least one skill should have been promoted to Active");

    // 7. Run curation — should not delete active skills
    let curate_report = curator::curate(&reg, tenant, &config).expect("curate");
    assert_eq!(curate_report.pruned, 0, "No skills should be pruned yet");

    // 8. Verify all_tenants()
    let tenants = reg.all_tenants().expect("all tenants");
    assert!(tenants.contains(&tenant.to_string()));

    // 9. Insert an old retired skill and verify pruning
    let old_skill = Skill {
        id: Uuid::new_v4().to_string(),
        tenant_id: tenant.to_string(),
        name: "old-retired".into(),
        description: "Should be pruned".into(),
        triggers: vec!["old".into()],
        instructions: "do old stuff".into(),
        version: "0.1.0".into(),
        origin: SkillOrigin::Auto,
        status: SkillStatus::Retired,
        created_at: now - chrono::Duration::days(100),
        updated_at: now - chrono::Duration::days(100),
        last_used_at: Some(now - chrono::Duration::days(100)),
        use_count: 1,
        success_count: 0,
        failure_count: 1,
        quality_score: 0.05,
        tags: vec!["old".into()],
        pinned: false,
    };
    reg.save(&old_skill).expect("save old skill");

    let prune_report = curator::curate(&reg, tenant, &config).expect("curate after old");
    assert!(prune_report.pruned > 0, "Old retired skill should be pruned");

    // Cleanup
    mem.clear_tenant(tenant).ok();
}

/// Verify that empty memory produces no skills.
#[test]
fn test_no_skills_from_empty_memory() {
    let mem = MemoryEngine::open(memory_config()).expect("open memory");
    let reg = SkillRegistry::open(skills_config()).expect("open skills");
    let tenant = "empty-tenant";
    let config = SkillConfig::default();

    let proposed = creator::propose_skills(&reg, &mem, tenant, &config)
        .expect("propose");
    assert_eq!(proposed, 0, "Empty memory should produce no skills");

    mem.clear_tenant(tenant).ok();
}

/// Verify that evaluation with no skills is a no-op.
#[test]
fn test_evaluation_no_skills() {
    let reg = SkillRegistry::open(skills_config()).expect("open skills");
    let tenant = "no-skills";
    let config = SkillConfig::default();

    let report = evaluator::periodic_evaluation(&reg, tenant, &config)
        .expect("evaluate empty");
    assert_eq!(report.active, 0);
    assert_eq!(report.promoted, 0);
}

/// Verify curator skips empty tenants.
#[test]
fn test_curation_no_skills() {
    let reg = SkillRegistry::open(skills_config()).expect("open skills");
    let tenant = "no-curate";
    let config = SkillConfig::default();

    let report = curator::curate(&reg, tenant, &config).expect("curate empty");
    assert_eq!(report.pruned, 0);
    assert_eq!(report.merged, 0);
}
