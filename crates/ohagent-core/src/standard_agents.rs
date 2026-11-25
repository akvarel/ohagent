//! Supported Standard Agent text profile and deterministic evidence checks.
//!
//! The TypeScript source package is authoritative. Its generated profile is
//! pinned at build time. Arbitrary packages, hooks and handlers are not loaded.
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextAgent {
    pub id: String,
    pub prompt: String,
    pub model: String,
    pub max_steps: u32,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextProfile {
    pub specification_version: String,
    pub package_id: String,
    pub package_version: String,
    pub agents: Vec<TextAgent>,
}
impl TextProfile {
    pub fn analytical_brief() -> Result<Self, &'static str> {
        let profile: Self = serde_json::from_str(include_str!(
            "../../../agents/standardagent-analytical-brief/text-profile.json"
        ))
        .map_err(|_| "invalid_agent_profile")?;
        if profile.specification_version != "0.2.0"
            || profile.agents.len() != 4
            || profile
                .agents
                .iter()
                .any(|a| a.max_steps != 1 || a.prompt.is_empty())
            || profile
                .agents
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>()
                != ["coordinator", "analyst", "evidence_checker", "reviewer"]
        {
            return Err("unsupported_agent_profile");
        }
        Ok(profile)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brief {
    pub title: String,
    pub summary: String,
    pub findings: String,
    pub limitations: String,
    pub citations: Vec<Citation>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    pub quote: String,
}

/// This verifies structure and exact source quotations, not semantic truth.
/// A model's review is never treated as a proof that every claim is factual.
pub fn verify_brief(output: &str, materials: &str) -> Result<Brief, &'static str> {
    if output.len() > 64_000 {
        return Err("artifact_too_large");
    }
    let output = output.trim();
    // Some providers wrap a JSON result despite a no-fences instruction. Only
    // the exact whole-document wrapper is accepted, never substring extraction.
    let output = output
        .strip_prefix("```json\n")
        .and_then(|s| s.strip_suffix("\n```"))
        .or_else(|| {
            output
                .strip_prefix("```\n")
                .and_then(|s| s.strip_suffix("\n```"))
        })
        .unwrap_or(output);
    let brief: Brief = serde_json::from_str(output).map_err(|_| "invalid_brief_json")?;
    if [
        &brief.title,
        &brief.summary,
        &brief.findings,
        &brief.limitations,
    ]
    .iter()
    .any(|s| s.trim().is_empty() || s.len() > 24_000)
        || brief.citations.is_empty()
        || brief.citations.len() > 20
    {
        return Err("invalid_brief_structure");
    }
    if brief.citations.iter().any(|c| {
        c.quote.trim().is_empty() || c.quote.chars().count() > 500 || !materials.contains(&c.quote)
    }) {
        return Err("unverified_source_quote");
    }
    Ok(brief)
}
impl Brief {
    /// Render only validated output as plain Markdown, never executable HTML.
    pub fn markdown(&self) -> String {
        let quotes = self
            .citations
            .iter()
            .map(|c| format!("> {}", markdown_text(&c.quote).replace('\n', "\n> ")))
            .collect::<Vec<_>>()
            .join("\n\n");
        format!("# {}\n\n{}\n\n## Findings\n\n{}\n\n## Limitations\n\n{}\n\n## Source quotations\n\n{}\n\n---\nVerification: required fields and exact source quotations checked deterministically. Semantic accuracy is not certified.\n", markdown_text(&self.title), markdown_text(&self.summary), markdown_text(&self.findings), markdown_text(&self.limitations), quotes)
    }
}

fn markdown_text(value: &str) -> String {
    let mut safe = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => safe.push_str("&amp;"),
            '<' => safe.push_str("&lt;"),
            '>' => safe.push_str("&gt;"),
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '(' | ')' | '#' | '+' | '-' | '.'
            | '!' | '|' | '~' => {
                safe.push('\\');
                safe.push(c);
            }
            _ => safe.push(c),
        }
    }
    safe
}

#[cfg(test)]
mod tests {
    use super::*;
    fn output(quote: &str) -> String {
        serde_json::json!({"title":"Pilot","summary":"Scope","findings":"Observation","limitations":"Only supplied data","citations":[{"quote":quote}]}).to_string()
    }
    #[test]
    fn package_profile_preserves_distinct_domain_agents() {
        let p = TextProfile::analytical_brief().unwrap();
        assert_eq!(p.package_id, "@orangehat/standardagent-analytical-brief");
        assert!(p
            .agents
            .iter()
            .all(|a| a.model == "deepseek:deepseek-v4-flash"));
    }
    #[test]
    fn exact_quotes_are_verified_without_model_voting() {
        let b = verify_brief(&output("alpha revenue 12"), "Record: alpha revenue 12 EUR.").unwrap();
        assert!(b.markdown().contains("Semantic accuracy is not certified"));
    }
    #[test]
    fn artifact_treats_html_and_remote_images_as_text() {
        let source = "<script>alert(1)</script> ![private](https://example.invalid/pixel)";
        let mut brief = verify_brief(&output(source), source).unwrap();
        brief.title = source.into();
        brief.summary = source.into();
        brief.findings = source.into();
        brief.limitations = source.into();
        let markdown = brief.markdown();
        assert!(!markdown.contains("<script>"));
        assert!(!markdown.contains("![private]("));
        assert!(markdown.contains("&lt;script&gt;"));
        assert!(markdown.contains("\\!\\[private\\]"));
    }
    #[test]
    fn fabricated_quote_fails() {
        assert_eq!(
            verify_brief(&output("revenue 99"), "revenue 12").unwrap_err(),
            "unverified_source_quote"
        );
    }
    #[test]
    fn empty_citations_and_oversized_results_fail() {
        let no_cites =
            r#"{"title":"x","summary":"x","findings":"x","limitations":"x","citations":[]}"#;
        assert!(verify_brief(no_cites, "x").is_err());
        assert!(verify_brief(&"x".repeat(64_001), "x").is_err());
    }
    #[test]
    fn unexpected_prompt_or_reasoning_fields_are_rejected() {
        let mut value: serde_json::Value = serde_json::from_str(&output("x")).unwrap();
        value["reasoning"] = serde_json::json!("private");
        assert!(verify_brief(&value.to_string(), "x").is_err());
    }
}
