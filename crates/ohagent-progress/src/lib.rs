//! Bounded, metadata-only activity projection. Never accepts prompts or tool output.
pub mod publisher;
pub mod receiver;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

pub const MAX_SESSIONS: usize = 128;
pub const MAX_EVENTS: usize = 24;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Idle,
    Working,
    Waiting,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActivityEvent {
    pub at: i64,
    pub kind: Activity,
    pub tool: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionActivity {
    /// Opaque tenant-local digest, never the runtime's raw session identifier.
    pub id: String,
    pub status: Activity,
    pub updated_at: i64,
    pub started_at: i64,
    pub tools: BTreeMap<String, u64>,
    pub current_tool: Option<String>,
    /// Latest provider call, not cumulative spend or a sum of context windows.
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub events: VecDeque<ActivityEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: u8,
    pub tenant_id: String,
    pub source_id: String,
    pub boot_id: String,
    pub sequence: u64,
    pub generated_at: i64,
    pub sessions: Vec<SessionActivity>,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:@".contains(&c))
}

pub fn session_digest(tenant: &str, session: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(tenant.as_bytes());
    hash.update([0]);
    hash.update(session.as_bytes());
    format!("{:x}", hash.finalize())[..24].to_string()
}

#[derive(Clone)]
pub struct ProgressStore {
    inner: Arc<Mutex<BTreeMap<(String, String), SessionActivity>>>,
    boot_id: String,
}
impl Default for ProgressStore {
    fn default() -> Self {
        Self {
            inner: Default::default(),
            boot_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}
impl ProgressStore {
    pub fn update(
        &self,
        tenant: &str,
        session: &str,
        status: Activity,
        tool: Option<&str>,
        usage: Option<(u64, u64)>,
    ) {
        if tenant.trim().is_empty() || session.is_empty() {
            return;
        }
        let id = session_digest(tenant, session);
        let key = (tenant.to_owned(), id.clone());
        let mut all = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // Hard global cap bounds memory even if a caller supplies many tenants.
        if !all.contains_key(&key) && all.len() >= MAX_SESSIONS {
            if let Some(oldest) = all
                .iter()
                .min_by_key(|(_, v)| v.updated_at)
                .map(|(k, _)| k.clone())
            {
                all.remove(&oldest);
            }
        }
        let now = now_ms();
        let item = all.entry(key).or_insert_with(|| SessionActivity {
            id,
            status: Activity::Idle,
            updated_at: now,
            started_at: now,
            tools: BTreeMap::new(),
            current_tool: None,
            input_tokens: 0,
            output_tokens: 0,
            events: VecDeque::new(),
        });
        if status == Activity::Working
            && item.status != Activity::Working
            && item.status != Activity::Waiting
        {
            item.started_at = now;
        }
        let tool = tool
            .filter(|s| valid_id(s) && s.len() <= 64)
            .map(str::to_owned);
        let changed = item.status != status || item.current_tool != tool;
        item.status = status;
        item.updated_at = now;
        if let Some(ref name) = tool {
            if item.tools.len() < 64 || item.tools.contains_key(name) {
                let count = item.tools.entry(name.clone()).or_default();
                *count = count.saturating_add(1);
            }
        }
        item.current_tool = tool.clone();
        if let Some((input, output)) = usage {
            item.input_tokens = input;
            item.output_tokens = output;
        }
        if changed {
            item.events.push_back(ActivityEvent {
                at: now,
                kind: status,
                tool,
            });
            while item.events.len() > MAX_EVENTS {
                item.events.pop_front();
            }
        }
    }
    pub fn finish(&self, tenant: &str, session: &str, success: bool) {
        let key = (tenant.to_owned(), session_digest(tenant, session));
        let terminal = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .is_some_and(|s| matches!(s.status, Activity::Cancelled | Activity::Failed));
        if !terminal {
            self.update(
                tenant,
                session,
                if success {
                    Activity::Completed
                } else {
                    Activity::Failed
                },
                None,
                None,
            );
        }
    }
    pub fn snapshot(&self, tenant: &str, source: &str, sequence: u64) -> Snapshot {
        let all = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut sessions: Vec<_> = all
            .iter()
            .filter(|((t, _), _)| t == tenant)
            .map(|(_, v)| v.clone())
            .collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        Snapshot {
            version: 1,
            tenant_id: tenant.into(),
            source_id: source.into(),
            boot_id: self.boot_id.clone(),
            sequence,
            generated_at: now_ms(),
            sessions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tenants_and_raw_identifiers_are_isolated() {
        let store = ProgressStore::default();
        store.update(
            "a",
            "private-session",
            Activity::Working,
            Some("read"),
            None,
        );
        store.update("b", "private-session", Activity::Failed, None, None);
        let a = store.snapshot("a", "desktop", 1);
        let b = store.snapshot("b", "desktop", 1);
        assert_eq!(a.sessions.len(), 1);
        assert_ne!(a.sessions[0].id, b.sessions[0].id);
        assert_eq!(a.sessions[0].status, Activity::Working);
        assert!(!serde_json::to_string(&a)
            .unwrap()
            .contains("private-session"));
        assert!(store.snapshot("c", "desktop", 1).sessions.is_empty());
    }
    #[test]
    fn bounded_and_latest_usage_not_accumulated() {
        let store = ProgressStore::default();
        for n in 0..200 {
            store.update(
                "a",
                "s",
                Activity::Working,
                Some(&format!("tool{n}")),
                Some((100, 20)),
            );
        }
        let s = store.snapshot("a", "desktop", 1).sessions.remove(0);
        assert_eq!(s.tools.len(), 64);
        assert_eq!(s.events.len(), MAX_EVENTS);
        assert_eq!((s.input_tokens, s.output_tokens), (100, 20));
        for n in 0..200 {
            store.update("a", &n.to_string(), Activity::Idle, None, None);
        }
        assert_eq!(
            store.snapshot("a", "desktop", 1).sessions.len(),
            MAX_SESSIONS
        );
    }
    #[test]
    fn invalid_tool_names_do_not_escape_metadata_boundary() {
        let store = ProgressStore::default();
        store.update(
            "a",
            "s",
            Activity::Working,
            Some("bash secret=PRIVATE"),
            None,
        );
        assert!(store.snapshot("a", "desktop", 1).sessions[0]
            .tools
            .is_empty());
    }
}
