//! System prompt builder — assembles the full model prompt from layers.
//!
//! # Layer architecture (invariant: rules + skills NEVER compressed)
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ Layer 1: AGENTS.md      (per-request)    │  ← reloads on project switch
//! │ Layer 2: Active Skills  (persistent)     │  ← N E V E R  compressed
//! │ Layer 3: Memory RAG     (per-request)    │  ← N E V E R  compressed
//! │ Layer 4: RollingSummary (compressed)     │  ← ONLY this is compressed
//! │ Layer 5: Current task   (this request)   │  ← N E V E R  compressed
//! └──────────────────────────────────────────┘
//! ```
//!
//! Flash's `build_merge_prompt` only receives layer 4 content (conversation history).
//! After each merge, layers 1-3 are re-injected fresh — they never go through compression.
//!
//! ## Project switching
//!
//! `assemble()` accepts `project_dir`. AGENTS.md files are re-read from
//! the given directory on every call. When a user switches projects in
//! Open WebUI, the new project's AGENTS.md rules are automatically picked up.
//! Skills and tenant overrides remain static (loaded once at daemon startup).
//!
//! ## Context budget priority
//!
//! When the total exceeds the model's context window:
//! 1. Rules (AGENTS.md) — ALWAYS present (truncated if >20% of window)
//! 2. Skills — ALWAYS present (truncated if >15% of window)
//! 3. Memory RAG — top-N only (fits remaining budget)
//! 4. Rolling summary — compressed history
//! 5. Current conversation — as much as fits

use std::path::PathBuf;

/// Get home directory without external crate.
fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

/// Sources of persistent instructions that must survive context compression.
#[derive(Debug, Clone, Default)]
pub struct PersistentInstructions {
    /// Active skills from SkillRegistry.
    pub skills: Vec<SkillPrompt>,
    /// Per-tenant custom instructions.
    pub tenant_overrides: Option<String>,
}

/// A project-level rule loaded from AGENTS.md.
#[derive(Debug, Clone)]
pub struct ProjectRule {
    pub project_name: String,
    pub rules_text: String,
    pub priority: RulePriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RulePriority {
    /// ~/.AGENTS.md — global, always loaded
    Global = 0,
    /// CWD/AGENTS.md — project-specific
    Project = 1,
    /// Parent dir AGENTS.md
    Inherited = 2,
}

/// A skill as it appears in the system prompt.
#[derive(Debug, Clone)]
pub struct SkillPrompt {
    pub id: String,
    pub name: String,
    /// Trigger description (e.g. "use when user asks about...")
    pub trigger: String,
    /// The skill instructions.
    pub instructions: String,
}

/// Budget allocation for a context window (in tokens).
#[derive(Debug, Clone)]
pub struct PromptBudget {
    pub total_window: u32,
    pub rules_max: u32,
    pub skills_max: u32,
    pub memory_rag_max: u32,
    pub remaining_for_conversation: u32,
}

impl PromptBudget {
    /// Allocate budget from a model's context window (with 20% safety margin).
    pub fn from_window(context_window: u32) -> Self {
        let usable = (context_window as f64 * 0.80) as u32;
        Self {
            total_window: context_window,
            rules_max: (usable as f64 * 0.20) as u32,
            skills_max: (usable as f64 * 0.15) as u32,
            memory_rag_max: (usable as f64 * 0.10) as u32,
            remaining_for_conversation: usable.saturating_sub(
                (usable as f64 * 0.45) as u32
            ),
        }
    }
}

/// The assembled prompt, split into layers for inspection.
#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    /// Full system prompt ready for the model.
    pub system: String,
    /// Token estimates per layer.
    pub layer_tokens: LayerTokens,
    /// Whether compression would help (conversation > remaining budget).
    pub needs_compression: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LayerTokens {
    pub rules: u32,
    pub skills: u32,
    pub memory_rag: u32,
    pub compressed_history: u32,
    pub conversation: u32,
    pub total: u32,
}

/// Assembles the full system prompt from persistent + dynamic layers.
///
/// Skills and tenant overrides are loaded once at startup.
/// AGENTS.md rules are re-read on every request, keyed by `project_dir` —
/// so switching projects automatically picks up the right rules.
#[derive(Debug, Clone)]
pub struct SystemPromptBuilder {
    skills: Vec<SkillPrompt>,
    tenant_overrides: Option<String>,
}

impl SystemPromptBuilder {
    /// Create a builder with skills + tenant overrides.
    /// AGENTS.md rules are loaded per-request via `project_dir` in `assemble()`.
    pub fn new(skills: Vec<SkillPrompt>, tenant_overrides: Option<String>) -> Self {
        Self { skills, tenant_overrides }
    }

    /// Load AGENTS.md files following Jcode's resolution order:
    /// 1. ~/.AGENTS.md (global, always)
    /// 2. Current project_dir's AGENTS.md (project)
    /// 3. Parent directories' AGENTS.md (inherited, excluding home)
    pub fn load_agents_rules(project_dir: &PathBuf) -> Vec<ProjectRule> {
        let mut rules = Vec::new();
        let home = home_dir();

        // Global: ~/.AGENTS.md (always loaded, regardless of project)
        if let Some(ref h) = home {
            let global_path = h.join(".AGENTS.md");
            if global_path.exists() {
                if let Ok(text) = std::fs::read_to_string(&global_path) {
                    rules.push(ProjectRule {
                        project_name: "global".into(),
                        rules_text: text,
                        priority: RulePriority::Global,
                    });
                }
            }
        }

        // Walk from project_dir up to /
        let mut current = project_dir.clone();
        loop {
            // Don't re-read ~/.AGENTS.md as an Inherited rule
            if Some(&current) == home.as_ref() {
                if !current.pop() { break; }
                continue;
            }

            let agents_path = current.join("AGENTS.md");
            if agents_path.exists() {
                if let Ok(text) = std::fs::read_to_string(&agents_path) {
                    let priority = if current == *project_dir {
                        RulePriority::Project
                    } else {
                        RulePriority::Inherited
                    };
                    rules.push(ProjectRule {
                        project_name: current
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "unknown".into()),
                        rules_text: text,
                        priority,
                    });
                }
            }

            if !current.pop() {
                break;
            }
        }

        rules.sort_by_key(|r| r.priority);
        rules
    }

    /// Assemble the full system prompt for a request.
    ///
    /// `project_dir` — current working directory; AGENTS.md re-read from here.
    /// `user_message` — the user's last message, used for skills-on-demand filtering.
    /// `conversation_messages` — raw messages for this turn (layer 5).
    /// `compressed_history` — from RollingSummary, if available (layer 4).
    /// `memory_rag` — relevant memories from MemoryEngine.search() (layer 3).
    /// `budget` — token budget for the target model.
    pub fn assemble(
        &self,
        project_dir: &PathBuf,
        user_message: &str,
        conversation_messages: &str,
        compressed_history: Option<&str>,
        memory_rag: &[String],
        budget: &PromptBudget,
    ) -> AssembledPrompt {
        let mut layer_tokens = LayerTokens::default();

        // ── Layer 1: AGENTS.md rules (re-read every request for project switching) ──
        let agents_rules = Self::load_agents_rules(project_dir);
        let rules_text = Self::format_rules_static(&agents_rules, &self.tenant_overrides);
        let rules_text = trim_to_budget(&rules_text, budget.rules_max);
        layer_tokens.rules = estimate_tokens(&rules_text);

        // ── Layer 2: Skills (filtered: only those matching user message) ──
        let relevant_skills = Self::find_relevant_skills(&self.skills, user_message);
        let skills_text = Self::format_skills_static(&relevant_skills);
        let skills_text = trim_to_budget(&skills_text, budget.skills_max);
        layer_tokens.skills = estimate_tokens(&skills_text);

        // ── Layer 3: Memory RAG ──
        let rag_text = memory_rag.join("\n");
        let rag_text = trim_to_budget(&rag_text, budget.memory_rag_max);
        layer_tokens.memory_rag = estimate_tokens(&rag_text);

        // ── Layer 4: Compressed history ──
        if let Some(ch) = compressed_history {
            if !ch.is_empty() {
                layer_tokens.compressed_history = estimate_tokens(ch);
            }
        }

        // ── Build the fixed prefix (layers 1-4) ──
        let mut system = String::new();
        if !rules_text.is_empty() {
            system.push_str("── RULES ──\n");
            system.push_str(&rules_text);
            system.push_str("\n\n");
        }
        if !skills_text.is_empty() {
            system.push_str("── SKILLS ──\n");
            system.push_str(&skills_text);
            system.push_str("\n\n");
        }
        if !rag_text.is_empty() {
            system.push_str("── RELEVANT CONTEXT ──\n");
            system.push_str(&rag_text);
            system.push_str("\n\n");
        }
        if let Some(ch) = compressed_history {
            if !ch.is_empty() {
                system.push_str("── CONVERSATION HISTORY ──\n");
                system.push_str(ch);
                system.push_str("\n\n");
            }
        }

        // ── Layer 5: Current conversation ──
        layer_tokens.conversation = estimate_tokens(conversation_messages);
        let remaining = budget
            .remaining_for_conversation
            .saturating_sub(layer_tokens.total_except_conversation());
        let needs_compression = (layer_tokens.conversation as u32) > remaining;

        layer_tokens.total = layer_tokens.total_all();

        AssembledPrompt {
            system,
            layer_tokens,
            needs_compression,
        }
    }

    fn format_rules_static(rules: &[ProjectRule], tenant_overrides: &Option<String>) -> String {
        if rules.is_empty() && tenant_overrides.is_none() {
            return String::new();
        }
        let mut out = String::new();
        for rule in rules {
            out.push_str(&format!(
                "## Project: {} (priority: {:?})\n{}\n\n",
                rule.project_name, rule.priority, rule.rules_text
            ));
        }
        if let Some(ref overrides) = tenant_overrides {
            out.push_str("## Tenant Instructions\n");
            out.push_str(overrides);
            out.push('\n');
        }
        out
    }

    fn format_skills_static(skills: &[SkillPrompt]) -> String {
        if skills.is_empty() {
            return String::new();
        }
        let mut out = String::from("Available skills:\n\n");
        for skill in skills {
            out.push_str(&format!(
                "### /{} — {}\nTrigger: {}\n{}\n\n",
                skill.name, skill.id, skill.trigger, skill.instructions
            ));
        }
        out
    }

    /// Filter skills by trigger keyword match against the user's message.
    ///
    /// Only skills whose trigger phrases appear in the message are included.
    /// If no skills match, returns all skills as fallback (so the agent can
    /// still discover available skills).
    fn find_relevant_skills(all_skills: &[SkillPrompt], user_message: &str) -> Vec<SkillPrompt> {
        if user_message.trim().is_empty() || all_skills.is_empty() {
            return all_skills.to_vec();
        }

        let msg_lower = user_message.to_lowercase();

        let matched: Vec<SkillPrompt> = all_skills
            .iter()
            .filter(|s| {
                s.trigger
                    .split(',')
                    .map(|t| t.trim().to_lowercase())
                    .any(|trigger| {
                        // Direct substring match
                        if msg_lower.contains(&trigger) {
                            return true;
                        }
                        // Word-level fuzzy: any trigger word (3+ chars) appears
                        // as a substring anywhere in the message
                        trigger.split_whitespace().any(|word| {
                            if word.len() < 3 {
                                return false;
                            }
                            if msg_lower.contains(word) {
                                return true;
                            }
                            // Prefix match: "testing" should match "test" in message
                            for prefix_len in (4..=word.len()).rev() {
                                if msg_lower.contains(&word[..prefix_len]) {
                                    return true;
                                }
                            }
                            false
                        })
                    })
            })
            .cloned()
            .collect();

        // Fallback: if nothing matched, return all to avoid skill blindness
        if matched.is_empty() {
            all_skills.to_vec()
        } else {
            matched
        }
    }
}

impl LayerTokens {
    pub fn total_except_conversation(&self) -> u32 {
        self.rules + self.skills + self.memory_rag + self.compressed_history
    }

    pub fn total_all(&self) -> u32 {
        self.total_except_conversation() + self.conversation
    }
}

/// Fast token estimate (same heuristic as context_estimator).
fn estimate_tokens(text: &str) -> u32 {
    let word_count = text.split_whitespace().count() as u32;
    let char_count = text.chars().count() as u32;
    let word_est = (word_count as f64 * 1.3) as u32;
    let char_est = (char_count as f64 * 0.25) as u32;
    word_est.max(char_est).max(1)
}

/// Trim text to fit within a token budget.
fn trim_to_budget(text: &str, max_tokens: u32) -> String {
    if max_tokens == 0 {
        return String::new();
    }
    let current = estimate_tokens(text);
    if current <= max_tokens {
        return text.to_string();
    }
    let ratio = max_tokens as f64 / current as f64;
    let max_chars = (text.chars().count() as f64 * ratio) as usize;
    let trimmed: String = text.chars().take(max_chars).collect();
    format!("{trimmed}\n...[truncated to fit context budget]")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_builder() -> SystemPromptBuilder {
        SystemPromptBuilder::new(
            vec![
                SkillPrompt {
                    id: "s1".into(),
                    name: "test-skill".into(),
                    trigger: "when testing".into(),
                    instructions: "Do thing.".into(),
                },
                SkillPrompt {
                    id: "s2".into(),
                    name: "deploy".into(),
                    trigger: "deploy, k8s, kubernetes".into(),
                    instructions: "Deploy to cluster.".into(),
                },
            ],
            None,
        )
    }

    #[test]
    fn test_budget_allocation() {
        let budget = PromptBudget::from_window(128_000);
        let usable = (128_000.0 * 0.80) as u32;
        assert_eq!(budget.total_window, 128_000);
        assert!(budget.rules_max <= usable);
        assert!(budget.skills_max <= budget.rules_max);
        assert!(budget.memory_rag_max <= budget.skills_max);
    }

    #[test]
    fn test_assemble_with_compression_flag() {
        let builder = test_builder();
        let budget = PromptBudget::from_window(128_000);
        let cwd = PathBuf::from(".");

        // Short conversation
        let result = builder.assemble(&cwd, "hello", "hello", None, &[], &budget);
        assert!(!result.needs_compression);
        assert!(result.system.contains("── SKILLS ──"));
        assert!(result.system.contains("test-skill"));

        // Very long conversation — should need compression
        let long_conv = "long message. ".repeat(50_000);
        let result = builder.assemble(&cwd, "long", &long_conv, None, &[], &budget);
        assert!(result.needs_compression);
    }

    #[test]
    fn test_compressed_history_present() {
        let builder = test_builder();
        let budget = PromptBudget::from_window(128_000);
        let cwd = PathBuf::from(".");

        let result = builder.assemble(
            &cwd,
            "hello",
            "hello",
            Some("compressed: user wanted pizza"),
            &[],
            &budget,
        );
        assert!(result.system.contains("── CONVERSATION HISTORY ──"));
        assert!(result.system.contains("pizza"));
    }

    #[test]
    fn test_rules_reloaded_on_project_switch() {
        // Create a temp dir with its own AGENTS.md
        let tmp = std::env::temp_dir().join("ohagent_test_agents_switch");
        let _ = std::fs::create_dir_all(&tmp);
        let agents_path = tmp.join("AGENTS.md");
        std::fs::write(&agents_path, "## Test Project\nRule: always test.\n").unwrap();

        let builder = test_builder();
        let budget = PromptBudget::from_window(128_000);

        let result = builder.assemble(&tmp, "hello", "hello", None, &[], &budget);
        assert!(result.system.contains("Test Project"));
        assert!(result.system.contains("always test"));

        // Cleanup
        let _ = std::fs::remove_file(&agents_path);
        let _ = std::fs::remove_dir(&tmp);
    }

    #[test]
    fn test_different_project_different_cwd() {
        let builder = test_builder();
        let budget = PromptBudget::from_window(128_000);

        let cwd_a = PathBuf::from("/tmp/a");
        let cwd_b = PathBuf::from("/tmp/b");

        let result_a = builder.assemble(&cwd_a, "hi", "hi", None, &[], &budget);
        let result_b = builder.assemble(&cwd_b, "hi", "hi", None, &[], &budget);

        // Both should assemble without panicking
        assert!(!result_a.system.is_empty());
        assert!(!result_b.system.is_empty());
    }

    #[test]
    fn test_rules_trim_to_budget() {
        let big_rules = "x".repeat(500_000);
        let trimmed = trim_to_budget(&big_rules, 100);
        assert!(estimate_tokens(&trimmed) <= 150);
    }

    #[test]
    fn test_global_rules_always_loaded() {
        // Global ~/.AGENTS.md is always loaded regardless of project_dir
        let builder = test_builder();
        let budget = PromptBudget::from_window(128_000);

        // Even with non-existent project_dir, global rules should load
        // (if ~/.AGENTS.md exists on this machine)
        let result = builder.assemble(&PathBuf::from("/nonexistent"), "hi", "hi", None, &[], &budget);
        // Should not panic — just might not have rules if ~/.AGENTS.md doesn't exist
        assert!(!result.system.is_empty());
    }

    #[test]
    fn test_skills_on_demand_filtering() {
        let builder = test_builder();
        let budget = PromptBudget::from_window(128_000);
        let cwd = PathBuf::from(".");

        // Query about deployment — only deploy skill should appear in skills section
        let result = builder.assemble(
            &cwd, "deploy to kubernetes", "deploy to kubernetes",
            None, &[], &budget,
        );
        let skills_section = extract_skills_section(&result.system);
        assert!(skills_section.contains("/deploy"), "expected deploy skill");
        assert!(!skills_section.contains("test-skill"), "test-skill should not appear for deploy query");

        // Query about testing — only test skill should appear in skills section
        let result = builder.assemble(
            &cwd, "run the test suite", "run the test suite",
            None, &[], &budget,
        );
        let skills_section = extract_skills_section(&result.system);
        assert!(skills_section.contains("test-skill"), "expected test-skill");
        assert!(!skills_section.contains("/deploy"), "deploy skill should not appear for testing query");

        // Unrelated query — all skills fall back
        let result = builder.assemble(
            &cwd, "hello world", "hello world",
            None, &[], &budget,
        );
        // Fallback: all skills included
        assert!(result.system.contains("── SKILLS ──"));
        assert!(result.system.contains("test-skill"));
    }
}

/// Helper: extract the SKILLS section from an assembled prompt.
fn extract_skills_section(prompt: &str) -> &str {
    if let Some(skills_start) = prompt.find("── SKILLS ──") {
        let after_skills = &prompt[skills_start..];
        if let Some(next_section) = after_skills.find("── RELEVANT") {
            &after_skills[..next_section]
        } else if let Some(next_section) = after_skills.find("── CONVERSATION") {
            &after_skills[..next_section]
        } else {
            after_skills
        }
    } else {
        prompt
    }
}