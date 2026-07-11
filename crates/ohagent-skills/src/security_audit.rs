//! Security audit for skills — content hashing, trusted sources, and dangerous pattern scanning.
//!
//! Every skill imported from an external source (Skills Hub, GitHub, etc.) is
//! content-hashed and checked against an allowlist of trusted sources before
//! being activated. Dangerous patterns (shell execution, file destruction,
//! network exfiltration) are flagged.

use sha2::{Sha256, Digest};
use tracing::warn;

/// Repositories/paths considered trusted for skill imports.
const TRUSTED_SOURCES: &[&str] = &[
    "agentskills.io",
    "github.com/nousresearch",
    "github.com/orangehat",
];

/// Dangerous patterns to scan for in skill instructions.
/// Matching instructions are flagged but not automatically blocked.
const DANGEROUS_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "> /dev/sda",
    "mkfs.",
    "dd if=",
    ":(){ :|:& };:",   // fork bomb
    "chmod 777 /",
    "wget.*| sh",
    "curl.*| bash",
    "sudo rm",
    "DROP TABLE",
    "TRUNCATE TABLE",
];

/// Audit result for a single skill.
#[derive(Debug, Clone)]
pub struct SkillAudit {
    /// SHA-256 content hash.
    pub content_hash: String,
    /// Whether the skill comes from a trusted source.
    pub trusted_source: bool,
    /// Dangerous patterns found (empty = clean).
    pub warnings: Vec<String>,
    /// Overall verdict: pass, review, or block.
    pub verdict: AuditVerdict,
}

/// Overall audit verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditVerdict {
    /// Safe to activate.
    Pass,
    /// Flagged for human review.
    Review,
    /// Blocked — cannot be activated from this source.
    Block,
}

/// Compute the SHA-256 content hash of a skill's instructions.
pub fn content_hash(instructions: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(instructions.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Check if a source is in the trusted allowlist.
pub fn is_trusted_source(source_url: &str) -> bool {
    TRUSTED_SOURCES.iter().any(|trusted| source_url.contains(trusted))
}

/// Scan skill instructions for dangerous patterns.
pub fn scan_dangerous_patterns(instructions: &str) -> Vec<String> {
    let lower = instructions.to_lowercase();
    DANGEROUS_PATTERNS
        .iter()
        .filter(|pattern| lower.contains(*pattern))
        .map(|p| format!("Dangerous pattern detected: {p}"))
        .collect()
}

/// Run a full security audit on a skill.
///
/// Returns a `SkillAudit` with verdict:
/// - `Pass` — trusted source, no dangerous patterns
/// - `Review` — dangerous patterns found, flag for review
/// - `Block` — untrusted source + dangerous patterns
pub fn audit_skill(instructions: &str, source_url: &str) -> SkillAudit {
    let hash = content_hash(instructions);
    let trusted = is_trusted_source(source_url);
    let warnings = scan_dangerous_patterns(instructions);

    let verdict = if !trusted && !warnings.is_empty() {
        warn!(
            source = %source_url,
            hash = %hash,
            warnings = ?warnings,
            "Skill blocked: untrusted source with dangerous patterns"
        );
        AuditVerdict::Block
    } else if !warnings.is_empty() {
        warn!(
            source = %source_url,
            hash = %hash,
            warnings = ?warnings,
            "Skill flagged for review"
        );
        AuditVerdict::Review
    } else {
        AuditVerdict::Pass
    };

    SkillAudit {
        content_hash: hash,
        trusted_source: trusted,
        warnings,
        verdict,
    }
}

/// Verify a skill's content hash matches expectations.
pub fn verify_content_hash(instructions: &str, expected_hash: &str) -> bool {
    content_hash(instructions) == expected_hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_hash_deterministic() {
        let h1 = content_hash("test instruction");
        let h2 = content_hash("test instruction");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_trusted_source() {
        assert!(is_trusted_source("https://github.com/nousresearch/hermes-agent"));
        assert!(is_trusted_source("https://github.com/orangehat/ohagent"));
        assert!(!is_trusted_source("https://evil.com/malicious-skill"));
    }

    #[test]
    fn test_dangerous_pattern_detection() {
        let warnings = scan_dangerous_patterns("run rm -rf / on the server");
        assert!(!warnings.is_empty());
        assert!(warnings[0].contains("rm -rf /"));
    }

    #[test]
    fn test_clean_skill_passes_audit() {
        let result = audit_skill(
            "read the file /tmp/data.txt",
            "https://github.com/orangehat/ohagent",
        );
        assert_eq!(result.verdict, AuditVerdict::Pass);
    }

    #[test]
    fn test_blocked_skill() {
        let result = audit_skill(
            "run rm -rf /",
            "https://evil.com/malicious-skill",
        );
        assert_eq!(result.verdict, AuditVerdict::Block);
    }

    #[test]
    fn test_verify_hash() {
        let instr = "safe instruction";
        let hash = content_hash(instr);
        assert!(verify_content_hash(instr, &hash));
        assert!(!verify_content_hash(instr, "fakehash"));
    }
}
