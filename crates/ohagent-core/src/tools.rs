//! Agent tools — pluggable capabilities the agent can call during execution.
//!
//! Tools are registered on the bridge and become available to the agent
//! via jcode's tool-calling mechanism. Each tool has a name, description,
//! and a handler function.

use ohagent_swarm::{CoordinatorConfig, SwarmOrchestrator, TaskGraph};
use std::sync::Arc;
use tracing::{info, warn};

/// A tool that the agent can invoke.
#[derive(Clone)]
pub struct Tool {
    /// Unique tool name (e.g. "swarm_run")
    pub name: String,
    /// Description shown to the agent for tool selection
    pub description: String,
    /// JSON Schema for the tool's parameters
    pub parameters_schema: serde_json::Value,
    /// Handler function — takes JSON parameters, returns JSON result
    pub handler: Arc<dyn Fn(serde_json::Value) -> ToolResult + Send + Sync>,
}

/// Result from a tool execution.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Success or failure
    pub success: bool,
    /// Human-readable output
    pub output: String,
    /// Structured data (optional)
    pub data: Option<serde_json::Value>,
    /// Error message if failed
    pub error: Option<String>,
}

impl ToolResult {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            success: true,
            output: output.into(),
            data: None,
            error: None,
        }
    }

    pub fn ok_with_data(output: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            success: true,
            output: output.into(),
            data: Some(data),
            error: None,
        }
    }

    pub fn err(output: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            success: false,
            output: output.into(),
            data: None,
            error: Some(error.into()),
        }
    }
}

/// Registry of available tools.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: Vec<Tool>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Register a tool.
    pub fn register(&mut self, tool: Tool) {
        self.tools.push(tool);
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// List all registered tool names and descriptions.
    pub fn list(&self) -> Vec<(String, String)> {
        self.tools
            .iter()
            .map(|t| (t.name.clone(), t.description.clone()))
            .collect()
    }

    /// Execute a tool by name with parameters.
    pub fn execute(&self, name: &str, params: serde_json::Value) -> Option<ToolResult> {
        let tool = self.get(name)?;
        Some((tool.handler)(params))
    }
}

// ── Built-in Tools ──

/// Create a `swarm_run` tool that spawns a DAG-based multi-agent run.
pub fn swarm_run_tool(jcode_path: String, working_dir: String) -> Tool {
    let jcode_path = jcode_path; // moved into closure
    let working_dir = working_dir;
    Tool {
        name: "swarm_run".into(),
        description:
            "Run a swarm of sub-agents to execute a task DAG in parallel. \
             Provide a JSON plan with 'goal' and 'nodes' (each with id, label, kind, prompt, depends_on). \
             Returns aggregated results from all completed nodes."
                .into(),
        parameters_schema: serde_json::json!({
            "type": "object",
            "required": ["plan"],
            "properties": {
                "plan": {
                    "type": "object",
                    "description": "Task graph plan",
                    "required": ["goal", "nodes"],
                    "properties": {
                        "goal": { "type": "string", "description": "Overall goal of this swarm" },
                        "nodes": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["id", "label", "kind", "prompt"],
                                "properties": {
                                    "id": { "type": "string", "description": "UUID for the node" },
                                    "label": { "type": "string" },
                                    "kind": {
                                        "type": "string",
                                        "enum": ["explore", "implement", "verify", "fix", "synthesize"]
                                    },
                                    "prompt": { "type": "string" },
                                    "depends_on": {
                                        "type": "array",
                                        "items": { "type": "string" }
                                    },
                                    "priority": { "type": "integer", "default": 0 }
                                }
                            }
                        },
                        "max_concurrency": { "type": "integer", "default": 5 },
                        "max_depth": { "type": "integer", "default": 5 }
                    }
                }
            }
        }),
        handler: Arc::new(move |params: serde_json::Value| {
            let plan = match params.get("plan") {
                Some(p) => p.clone(),
                None => return ToolResult::err("Missing 'plan' parameter", "plan is required"),
            };

            let goal = plan
                .get("goal")
                .and_then(|v| v.as_str())
                .unwrap_or("swarm task")
                .to_string();

            let mut graph = TaskGraph::new(&goal);

            if let Some(concurrency) = plan.get("max_concurrency").and_then(|v| v.as_u64()) {
                graph.max_concurrency = concurrency as usize;
            }
            if let Some(depth) = plan.get("max_depth").and_then(|v| v.as_u64()) {
                graph.max_depth = depth as u32;
            }

            let nodes = match plan.get("nodes").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => return ToolResult::err("Missing 'nodes' array", "nodes is required"),
            };

            for node_val in nodes {
                let id_str = match node_val.get("id").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => continue,
                };
                let id = match uuid::Uuid::parse_str(id_str) {
                    Ok(u) => u,
                    Err(e) => {
                        warn!(?e, node = id_str, "Invalid UUID, skipping node");
                        continue;
                    }
                };

                let label = node_val
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unnamed")
                    .to_string();

                let kind: ohagent_swarm::TaskKind = match node_val
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("explore")
                {
                    "explore" => ohagent_swarm::TaskKind::Explore,
                    "implement" => ohagent_swarm::TaskKind::Implement,
                    "verify" => ohagent_swarm::TaskKind::Verify,
                    "fix" => ohagent_swarm::TaskKind::Fix,
                    "synthesize" => ohagent_swarm::TaskKind::Synthesize,
                    _ => ohagent_swarm::TaskKind::Explore,
                };

                let prompt = node_val
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let deps: Vec<ohagent_swarm::Dependency> = node_val
                    .get("depends_on")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|d| {
                                let node_id = uuid::Uuid::parse_str(d.as_str()?).ok()?;
                                Some(ohagent_swarm::Dependency {
                                    node_id,
                                    artifact_key: None,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let priority = node_val
                    .get("priority")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;

                graph.add_node(ohagent_swarm::TaskNode {
                    id,
                    label,
                    kind,
                    prompt,
                    depends_on: deps,
                    priority,
                    state: ohagent_swarm::TaskState::Pending,
                    result: None,
                    error: None,
                    worker_id: None,
                    retries: 0,
                    max_retries: 2,
                });
            }

            if graph.nodes.is_empty() {
                return ToolResult::err("No valid nodes in plan", "empty graph");
            }

            info!(
                goal = %graph.goal,
                nodes = graph.nodes.len(),
                "Running swarm"
            );

            let config = CoordinatorConfig {
                max_concurrency: graph.max_concurrency,
                max_depth: graph.max_depth,
                jcode_path: jcode_path.clone(),
                working_dir: working_dir.clone(),
                worker_timeout_secs: 600,
            };

            let orchestrator = SwarmOrchestrator::new(graph, config);

            // Run synchronously via tokio runtime
            let rt = tokio::runtime::Runtime::new().unwrap();
            let (final_graph, results) = rt.block_on(orchestrator.run());

            let counts = final_graph.state_counts();
            let summary = format!(
                "Swarm complete: {:?}. {} results collected.",
                counts,
                results.len()
            );

            let result_json = serde_json::json!({
                "state_counts": counts,
                "results": results.iter().map(|r| serde_json::json!({
                    "node_id": r.node_id.to_string(),
                    "success": r.success,
                    "output_preview": &r.output[..r.output.len().min(500)],
                    "duration_secs": r.duration_secs,
                })).collect::<Vec<_>>(),
            });

            ToolResult::ok_with_data(summary, result_json)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_registry_register_and_get() {
        let mut reg = ToolRegistry::new();
        reg.register(Tool {
            name: "test_tool".into(),
            description: "a test".into(),
            parameters_schema: serde_json::json!({"type": "object"}),
            handler: Arc::new(|_| ToolResult::ok("done")),
        });

        assert!(reg.get("test_tool").is_some());
        assert!(reg.get("missing").is_none());
        assert_eq!(reg.list().len(), 1);
    }

    #[test]
    fn test_tool_registry_execute() {
        let mut reg = ToolRegistry::new();
        reg.register(Tool {
            name: "add".into(),
            description: "adds two numbers".into(),
            parameters_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "a": {"type": "number"},
                    "b": {"type": "number"}
                }
            }),
            handler: Arc::new(|params| {
                let a = params.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let b = params.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                ToolResult::ok(format!("{}", a + b))
            }),
        });

        let result = reg
            .execute("add", serde_json::json!({"a": 3, "b": 4}))
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output, "7");
    }
}
