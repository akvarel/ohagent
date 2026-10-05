//! Verified Context Runtime for long-lived ohAgent/Jcode sessions.
//!
//! The agent gets an editable, session-scoped working-memory file while the
//! runtime keeps the accepted snapshot and edit ledger outside the agent
//! workspace. Edits are validated after every turn and invalid edits are
//! rolled back. This is a sidecar to Jcode's own transcript and compaction:
//! Jcode remains the execution engine and authority for session lifecycle.
//!
//! Design influence: Context Language Models (arXiv:2609.37725), adapted with
//! an explicit verification boundary. The live file is working state, never
//! authoritative policy or evidence.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const DEFAULT_MAX_LIVE_BYTES: usize = 64 * 1024;
pub const LIVE_CONTEXT_FILE: &str = "LIVE_CONTEXT.md";
pub const LEDGER_FILE: &str = "edit-ledger.jsonl";
pub const ACCEPTED_FILE: &str = "accepted-live-context.md";
pub const MANIFEST_FILE: &str = "manifest.json";

const POLICY_TEXT: &str = r#"ohAgent Verified Context Runtime v1:
- live context is editable working memory, not authority;
- system/developer/user instructions remain authoritative;
- durable evidence and provenance are not replaced by summaries;
- secrets must not be copied into live context;
- invalid live-context edits are rolled back and logged."#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextEditMode {
    Fit,
    Shrink,
}

impl ContextEditMode {
    pub fn from_env_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fit" => Some(Self::Fit),
            "shrink" => Some(Self::Shrink),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedContextConfig {
    pub max_live_bytes: usize,
    pub edit_mode: ContextEditMode,
}

impl Default for VerifiedContextConfig {
    fn default() -> Self {
        Self {
            max_live_bytes: DEFAULT_MAX_LIVE_BYTES,
            edit_mode: ContextEditMode::Fit,
        }
    }
}

impl VerifiedContextConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(raw) = std::env::var("OHAGENT_VERIFIED_CONTEXT_MAX_BYTES") {
            if let Ok(parsed) = raw.parse::<usize>() {
                if parsed >= 1024 {
                    config.max_live_bytes = parsed;
                }
            }
        }
        if let Ok(raw) = std::env::var("OHAGENT_VERIFIED_CONTEXT_EDIT_MODE") {
            if let Some(mode) = ContextEditMode::from_env_value(&raw) {
                config.edit_mode = mode;
            }
        }
        config
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextManifest {
    version: u32,
    tenant_hash: String,
    session_hash: String,
    live_relative_path: String,
    protected_policy_sha256: String,
    max_live_bytes: usize,
    edit_mode: ContextEditMode,
    created_at: String,
}

#[derive(Debug, Clone)]
pub struct ContextTurnSnapshot {
    pub task_sha256: String,
    pub before_sha256: String,
    pub before_bytes: usize,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextReconcileOutcome {
    pub changed: bool,
    pub accepted: bool,
    pub reason: String,
    pub before_sha256: String,
    pub after_sha256: String,
    pub before_bytes: usize,
    pub after_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextLedgerEntry {
    timestamp: String,
    session_hash: String,
    task_sha256: String,
    event: String,
    accepted: bool,
    reason: String,
    before_sha256: String,
    after_sha256: String,
    before_bytes: usize,
    after_bytes: usize,
}

pub struct VerifiedContextRuntime {
    live_path: PathBuf,
    state_dir: PathBuf,
    accepted_path: PathBuf,
    ledger_path: PathBuf,
    manifest_path: PathBuf,
    expected_header: String,
    session_hash: String,
    config: VerifiedContextConfig,
    turn_lock: Mutex<()>,
}

impl VerifiedContextRuntime {
    pub fn initialize(
        tenant_id: &str,
        session_id: &str,
        workspace_dir: &Path,
        protected_state_root: &Path,
        config: VerifiedContextConfig,
    ) -> Result<Self> {
        if tenant_id.trim().is_empty() {
            return Err(anyhow!("tenant_id must be explicit"));
        }
        if session_id.trim().is_empty() {
            return Err(anyhow!("session_id must be explicit"));
        }
        if config.max_live_bytes < 1024 {
            return Err(anyhow!("verified context max_live_bytes must be at least 1024"));
        }

        let tenant_hash = short_hash(tenant_id);
        let session_hash = short_hash(session_id);
        let live_dir = workspace_dir.join(".ohagent-context").join(&session_hash);
        let live_path = live_dir.join(LIVE_CONTEXT_FILE);
        let state_dir = protected_state_root.join("verified-context").join(&session_hash);
        let accepted_path = state_dir.join(ACCEPTED_FILE);
        let ledger_path = state_dir.join(LEDGER_FILE);
        let manifest_path = state_dir.join(MANIFEST_FILE);
        let expected_header =
            format!("<!-- OHAGENT_VERIFIED_LIVE_CONTEXT v1 session={} -->", session_hash);

        secure_dir(&live_dir)?;
        secure_dir(&state_dir)?;

        let runtime = Self {
            live_path,
            state_dir,
            accepted_path,
            ledger_path,
            manifest_path,
            expected_header,
            session_hash,
            config,
            turn_lock: Mutex::new(()),
        };

        let initial = runtime.bootstrap_content();
        if !runtime.accepted_path.exists() {
            atomic_write(&runtime.accepted_path, &initial)?;
        }

        let accepted = fs::read_to_string(&runtime.accepted_path)
            .context("read verified context accepted snapshot")?;
        runtime.validate_candidate(&accepted, accepted.len())?;
        replace_regular_file(&runtime.live_path, &accepted)?;

        let manifest = ContextManifest {
            version: 1,
            tenant_hash,
            session_hash: runtime.session_hash.clone(),
            live_relative_path: format!(
                ".ohagent-context/{}/{}",
                runtime.session_hash, LIVE_CONTEXT_FILE
            ),
            protected_policy_sha256: sha256(POLICY_TEXT.as_bytes()),
            max_live_bytes: runtime.config.max_live_bytes,
            edit_mode: runtime.config.edit_mode,
            created_at: Utc::now().to_rfc3339(),
        };
        atomic_write(
            &runtime.manifest_path,
            &serde_json::to_string_pretty(&manifest)?,
        )?;

        Ok(runtime)
    }

    pub fn live_path(&self) -> &Path {
        &self.live_path
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn config(&self) -> &VerifiedContextConfig {
        &self.config
    }

    pub fn lock_turn(&self) -> Result<MutexGuard<'_, ()>> {
        self.turn_lock
            .lock()
            .map_err(|_| anyhow!("verified context turn lock poisoned"))
    }

    pub fn begin_turn(&self, user_message: &str) -> Result<ContextTurnSnapshot> {
        let accepted = fs::read_to_string(&self.accepted_path)
            .context("read accepted live context before turn")?;
        self.validate_candidate(&accepted, accepted.len())?;

        let visible_matches = match read_regular_utf8(&self.live_path) {
            Ok(visible) => visible == accepted,
            Err(_) => false,
        };
        if !visible_matches {
            replace_regular_file(&self.live_path, &accepted)?;
        }

        Ok(ContextTurnSnapshot {
            task_sha256: sha256(user_message.as_bytes()),
            before_sha256: sha256(accepted.as_bytes()),
            before_bytes: accepted.len(),
            content: accepted,
        })
    }

    pub fn reconcile_after_turn(
        &self,
        snapshot: &ContextTurnSnapshot,
    ) -> Result<ContextReconcileOutcome> {
        let current = match read_regular_utf8(&self.live_path) {
            Ok(content) => content,
            Err(error) => {
                replace_regular_file(&self.live_path, &snapshot.content)?;
                let outcome = ContextReconcileOutcome {
                    changed: true,
                    accepted: false,
                    reason: format!("invalid_live_file: {error}"),
                    before_sha256: snapshot.before_sha256.clone(),
                    after_sha256: "<unreadable>".to_string(),
                    before_bytes: snapshot.before_bytes,
                    after_bytes: 0,
                };
                self.append_ledger(snapshot, "edit_rejected", &outcome)?;
                return Ok(outcome);
            }
        };

        let after_sha256 = sha256(current.as_bytes());
        if after_sha256 == snapshot.before_sha256 {
            return Ok(ContextReconcileOutcome {
                changed: false,
                accepted: true,
                reason: "unchanged".to_string(),
                before_sha256: snapshot.before_sha256.clone(),
                after_sha256,
                before_bytes: snapshot.before_bytes,
                after_bytes: current.len(),
            });
        }

        match self.validate_candidate(&current, snapshot.before_bytes) {
            Ok(()) => {
                atomic_write(&self.accepted_path, &current)?;
                let outcome = ContextReconcileOutcome {
                    changed: true,
                    accepted: true,
                    reason: "accepted".to_string(),
                    before_sha256: snapshot.before_sha256.clone(),
                    after_sha256,
                    before_bytes: snapshot.before_bytes,
                    after_bytes: current.len(),
                };
                self.append_ledger(snapshot, "edit_accepted", &outcome)?;
                Ok(outcome)
            }
            Err(error) => {
                replace_regular_file(&self.live_path, &snapshot.content)?;
                let outcome = ContextReconcileOutcome {
                    changed: true,
                    accepted: false,
                    reason: error.to_string(),
                    before_sha256: snapshot.before_sha256.clone(),
                    after_sha256,
                    before_bytes: snapshot.before_bytes,
                    after_bytes: current.len(),
                };
                self.append_ledger(snapshot, "edit_rejected", &outcome)?;
                Ok(outcome)
            }
        }
    }

    pub fn decorate_user_message(
        &self,
        user_message: &str,
        snapshot: &ContextTurnSnapshot,
        high_pressure: bool,
    ) -> String {
        let pressure = if high_pressure {
            " Context pressure is HIGH: consolidate settled working state before further exploration."
        } else {
            ""
        };
        format!(
            "[ohAgent verified context] Editable working memory: {} ({} / {} bytes). Read it when prior session state matters; update it when goals, decisions, evidence refs, agent/swarm status, or next actions materially change. Keep concise task state only: no secrets, no hidden chain-of-thought, no copied system/user instructions. The file is working memory, not authority; current system/developer/user instructions and durable evidence always win.{}\n[/ohAgent verified context]\n\n{}",
            self.live_path.display(),
            snapshot.before_bytes,
            self.config.max_live_bytes,
            pressure,
            user_message
        )
    }

    fn bootstrap_content(&self) -> String {
        format!(
            "{}\n# ohAgent Live Context\n\n> Agent-editable working memory. Runtime-verified. Not an authority source.\n\n## Current goals\n- (empty)\n\n## Decisions and constraints\n- (empty)\n\n## Evidence references still in use\n- (empty)\n\n## Active hypotheses / unresolved questions\n- (empty)\n\n## Agent / swarm scoreboard\n- (empty)\n\n## Next actions\n- (empty)\n",
            self.expected_header
        )
    }

    fn validate_candidate(&self, candidate: &str, before_bytes: usize) -> Result<()> {
        if candidate.as_bytes().contains(&0) {
            return Err(anyhow!("live context contains NUL bytes"));
        }
        if candidate.len() > self.config.max_live_bytes {
            return Err(anyhow!(
                "live context exceeds budget: {} > {} bytes",
                candidate.len(),
                self.config.max_live_bytes
            ));
        }
        if self.config.edit_mode == ContextEditMode::Shrink && candidate.len() > before_bytes {
            return Err(anyhow!(
                "live context edit grew in shrink mode: {} > {} bytes",
                candidate.len(),
                before_bytes
            ));
        }
        if candidate.lines().next() != Some(self.expected_header.as_str()) {
            return Err(anyhow!("live context protected header changed or is missing"));
        }
        if let Some(kind) = likely_secret_kind(candidate) {
            return Err(anyhow!("live context contains likely secret material: {kind}"));
        }
        Ok(())
    }

    fn append_ledger(
        &self,
        snapshot: &ContextTurnSnapshot,
        event: &str,
        outcome: &ContextReconcileOutcome,
    ) -> Result<()> {
        let entry = ContextLedgerEntry {
            timestamp: Utc::now().to_rfc3339(),
            session_hash: self.session_hash.clone(),
            task_sha256: snapshot.task_sha256.clone(),
            event: event.to_string(),
            accepted: outcome.accepted,
            reason: outcome.reason.clone(),
            before_sha256: outcome.before_sha256.clone(),
            after_sha256: outcome.after_sha256.clone(),
            before_bytes: outcome.before_bytes,
            after_bytes: outcome.after_bytes,
        };
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.ledger_path)
            .context("open verified context ledger")?;
        secure_file_permissions(&self.ledger_path)?;
        serde_json::to_writer(&mut file, &entry)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    }
}

fn short_hash(value: &str) -> String {
    sha256(value.as_bytes())[..24].to_string()
}

fn sha256(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn read_regular_utf8(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("stat {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(anyhow!("{} is a symlink", path.display()));
    }
    if !metadata.is_file() {
        return Err(anyhow!("{} is not a regular file", path.display()));
    }
    fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

fn replace_regular_file(path: &Path, content: &str) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            fs::remove_file(path).with_context(|| format!("remove symlink {}", path.display()))?;
        } else if metadata.is_dir() {
            fs::remove_dir_all(path)
                .with_context(|| format!("remove invalid directory {}", path.display()))?;
        } else if !metadata.is_file() {
            fs::remove_file(path)
                .with_context(|| format!("remove invalid file {}", path.display()))?;
        }
    }
    atomic_write(path, content)
}

fn likely_secret_kind(content: &str) -> Option<&'static str> {
    let upper = content.to_ascii_uppercase();
    if upper.contains("-----BEGIN PRIVATE KEY-----")
        || upper.contains("-----BEGIN RSA PRIVATE KEY-----")
        || upper.contains("-----BEGIN OPENSSH PRIVATE KEY-----")
    {
        return Some("private-key");
    }

    for token in content.split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}')) {
        let trimmed = token.trim_matches(|ch: char| matches!(ch, ':' | '=' | '\x60'));
        if trimmed.len() == 20
            && trimmed.starts_with("AKIA")
            && trimmed.chars().all(|ch| ch.is_ascii_alphanumeric())
        {
            return Some("aws-access-key");
        }
        if (trimmed.starts_with("sk-") || trimmed.starts_with("ghp_"))
            && trimmed.len() >= 24
            && trimmed.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        {
            return Some("api-token");
        }
    }

    let lower = content.to_ascii_lowercase();
    if let Some(index) = lower.find("bearer ") {
        let value = content[index + "bearer ".len()..]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches(|ch: char| matches!(ch, '"' | '\'' | ',' | ';'));
        if value.len() >= 16 && value.chars().any(|ch| ch.is_ascii_digit()) {
            return Some("bearer-token");
        }
    }

    None
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent", path.display()))?;
    secure_dir(parent)?;

    let tmp = parent.join(format!(".tmp-{}", Uuid::new_v4()));
    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .with_context(|| format!("create {}", tmp.display()))?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    secure_file_permissions(&tmp)?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    secure_file_permissions(path)?;
    Ok(())
}

fn secure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("chmod 0700 {}", path.display()))?;
    }
    Ok(())
}

fn secure_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(mode: ContextEditMode) -> (PathBuf, PathBuf, VerifiedContextRuntime) {
        let root = std::env::temp_dir().join(format!("ohagent-vctx-{}", Uuid::new_v4()));
        let workspace = root.join("workspace");
        let protected = root.join("state");
        fs::create_dir_all(&workspace).unwrap();
        let runtime = VerifiedContextRuntime::initialize(
            "tenant-secret-name",
            "session-123",
            &workspace,
            &protected,
            VerifiedContextConfig {
                max_live_bytes: 4096,
                edit_mode: mode,
            },
        )
        .unwrap();
        (root, workspace, runtime)
    }

    #[test]
    fn initializes_session_scoped_live_context_without_raw_tenant_path() {
        let (root, _workspace, runtime) = fixture(ContextEditMode::Fit);
        let live = runtime.live_path().display().to_string();
        assert!(live.contains(".ohagent-context"));
        assert!(!live.contains("tenant-secret-name"));
        let content = fs::read_to_string(runtime.live_path()).unwrap();
        assert!(content.starts_with("<!-- OHAGENT_VERIFIED_LIVE_CONTEXT v1"));
        assert!(content.contains("## Current goals"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn accepts_valid_edit_and_persists_accepted_snapshot() {
        let (root, _workspace, runtime) = fixture(ContextEditMode::Fit);
        let _guard = runtime.lock_turn().unwrap();
        let before = runtime.begin_turn("fix the incident").unwrap();
        let mut content = fs::read_to_string(runtime.live_path()).unwrap();
        content = content.replace("- (empty)", "- inspect trace abc");
        fs::write(runtime.live_path(), content).unwrap();

        let outcome = runtime.reconcile_after_turn(&before).unwrap();
        assert!(outcome.changed);
        assert!(outcome.accepted);
        let accepted = fs::read_to_string(runtime.state_dir().join(ACCEPTED_FILE)).unwrap();
        assert!(accepted.contains("inspect trace abc"));
        let ledger = fs::read_to_string(runtime.state_dir().join(LEDGER_FILE)).unwrap();
        assert!(ledger.contains("edit_accepted"));
        assert!(!ledger.contains("fix the incident"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_header_tamper_and_restores_exact_snapshot() {
        let (root, _workspace, runtime) = fixture(ContextEditMode::Fit);
        let _guard = runtime.lock_turn().unwrap();
        let before = runtime.begin_turn("task").unwrap();
        fs::write(runtime.live_path(), "# hacked\n").unwrap();

        let outcome = runtime.reconcile_after_turn(&before).unwrap();
        assert!(outcome.changed);
        assert!(!outcome.accepted);
        let restored = fs::read_to_string(runtime.live_path()).unwrap();
        assert_eq!(sha256(restored.as_bytes()), before.before_sha256);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_growth_in_shrink_mode() {
        let (root, _workspace, runtime) = fixture(ContextEditMode::Shrink);
        let _guard = runtime.lock_turn().unwrap();
        let before = runtime.begin_turn("task").unwrap();
        let mut content = fs::read_to_string(runtime.live_path()).unwrap();
        content.push_str("\nextra state that grows the file\n");
        fs::write(runtime.live_path(), content).unwrap();

        let outcome = runtime.reconcile_after_turn(&before).unwrap();
        assert!(!outcome.accepted);
        assert!(outcome.reason.contains("shrink mode"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn notice_contains_pointer_not_live_contents_or_tenant_id() {
        let (root, _workspace, runtime) = fixture(ContextEditMode::Fit);
        let _guard = runtime.lock_turn().unwrap();
        let before = runtime.begin_turn("top secret user task").unwrap();
        let notice = runtime.decorate_user_message("hello", &before, true);
        assert!(notice.contains("LIVE_CONTEXT.md"));
        assert!(notice.contains("Context pressure is HIGH"));
        assert!(notice.ends_with("hello"));
        assert!(!notice.contains("tenant-secret-name"));
        assert!(!notice.contains("top secret user task"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn likely_secret_edit_is_rejected_and_rolled_back() {
        let (root, _workspace, runtime) = fixture(ContextEditMode::Fit);
        let _guard = runtime.lock_turn().unwrap();
        let before = runtime.begin_turn("task").unwrap();
        let mut content = fs::read_to_string(runtime.live_path()).unwrap();
        content.push_str("\ncredential: sk-abcdefghijklmnopqrstuvwxyz123456\n");
        fs::write(runtime.live_path(), content).unwrap();

        let outcome = runtime.reconcile_after_turn(&before).unwrap();
        assert!(!outcome.accepted);
        assert!(outcome.reason.contains("likely secret"));
        let restored = fs::read_to_string(runtime.live_path()).unwrap();
        assert_eq!(sha256(restored.as_bytes()), before.before_sha256);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn directory_replacement_is_repaired_on_next_turn() {
        let (root, _workspace, runtime) = fixture(ContextEditMode::Fit);
        fs::remove_file(runtime.live_path()).unwrap();
        fs::create_dir(runtime.live_path()).unwrap();

        let before = runtime.begin_turn("task").unwrap();
        let metadata = fs::symlink_metadata(runtime.live_path()).unwrap();
        assert!(metadata.is_file());
        let restored = fs::read_to_string(runtime.live_path()).unwrap();
        assert_eq!(sha256(restored.as_bytes()), before.before_sha256);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_replacement_is_rejected_and_repaired() {
        use std::os::unix::fs::symlink;

        let (root, _workspace, runtime) = fixture(ContextEditMode::Fit);
        let _guard = runtime.lock_turn().unwrap();
        let before = runtime.begin_turn("task").unwrap();
        let target = root.join("attacker.txt");
        fs::write(&target, "attacker").unwrap();
        fs::remove_file(runtime.live_path()).unwrap();
        symlink(&target, runtime.live_path()).unwrap();

        let outcome = runtime.reconcile_after_turn(&before).unwrap();
        assert!(!outcome.accepted);
        let metadata = fs::symlink_metadata(runtime.live_path()).unwrap();
        assert!(metadata.is_file());
        assert!(!metadata.file_type().is_symlink());
        let _ = fs::remove_dir_all(root);
    }
}
