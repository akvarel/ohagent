use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessInput {
    pub title: String,
    pub goal: String,
    pub expected_result: String,
    pub materials: String,
}
impl ProcessInput {
    pub(crate) fn valid(&self) -> bool {
        !self.title.trim().is_empty()
            && self.title.len() <= 200
            && !self.goal.trim().is_empty()
            && self.goal.len() <= 4000
            && !self.expected_result.trim().is_empty()
            && self.expected_result.len() <= 4000
            && !self.materials.trim().is_empty()
            && self.materials.len() <= 100_000
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Process {
    pub id: String,
    pub version: u64,
    #[serde(flatten)]
    pub input: ProcessInput,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub agent_definition_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_instance_id: Option<String>,
    pub attempt_id: String,
    pub status: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub verification: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub process_id: String,
    pub process_version: u64,
    pub title: String,
    pub goal: String,
    pub status: String,
    pub version: u64,
    pub watermark: u64,
    pub created_at: String,
    pub updated_at: String,
    pub tasks: Vec<Task>,
    pub edges: Vec<Edge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
}
impl Run {
    pub(crate) fn terminal(&self) -> bool {
        matches!(
            self.status.as_str(),
            "completed" | "failed" | "cancelled" | "interrupted"
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub schema_version: u32,
    pub event_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub run_id: String,
    pub run_seq: u64,
    pub occurred_at: String,
    pub recorded_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_definition_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_instance_id: Option<String>,
    pub payload: Value,
}
#[derive(Clone, Debug)]
pub struct RunExecution {
    pub tenant_id: String,
    pub run_id: String,
    pub process: Process,
    pub producer_epoch: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {
    pub process_id: String,
    pub process_version: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CancelRequest {
    pub version: u64,
}
