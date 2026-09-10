//! Recurring improvement backlog — durable convention for repeated problems.
//!
//! Agents observe the same problem many times; without a durable, counted
//! record, the recurrence dissolves into logs. This module implements a
//! simple file-backed backlog where each new observation of a known problem
//! **strengthens one durable entry** instead of creating noise.
//!
//! ## Canonical storage
//!
//! Two files in the canonical memory location (default
//! `TEAM_MEMORY/`, i.e. the repo's existing team-memory area):
//!
//! - `IMPROVEMENT_BACKLOG.json` — canonical, machine-readable store.
//! - `IMPROVEMENT_BACKLOG.md`   — rendered human-readable view (regenerated).
//!
//! Writes are atomic (temp file + rename) so a crash never corrupts the
//! backlog.
//!
//! ## Identity / deduplication
//!
//! Identity is deterministic first: normalized `component` + `category` +
//! `signature`. An exact match increments the existing entry (recurrence
//! counted). A same-component same-category but different-signature match is
//! recorded as a *possible duplicate* link — never silently merged. An LLM
//! matcher may later promote such links, but never merges high-impact items
//! automatically.
//!
//! ## Ranking
//!
//! Sort is deterministic: `priority` first (a critical count=1 problem is
//! never buried by a low-impact count=100 one), then `count` desc, then
//! `last_seen` desc, then `id` asc.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Canonical backlog file names (relative to the memory dir).
pub const BACKLOG_JSON: &str = "IMPROVEMENT_BACKLOG.json";
pub const BACKLOG_MD: &str = "IMPROVEMENT_BACKLOG.md";

/// One durable improvement backlog entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BacklogEntry {
    pub id: String,
    /// Normalized problem identity: `component|category|signature`.
    pub key: String,
    pub summary: String,
    pub first_seen: String,
    pub last_seen: String,
    pub count: u64,
    /// "critical" | "high" | "medium" | "low"
    pub priority: String,
    /// "open" | "resolved" | "wont_fix"
    pub status: String,
    pub evidence: Vec<String>,
    /// Components/subsystems this entry may overlap with (ambiguity kept
    /// explicit instead of merging two possibly different problems).
    pub possible_duplicates: Vec<String>,
    pub component: String,
    pub category: String,
    #[serde(default)]
    pub impact: String,
    #[serde(default)]
    pub next_action: String,
    #[serde(default)]
    pub last_source: String,
}

/// A new observation to record in the backlog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub component: String,
    /// Failure/category bucket, e.g. "test-flake", "selector-stale", "api-500".
    pub category: String,
    /// Stable error signature (stack-ish, test name, error code...).
    pub signature: String,
    pub summary: String,
    pub priority: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub impact: String,
    #[serde(default)]
    pub next_action: String,
    #[serde(default)]
    pub source: String,
}

impl Observation {
    /// Deterministic normalized identity. Cheap and LLM-free.
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}",
            normalize(&self.component),
            normalize(&self.category),
            normalize(&self.signature)
        )
    }
}

/// File-backed recurring improvement backlog.
pub struct ImprovementBacklog {
    json_path: PathBuf,
    md_path: PathBuf,
    entries: Vec<BacklogEntry>,
}

impl ImprovementBacklog {
    /// Open (or create) the backlog in `dir`.
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("create backlog dir {}", dir.display()))?;
        let json_path = dir.join(BACKLOG_JSON);
        let md_path = dir.join(BACKLOG_MD);
        let entries = if json_path.exists() {
            let raw = std::fs::read_to_string(&json_path)?;
            serde_json::from_str(&raw).context("parse improvement backlog")?
        } else {
            Vec::new()
        };
        Ok(Self {
            json_path,
            md_path,
            entries,
        })
    }

    /// Record an observation: match an existing entry (increment count,
    /// update last_seen, append evidence) or create a new one. Ambiguous
    /// near-matches are linked as possible duplicates, never merged.
    pub fn report(&mut self, obs: &Observation) -> Result<BacklogEntry> {
        let now = chrono::Utc::now().to_rfc3339();
        let key = obs.key();

        let matched = self
            .entries
            .iter_mut()
            .find(|e| e.key == key && e.status == "open");

        if let Some(e) = matched {
            e.count += 1;
            e.last_seen = now.clone();
            if let Some(ev) = obs.evidence.first() {
                let ev = redact(ev);
                if !e.evidence.contains(&ev) && e.evidence.len() < MAX_EVIDENCE {
                    e.evidence.push(ev);
                }
            }
            escalate_priority(e, &obs.priority);
            if !obs.next_action.is_empty() {
                e.next_action = redact(&obs.next_action);
            }
            e.last_source = redact(&obs.source);
            let updated = e.clone();
            self.persist()?;
            return Ok(updated);
        }

        // Ambiguity: same component+category but different signature — link,
        // never merge.
        let mut possible = Vec::new();
        for e in &self.entries {
            if normalize(&e.component) == normalize(&obs.component)
                && normalize(&e.category) == normalize(&obs.category)
                && e.status == "open"
            {
                possible.push(e.id.clone());
                // Record the link on the existing side too (kept durable).
            }
        }
        let new_id = next_id(&self.entries);
        for other in &mut self.entries {
            if possible.contains(&other.id) {
                other.possible_duplicates.push(new_id.clone());
            }
        }

        let entry = BacklogEntry {
            id: next_id(&self.entries),
            key: key.clone(),
            summary: redact(&obs.summary),
            first_seen: now.clone(),
            last_seen: now.clone(),
            count: 1,
            priority: normalize_priority(&obs.priority),
            status: "open".into(),
            evidence: obs
                .evidence
                .iter()
                .take(MAX_EVIDENCE)
                .map(|e| redact(e))
                .collect(),
            possible_duplicates: possible,
            component: obs.component.clone(),
            category: obs.category.clone(),
            impact: redact(&obs.impact),
            next_action: redact(&obs.next_action),
            last_source: redact(&obs.source),
        };
        self.entries.push(entry.clone());
        self.persist()?;
        Ok(entry)
    }

    /// Deterministic ranking: priority dominant, then recurrence count,
    /// then recency, then id (tie-break).
    pub fn ranked(&self, open_only: bool) -> Vec<BacklogEntry> {
        let mut list: Vec<BacklogEntry> = self
            .entries
            .iter()
            .filter(|e| !open_only || e.status == "open")
            .cloned()
            .collect();
        list.sort_by(|a, b| {
            priority_rank(&a.priority)
                .cmp(&priority_rank(&b.priority))
                .then(b.count.cmp(&a.count))
                .then(b.last_seen.cmp(&a.last_seen))
                .then(a.id.cmp(&b.id))
        });
        list
    }

    pub fn entries(&self) -> &[BacklogEntry] {
        &self.entries
    }

    pub fn resolve(&mut self, id: &str) -> Result<bool> {
        for e in &mut self.entries {
            if e.id == id {
                e.status = "resolved".into();
                self.persist()?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Atomic persist: canonical JSON + rendered markdown view.
    fn persist(&mut self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.entries)?;
        let tmp = self.json_path.with_extension("json.tmp");
        std::fs::write(&tmp, json.as_bytes())
            .with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.json_path)
            .with_context(|| format!("rename into {}", self.json_path.display()))?;
        let md = render_markdown(self.entries());
        let tmp = self.md_path.with_extension("md.tmp");
        std::fs::write(&tmp, md)?;
        std::fs::rename(&tmp, &self.md_path)?;
        Ok(())
    }
}

const MAX_EVIDENCE: usize = 10;

fn next_id(entries: &[BacklogEntry]) -> String {
    let max = entries
        .iter()
        .filter_map(|e| {
            e.id.strip_prefix("IMP-")
                .and_then(|s| s.parse::<u64>().ok())
        })
        .max()
        .unwrap_or(0);
    format!("IMP-{:04}", max + 1)
}

fn normalize(s: &str) -> String {
    let mut out = String::new();
    let mut last_ws = false;
    for ch in s.trim().to_lowercase().chars() {
        if ch.is_whitespace() || ch == '-' || ch == '_' {
            if !last_ws {
                out.push('-');
                last_ws = true;
            }
        } else {
            out.push(ch);
            last_ws = false;
        }
    }
    out.trim_matches('-').to_string()
}

fn normalize_priority(p: &str) -> String {
    match p.to_ascii_lowercase().as_str() {
        "p0" | "critical" | "blocker" => "critical".into(),
        "p1" | "high" | "major" => "high".into(),
        "p2" | "medium" | "moderate" => "medium".into(),
        "p3" | "low" | "minor" => "low".into(),
        other => other.into(),
    }
}

fn priority_rank(p: &str) -> u8 {
    match p {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

/// A critical single-occurrence problem must outrank a noisy low-impact one:
/// priority dominates the sort; recurrence only orders within a class.
fn escalate_priority(entry: &mut BacklogEntry, observed: &str) {
    let observed = normalize_priority(observed);
    if priority_rank(&observed) < priority_rank(&entry.priority) {
        entry.priority = observed;
    }
}

/// Redact obvious secrets from free text before it becomes durable evidence.
///
/// Heuristics (no network, no LLM): AWS access keys, bearer tokens,
/// `key=value` / `key: value` secrets, and long base64/hex blobs.
pub fn redact(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for word in input.split_whitespace() {
        out.push_str(&redact_word(word));
        out.push(' ');
    }
    out.trim_end().to_string()
}

fn redact_word(w: &str) -> String {
    // AWS access key id
    if w.len() == 20 && w.starts_with("AKIA") && w.chars().all(|c| c.is_ascii_alphanumeric()) {
        return "[REDACTED-AWS-KEY]".into();
    }
    // key=value or key: value secret assignment
    let lower = w.to_ascii_lowercase();
    for prefix in [
        "password=",
        "password:",
        "token=",
        "token:",
        "secret=",
        "secret:",
        "api_key=",
        "api_key:",
        "apikey=",
        "apikey:",
        "authorization:",
        "bearer",
    ] {
        if lower_starts(&lower, prefix) {
            let klen = prefix.len();
            if prefix == "bearer" && w.len() > 6 {
                return "bearer [REDACTED]".into();
            }
            if w.len() > klen {
                return format!("{}[REDACTED]", &w[..klen]);
            }
        }
    }
    // Long base64/hex-looking blob: likely a credential
    if w.len() >= 32
        && w.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '_')
    {
        return "[REDACTED]".into();
    }
    w.to_string()
}

fn lower_starts(lower: &str, prefix: &str) -> bool {
    lower
        .get(..prefix.len())
        .map(|s| s == prefix)
        .unwrap_or(false)
}

/// Render the human-readable markdown view.
fn render_markdown(entries: &[BacklogEntry]) -> String {
    let mut sorted: Vec<&BacklogEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        priority_rank(&a.priority)
            .cmp(&priority_rank(&b.priority))
            .then(b.count.cmp(&a.count))
            .then(b.last_seen.cmp(&a.last_seen))
            .then(a.id.cmp(&b.id))
    });
    let mut out = String::from(
        "# Improvement Backlog\n\n> Generated from `IMPROVEMENT_BACKLOG.json` — do not edit by hand;\n> report occurrences via `ImprovementBacklog::report`.\n\n",
    );
    if sorted.is_empty() {
        out.push_str("_(empty)_\n");
        return out;
    }
    out.push_str(
        "| id | priority | count | component | category | status | summary | last_seen |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|\n");
    for e in sorted {
        let summary = e.summary.replace('|', "\\|");
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            e.id, e.priority, e.count, e.component, e.category, e.status, summary, e.last_seen
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("ohagent-backlog-{}-{}", tag, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn obs(
        component: &str,
        category: &str,
        sig: &str,
        summary: &str,
        priority: &str,
    ) -> Observation {
        Observation {
            component: component.into(),
            category: category.into(),
            signature: sig.into(),
            summary: summary.into(),
            priority: priority.into(),
            evidence: vec![],
            impact: String::new(),
            next_action: String::new(),
            source: String::new(),
        }
    }

    #[test]
    fn repeated_observation_increments_existing_entry() {
        let d = dir("repeat");
        let mut b = ImprovementBacklog::open(&d).unwrap();
        for i in 0..5 {
            let o = Observation {
                component: "tui".into(),
                category: "test-flake".into(),
                signature: "test handterm_spawn panics on rerender".into(),
                summary: format!("stale selector fallback, occurrence {i}"),
                priority: "high".into(),
                evidence: vec![format!("run #{i} log line 42")],
                impact: String::new(),
                next_action: String::new(),
                source: format!("session-{i}"),
            };
            b.report(&o).unwrap();
        }
        let ranked = b.ranked(true);
        assert_eq!(ranked.len(), 1, "5 observations = ONE entry, not five");
        assert_eq!(ranked[0].count, 5);
        assert_eq!(ranked[0].last_source, "session-4");
        assert_eq!(ranked[0].evidence.len(), 5);
    }

    #[test]
    fn different_problem_is_not_merged() {
        let d = dir("distinct");
        let mut b = ImprovementBacklog::open(&d).unwrap();
        b.report(&obs("tui", "test-flake", "selector stale", "A", "high"))
            .unwrap();
        b.report(&obs("tui", "test-flake", "outbox deadlock", "B", "high"))
            .unwrap();
        let ranked = b.ranked(true);
        assert_eq!(ranked.len(), 2, "different signatures must stay separate");
        assert_eq!(ranked[0].count, 1);
        assert_eq!(ranked[1].count, 1);
    }

    #[test]
    fn ambiguous_duplicate_becomes_candidate_link_not_merge() {
        let d = dir("ambiguous");
        let mut b = ImprovementBacklog::open(&d).unwrap();
        b.report(&obs(
            "gateway",
            "timeout",
            "upstream slow 10s",
            "upstream slow",
            "medium",
        ))
        .unwrap();
        b.report(&obs(
            "gateway",
            "timeout",
            "upstream slow after reload?",
            "similar but unclear",
            "medium",
        ))
        .unwrap();
        let entries = &b.entries;
        assert_eq!(entries.len(), 2, "ambiguous match must NOT auto-merge");
        assert!(
            !entries[1].possible_duplicates.is_empty(),
            "ambiguity recorded as candidate link"
        );
    }

    #[test]
    fn ranking_is_deterministic_and_priority_dominant() {
        let d = dir("ranking");
        let mut b = ImprovementBacklog::open(&d).unwrap();
        // Noisy low-priority problem with 100 recurrences.
        for _ in 0..100 {
            b.report(&obs("cli", "cosmetic", "help text typo", "typo", "low"))
                .unwrap();
        }
        // Critical problem seen once.
        b.report(&obs(
            "daemon",
            "crash",
            "panic on shutdown",
            "panic",
            "critical",
        ))
        .unwrap();
        let ranked = b.ranked(true);
        assert_eq!(
            ranked[0].id,
            ranked.iter().find(|e| e.priority == "critical").unwrap().id,
            "critical count=1 must outrank low count=100"
        );
        // Determinism: same input -> same order.
        let again = b.ranked(true);
        assert_eq!(
            ranked.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
            again.iter().map(|e| e.id.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn recurrence_with_higher_priority_escalates() {
        let d = dir("escalate");
        let mut b = ImprovementBacklog::open(&d).unwrap();
        b.report(&obs(
            "swarm",
            "stall",
            "agent-stuck-on-shutdown",
            "stuck",
            "low",
        ))
        .unwrap();
        // Same signature (normalization is case/separator-insensitive).
        b.report(&obs(
            "Swarm",
            "STALL",
            "Agent_Stuck_on_Shutdown",
            "stuck again",
            "critical",
        ))
        .unwrap();
        let ranked = b.ranked(true);
        assert_eq!(ranked.len(), 1, "normalized identity must match");
        assert_eq!(ranked[0].count, 2);
        assert_eq!(
            ranked[0].priority, "critical",
            "priority escalates, never de-escalates"
        );
    }

    #[test]
    fn secrets_are_redacted_in_backlog_output() {
        let d = dir("redact");
        let mut b = ImprovementBacklog::open(&d).unwrap();
        let o = Observation {
            component: "cli".into(),
            category: "auth".into(),
            signature: "token rejected".into(),
            summary: "failed with api_key=sk-abcdef1234567890abcdef123456".into(),
            priority: "high".into(),
            evidence: vec![
                "AKIAIOSFODNN7EXAMPLE in logs".into(),
                "authorization: Bearer abcdef1234567890abcdef1234567890".into(),
                "normal text stays".into(),
            ],
            impact: String::new(),
            next_action: String::new(),
            source: String::new(),
        };
        b.report(&o).unwrap();
        let raw = std::fs::read_to_string(d.join(BACKLOG_JSON)).unwrap();
        assert!(!raw.contains("sk-abcdef"), "api key must be redacted");
        assert!(!raw.contains("AKIAIOSFODNN7EXAMPLE"), "aws key redacted");
        assert!(
            !raw.contains("abcdef1234567890abcdef1234567890"),
            "bearer redacted"
        );
        assert!(
            raw.contains("normal text stays"),
            "benign evidence preserved"
        );
    }

    #[test]
    fn markdown_view_is_regenerated_and_lists_counts() {
        let d = dir("view");
        let mut b = ImprovementBacklog::open(&d).unwrap();
        for _ in 0..3 {
            b.report(&obs(
                "mcp",
                "reconnect",
                "pooled server died",
                "server died",
                "high",
            ))
            .unwrap();
        }
        let md = std::fs::read_to_string(d.join(BACKLOG_MD)).unwrap();
        assert!(md.contains("IMP-0001"));
        assert!(md.contains("3"), "count must appear in the rendered view");
        assert!(md.contains("open"));
    }

    #[test]
    fn backlog_survives_reopen() {
        let d = dir("reopen");
        {
            let mut b = ImprovementBacklog::open(&d).unwrap();
            b.report(&obs("a", "b", "c", "durable entry", "high"))
                .unwrap();
        }
        let b = ImprovementBacklog::open(&d).unwrap();
        assert_eq!(b.entries.len(), 1);
        assert_eq!(b.entries[0].summary, "durable entry");
    }
}
