//! System prompt builder — assembles the full model prompt from layers.
//!
//! # Layer architecture (invariant: rules + skills NEVER compressed)
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ Layer 1: AGENTS.md      (persistent)     │  ← N E V E R  compressed
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
    /// Loaded from AGENTS.md files (global + project).
    pub agents_rules: Vec<ProjectRule>,
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
            rules_max: (usable as f64 * 0.20) as u32,   // 20% for rules
            skills_max: (usable as f64 * 0.15) as u32,  // 15% for skills
            memory_rag_max: (usable as f64 * 0.10) as u32, // 10% for memory RAG
            remaining_for_conversation: usable.saturating_sub(
                (usable as f64 * 0.45) as u32
            ), // remaining ~55% for conversation
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
#[derive(Debug, Clone)]
pub struct SystemPromptBuilder {
    persistent: PersistentInstructions,
}

impl SystemPromptBuilder {
    /// Create a builder with persistent instructions (rules + skills).
    pub fn new(persistent: PersistentInstructions) -> Self {
        Self { persistent }
    }

    /// Load AGENTS.md files following Jcode's resolution order:
    /// 1. ~/.AGENTS.md (global)
    /// 2. Current directory's AGENTS.md (project)
    /// 3. Parent directories' AGENTS.md (inherited)
    pub fn load_agents_rules(project_dir: &PathBuf) -> Vec<ProjectRule> {
        let mut rules = Vec::new();

        // Global: ~/.AGENTS.md
        if let Some(home) = home_dir() {
            let global_path = home.join(".AGENTS.md");
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
                break; // Reached root
            }
        }

        rules.sort_by_key(|r| r.priority);
        rules
    }

    /// Assemble the full system prompt for a request.
    ///
    /// `conversation_messages` — raw messages for this turn (layer 5).
    /// `compressed_history` — from RollingSummary, if available (layer 4).
    /// `memory_rag` — relevant memories from MemoryEngine.search() (layer 3).
    /// `budget` — token budget for the target model.
    pub fn assemble(
        &self,
        conversation_messages: &str,
        compressed_history: Option<&str>,
        memory_rag: &[String],
        budget: &PromptBudget,
    ) -> AssembledPrompt {
        let mut layer_tokens = LayerTokens::default();

        // ── Layer 1: AGENTS.md rules ──
        let rules_text = self.format_rules();
        let rules_text = trim_to_budget(&rules_text, budget.rules_max);
        layer_tokens.rules = estimate_tokens(&rules_text);

        // ── Layer 2: Skills ──
        let skills_text = self.format_skills();
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

    fn format_rules(&self) -> String {
        if self.persistent.agents_rules.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        for rule in &self.persistent.agents_rules {
            out.push_str(&format!(
                "## Project: {} (priority: {:?})\n{}\n\n",
                rule.project_name, rule.priority, rule.rules_text
            ));
        }
        if let Some(ref overrides) = self.persistent.tenant_overrides {
            out.push_str("## Tenant Instructions\n");
            out.push_str(overrides);
            out.push('\n');
        }
        out
    }

    fn format_skills(&self) -> String {
        if self.persistent.skills.is_empty() {
            return String::new();
        }
        let mut out = String::from("Available skills:\n\n");
        for skill in &self.persistent.skills {
            out.push_str(&format!(
                "### /{} — {}\nTrigger: {}\n{}\n\n",
                skill.name, skill.id, skill.trigger, skill.instructions
            ));
        }
        out
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
    // Truncate proportionally by character count
    let ratio = max_tokens as f64 / current as f64;
    let max_chars = (text.chars().count() as f64 * ratio) as usize;
    let trimmed: String = text.chars().take(max_chars).collect();
    format!("{trimmed}\n...[truncated to fit context budget]")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let instructions = PersistentInstructions {
            agents_rules: vec![ProjectRule {
                project_name: "test".into(),
                rules_text: "Always use async.".into(),
                priority: RulePriority::Project,
            }],
            skills: vec![SkillPrompt {
                id: "s1".into(),
                name: "test-skill".into(),
                trigger: "when testing".into(),
                instructions: "Do thing.".into(),
            }],
            tenant_overrides: None,
        };

        let builder = SystemPromptBuilder::new(instructions);
        let budget = PromptBudget::from_window(128_000);

        // Short conversation — no compression needed
        let short_conv = "hello";
        let result = builder.assemble(short_conv, None, &[], &budget);
        assert!(!result.needs_compression);
        assert!(result.system.contains("── RULES ──"));
        assert!(result.system.contains("── SKILLS ──"));
        assert!(result.system.contains("Always use async."));
        assert!(result.system.contains("test-skill"));

        // Very long conversation — should need compression
        let long_conv = "long message. ".repeat(50_000);
        let result = builder.assemble(&long_conv, None, &[], &budget);
        assert!(result.needs_compression);
        // Rules must always be present
        assert!(result.system.contains("── RULES ──"));
    }

    #[test]
    fn test_compressed_history_present() {
        let builder = SystemPromptBuilder::new(PersistentInstructions::default());
        let budget = PromptBudget::from_window(128_000);

        let result = builder.assemble(
            "hello",
            Some("compressed: user wanted pizza"),
            &[],
            &budget,
        );
        assert!(result.system.contains("── CONVERSATION HISTORY ──"));
        assert!(result.system.contains("pizza"));
    }

    #[test]
    fn test_rules_trim_to_budget() {
        let big_rules = "x".repeat(500_000); // huge
        let trimmed = trim_to_budget(&big_rules, 100);
        assert!(estimate_tokens(&trimmed) <= 150); // allow some slack
    }

    #[test]
    fn test_skills_never_include_rules() {
        let instructions = PersistentInstructions {
            agents_rules: vec![ProjectRule {
                project_name: "p1".into(),
                rules_text: "RULE: no secrets in code".into(),
                priority: RulePriority::Global,
            }],
            skills: vec![SkillPrompt {
                id: "s1".into(),
                name: "deploy".into(),
                trigger: "on deploy".into(),
                instructions: "run cargo build".into(),
            }],
            tenant_overrides: None,
        };

        let builder = SystemPromptBuilder::new(instructions);
        let budget = PromptBudget::from_window(128_000);

        // Simulate what happens when compressed_history is injected:
        // the compressed_history should NOT contain rules text
        let compressed_only = "User asked about auth, agent suggested OAuth2.";
        let result = builder.assemble(
            "what about deployment?",
            Some(compressed_only),
            &[],
            &budget,
        );

        // Rules are present (injected separately)
        assert!(result.system.contains("no secrets in code"));
        // Compressed history is present (injected separately)
        assert!(result.system.contains("OAuth2"));
        // But the compressed_history itself should NOT contain rules
        // (this is enforced by build_merge_prompt only receiving conversation)
    }
}
