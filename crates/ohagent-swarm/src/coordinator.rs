//! Swarm coordinator — walks a TaskGraph, spawns subprocess agents, collects results.

use crate::dag::{TaskGraph, TaskKind, TaskNode, TaskState};
use chrono::Utc;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Configuration for the swarm coordinator.
#[derive(Debug, Clone)]
pub struct CoordinatorConfig {
    /// Max parallel workers
    pub max_concurrency: usize,
    /// Max DAG depth for recursive runs
    pub max_depth: u32,
    /// jcode CLI path
    pub jcode_path: String,
    /// Working directory for subprocesses
    pub working_dir: String,
    /// Timeout per worker in seconds
    pub worker_timeout_secs: u64,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 5,
            max_depth: 5,
            jcode_path: "jcode".into(),
            working_dir: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".into()),
            worker_timeout_secs: 600, // 10 min
        }
    }
}

/// Result from a worker subprocess.
#[derive(Debug, Clone)]
pub struct WorkerResult {
    /// Node ID this result corresponds to
    pub node_id: Uuid,
    /// Output text from the subprocess
    pub output: String,
    /// Did the process exit successfully?
    pub success: bool,
    /// Exit code
    pub exit_code: i32,
    /// Duration in seconds
    pub duration_secs: f64,
}

/// The swarm orchestrator — manages the full lifecycle of a swarm run.
pub struct SwarmOrchestrator {
    config: CoordinatorConfig,
    /// Shared mutable graph — coordinator writes, readers can snapshot
    graph: Arc<Mutex<TaskGraph>>,
    /// Collected results by node ID
    results: Arc<Mutex<Vec<WorkerResult>>>,
}

impl SwarmOrchestrator {
    /// Create a new orchestrator for the given graph and config.
    pub fn new(graph: TaskGraph, config: CoordinatorConfig) -> Self {
        Self {
            config,
            graph: Arc::new(Mutex::new(graph)),
            results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Run the full swarm to completion. Returns the final graph state and all results.
    pub async fn run(&self) -> (TaskGraph, Vec<WorkerResult>) {
        info!(
            goal = %self.graph.lock().await.goal,
            nodes = self.graph.lock().await.nodes.len(),
            "Swarm run started"
        );

        // Main loop: while not complete, find runnable nodes and execute them
        loop {
            // Get runnable nodes (respecting concurrency limit)
            let runnable = {
                let graph = self.graph.lock().await;
                let mut ids = graph.runnable_nodes();
                // Cap at concurrency limit (minus currently running)
                let running_count = graph
                    .state_counts()
                    .get(&TaskState::Running)
                    .copied()
                    .unwrap_or(0);
                let capacity = self.config.max_concurrency.saturating_sub(running_count);
                ids.truncate(capacity);
                ids
            };

            // Check if DAG is complete
            let done = self.graph.lock().await.is_complete();
            if done {
                info!("Swarm DAG complete");
                break;
            }

            if runnable.is_empty() {
                // Check if we're deadlocked (no runnable, not complete, no running)
                let running_count = self
                    .graph
                    .lock()
                    .await
                    .state_counts()
                    .get(&TaskState::Running)
                    .copied()
                    .unwrap_or(0);
                if running_count == 0 {
                    warn!("Swarm deadlock detected — marking remaining pending as skipped");
                    self.skip_remaining().await;
                    break;
                }
                // Otherwise wait for running tasks and re-check
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                continue;
            }

            // Mark nodes as running and spawn workers
            let mut handles = Vec::new();
            for node_id in &runnable {
                {
                    let mut graph = self.graph.lock().await;
                    if let Some(node) = graph.nodes.get_mut(node_id) {
                        node.state = TaskState::Running;
                    }
                }

                let node = {
                    let graph = self.graph.lock().await;
                    graph.nodes.get(node_id).cloned()
                };

                if let Some(node) = node {
                    let cfg = self.config.clone();
                    let graph = Arc::clone(&self.graph);
                    let results = Arc::clone(&self.results);
                    let nid = *node_id;

                    handles.push(tokio::spawn(async move {
                        let result = run_worker(nid, &node, &cfg).await;
                        // Update graph with result
                        {
                            let mut g = graph.lock().await;
                            if let Some(n) = g.nodes.get_mut(&nid) {
                                if result.success {
                                    n.state = TaskState::Completed;
                                    n.result = Some(result.output.clone());
                                } else {
                                    n.state = TaskState::Failed;
                                    n.error = Some(result.output.clone());
                                    // If retries remain, reset to pending
                                    if n.retries < n.max_retries {
                                        n.retries += 1;
                                        n.state = TaskState::Pending;
                                        debug!(node = %n.label, retry = n.retries, "Retrying failed node");
                                    }
                                }
                            }
                        }
                        results.lock().await.push(result);
                    }));
                }
            }

            // Wait for all spawned workers to complete
            for h in handles {
                if let Err(e) = h.await {
                    error!(?e, "Worker join error");
                }
            }
        }

        let graph = self.graph.lock().await.clone();
        let results = self.results.lock().await.clone();
        (graph, results)
    }

    /// Mark remaining pending nodes as skipped (deadlock recovery).
    async fn skip_remaining(&self) {
        let mut graph = self.graph.lock().await;
        for node in graph.nodes.values_mut() {
            if node.state == TaskState::Pending {
                node.state = TaskState::Skipped;
            }
        }
    }
}

/// Run a single worker: spawns jcode CLI with the node's prompt.
async fn run_worker(node_id: Uuid, node: &TaskNode, config: &CoordinatorConfig) -> WorkerResult {
    let start = Utc::now();
    debug!(label = %node.label, kind = ?node.kind, "Spawning worker");

    // Build context prompt: include dependency results if any
    let mut full_prompt = node.prompt.clone();

    // Build the jcode command
    let mut cmd = tokio::process::Command::new(&config.jcode_path);
    cmd.arg("--print") // non-interactive mode
        .arg("--output-format")
        .arg("stream-json")
        .current_dir(&config.working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Spawn the process
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            error!(?e, node = %node.label, "Failed to spawn jcode");
            return WorkerResult {
                node_id,
                output: format!("Failed to spawn jcode: {e}"),
                success: false,
                exit_code: -1,
                duration_secs: 0.0,
            };
        }
    };

    // Feed prompt via stdin
    use tokio::io::AsyncWriteExt;
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(full_prompt.as_bytes()).await {
            warn!(?e, "Failed to write to jcode stdin");
        }
        // stdin is dropped here, closing the pipe
    }

    // Wait with timeout
    let exit_status = match tokio::time::timeout(
        tokio::time::Duration::from_secs(config.worker_timeout_secs),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            error!(?e, node = %node.label, "Worker process error");
            return WorkerResult {
                node_id,
                output: format!("Worker error: {e}"),
                success: false,
                exit_code: -1,
                duration_secs: 0.0,
            };
        }
        Err(_) => {
            warn!(node = %node.label, "Worker timed out, killing");
            // child is dropped, kill_on_drop will handle it
            return WorkerResult {
                node_id,
                output: format!("Timed out after {}s", config.worker_timeout_secs),
                success: false,
                exit_code: -1,
                duration_secs: 0.0,
            };
        }
    };

    let stdout = String::from_utf8_lossy(&exit_status.stdout).to_string();
    let stderr = String::from_utf8_lossy(&exit_status.stderr).to_string();
    let combined = if stdout.is_empty() {
        stderr
    } else if stderr.is_empty() {
        stdout
    } else {
        format!("{stdout}\n--- stderr ---\n{stderr}")
    };

    let elapsed = (Utc::now() - start).num_milliseconds() as f64 / 1000.0;
    let success = exit_status.status.success();

    debug!(
        label = %node.label,
        success,
        duration = elapsed,
        "Worker completed"
    );

    WorkerResult {
        node_id,
        output: combined,
        success,
        exit_code: exit_status.status.code().unwrap_or(-1),
        duration_secs: elapsed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::{Dependency, TaskKind};

    fn make_node(id: Uuid, label: &str, kind: TaskKind, deps: Vec<Uuid>) -> TaskNode {
        TaskNode {
            id,
            label: label.into(),
            kind,
            prompt: format!("echo 'hello from {label}'"),
            depends_on: deps
                .iter()
                .map(|&nid| Dependency {
                    node_id: nid,
                    artifact_key: None,
                })
                .collect(),
            priority: 0,
            state: TaskState::Pending,
            result: None,
            error: None,
            worker_id: None,
            retries: 0,
            max_retries: 1,
        }
    }

    #[test]
    fn test_worker_result_struct() {
        let r = WorkerResult {
            node_id: Uuid::new_v4(),
            output: "ok".into(),
            success: true,
            exit_code: 0,
            duration_secs: 1.5,
        };
        assert!(r.success);
        assert_eq!(r.output, "ok");
    }

    #[test]
    fn test_config_defaults() {
        let cfg = CoordinatorConfig::default();
        assert_eq!(cfg.max_concurrency, 5);
        assert_eq!(cfg.max_depth, 5);
    }

    #[test]
    fn test_orchestrator_creation() {
        let graph = TaskGraph::new("test goal");
        let cfg = CoordinatorConfig::default();
        let _orch = SwarmOrchestrator::new(graph, cfg);
    }
}
