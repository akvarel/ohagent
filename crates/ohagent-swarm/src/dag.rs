//! Task DAG — nodes, dependencies, and graph structure for swarm execution.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Kind of task node — determines execution strategy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// Explore/search — gather information, read docs, scan code
    Explore,
    /// Implement/build — write code, create files, run commands
    Implement,
    /// Verify/test — run tests, validate, check invariants
    Verify,
    /// Fix/remediate — apply corrections based on verify results
    Fix,
    /// Synthesize/merge — combine results from dependencies
    Synthesize,
}

/// Execution state of a single node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Waiting for dependencies to complete
    Pending,
    /// Ready to run (all deps satisfied)
    Runnable,
    /// Currently executing
    Running,
    /// Completed successfully
    Completed,
    /// Failed (may trigger fix nodes if configured)
    Failed,
    /// Skipped (parent cancelled or dependency failed)
    Skipped,
}

/// A dependency edge: node X depends on node Y.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// ID of the node this dependency points to
    pub node_id: Uuid,
    /// Optional: what artifact/result to pass along
    pub artifact_key: Option<String>,
}

/// A single node in the task DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    /// Unique ID for this node
    pub id: Uuid,
    /// Human-readable label
    pub label: String,
    /// Kind of work this node represents
    pub kind: TaskKind,
    /// The prompt/instructions for the sub-agent
    pub prompt: String,
    /// Nodes this task depends on (must complete before this one runs)
    #[serde(default)]
    pub depends_on: Vec<Dependency>,
    /// Priority (lower = higher priority)
    #[serde(default)]
    pub priority: u32,
    /// Current execution state (managed by coordinator)
    #[serde(default = "default_state")]
    pub state: TaskState,
    /// Result output after completion (set by coordinator)
    #[serde(default)]
    pub result: Option<String>,
    /// Error message if failed
    #[serde(default)]
    pub error: Option<String>,
    /// Worker session ID (set during execution)
    #[serde(default)]
    pub worker_id: Option<String>,
    /// Retry count
    #[serde(default)]
    pub retries: u32,
    /// Maximum allowed retries
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_state() -> TaskState {
    TaskState::Pending
}
fn default_max_retries() -> u32 {
    2
}

/// The full task DAG for a swarm run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    /// All nodes keyed by ID
    pub nodes: HashMap<Uuid, TaskNode>,
    /// Root goal/prompt that triggered this swarm
    pub goal: String,
    /// Maximum parallel workers
    #[serde(default = "default_concurrency")]
    pub max_concurrency: usize,
    /// Maximum DAG depth (recursive spawning limit)
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
    /// Current depth level
    #[serde(default)]
    pub depth: u32,
}

fn default_concurrency() -> usize {
    5
}
fn default_max_depth() -> u32 {
    5
}

impl TaskGraph {
    /// Create a new empty task graph.
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            nodes: HashMap::new(),
            goal: goal.into(),
            max_concurrency: 5,
            max_depth: 5,
            depth: 0,
        }
    }

    /// Add a node to the graph.
    pub fn add_node(&mut self, node: TaskNode) -> Uuid {
        let id = node.id;
        self.nodes.insert(id, node);
        id
    }

    /// Check if a node is runnable (all deps completed, not blocked).
    pub fn is_runnable(&self, node_id: &Uuid) -> bool {
        let node = match self.nodes.get(node_id) {
            Some(n) => n,
            None => return false,
        };
        if node.state != TaskState::Pending {
            return false;
        }
        // Check all dependencies are completed
        node.depends_on.iter().all(|dep| {
            self.nodes
                .get(&dep.node_id)
                .map(|n| n.state == TaskState::Completed)
                .unwrap_or(false)
        })
    }

    /// Get all runnable nodes sorted by priority (lowest first).
    pub fn runnable_nodes(&self) -> Vec<Uuid> {
        let mut ids: Vec<Uuid> = self
            .nodes
            .iter()
            .filter(|(id, _)| self.is_runnable(id))
            .map(|(id, _)| *id)
            .collect();
        ids.sort_by_key(|id| self.nodes.get(id).map(|n| n.priority).unwrap_or(u32::MAX));
        ids
    }

    /// Check if the graph is complete (all nodes in terminal state).
    pub fn is_complete(&self) -> bool {
        self.nodes.values().all(|n| {
            matches!(
                n.state,
                TaskState::Completed | TaskState::Failed | TaskState::Skipped
            )
        })
    }

    /// Check if the graph has any failed nodes.
    pub fn has_failures(&self) -> bool {
        self.nodes.values().any(|n| n.state == TaskState::Failed)
    }

    /// Get results from all completed nodes, mapping label → result.
    pub fn gather_results(&self) -> HashMap<String, String> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.state == TaskState::Completed)
            .filter_map(|(_, n)| n.result.as_ref().map(|r| (n.label.clone(), r.clone())))
            .collect()
    }

    /// Count nodes by state.
    pub fn state_counts(&self) -> HashMap<TaskState, usize> {
        let mut counts = HashMap::new();
        for node in self.nodes.values() {
            *counts.entry(node.state.clone()).or_default() += 1;
        }
        counts
    }

    /// Get leaf nodes (nodes nothing depends on).
    pub fn leaf_nodes(&self) -> Vec<Uuid> {
        let dependents: HashSet<Uuid> = self
            .nodes
            .values()
            .flat_map(|n| n.depends_on.iter().map(|d| d.node_id))
            .collect();
        self.nodes
            .keys()
            .filter(|id| !dependents.contains(id))
            .copied()
            .collect()
    }

    /// Get root nodes (nodes that depend on nothing).
    pub fn root_nodes(&self) -> Vec<Uuid> {
        self.nodes
            .iter()
            .filter(|(_, n)| n.depends_on.is_empty())
            .map(|(id, _)| *id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: Uuid, label: &str, deps: Vec<Uuid>) -> TaskNode {
        TaskNode {
            id,
            label: label.into(),
            kind: TaskKind::Explore,
            prompt: format!("Do {}", label),
            depends_on: deps
                .into_iter()
                .map(|node_id| Dependency {
                    node_id,
                    artifact_key: None,
                })
                .collect(),
            priority: 0,
            state: TaskState::Pending,
            result: None,
            error: None,
            worker_id: None,
            retries: 0,
            max_retries: 2,
        }
    }

    #[test]
    fn test_simple_linear_dag() {
        let mut graph = TaskGraph::new("test");
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();

        graph.add_node(make_node(a, "A", vec![]));
        graph.add_node(make_node(b, "B", vec![a]));
        graph.add_node(make_node(c, "C", vec![b]));

        // Only A is runnable initially
        let runnable = graph.runnable_nodes();
        assert_eq!(runnable, vec![a]);

        // Mark A completed
        graph.nodes.get_mut(&a).unwrap().state = TaskState::Completed;

        // Now B is runnable
        let runnable = graph.runnable_nodes();
        assert_eq!(runnable, vec![b]);
    }

    #[test]
    fn test_parallel_runnable() {
        let mut graph = TaskGraph::new("test");
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();

        graph.add_node(make_node(a, "A", vec![]));
        graph.add_node(make_node(b, "B", vec![]));
        graph.add_node(make_node(c, "C", vec![a, b]));

        let runnable = graph.runnable_nodes();
        assert_eq!(runnable.len(), 2);
        assert!(runnable.contains(&a));
        assert!(runnable.contains(&b));
    }

    #[test]
    fn test_leaf_and_root() {
        let mut graph = TaskGraph::new("test");
        let root = Uuid::new_v4();
        let mid = Uuid::new_v4();
        let leaf = Uuid::new_v4();

        graph.add_node(make_node(root, "root", vec![]));
        graph.add_node(make_node(mid, "mid", vec![root]));
        graph.add_node(make_node(leaf, "leaf", vec![mid]));

        assert_eq!(graph.root_nodes(), vec![root]);
        assert_eq!(graph.leaf_nodes(), vec![leaf]);
    }
}
