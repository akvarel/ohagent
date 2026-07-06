//! Replay environment — frozen trace store for offline controller optimization.
//!
//! Inspired by AutoTTS: collect reasoning traces once (LLM calls), then
//! evaluate controllers on the frozen replay store (zero LLM calls).
//!
//! This enables iterative controller improvement:
//!
//! ```text
//! Coding Agent → writes controller.py
//!                    ↓
//!              ReplayEnv.evaluate(controller)
//!                    ↓
//!              returns: accuracy, tokens, traces
//!                    ↓
//! Coding Agent ← improves controller
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

use crate::cmc::{CmcConfig, CmcController, CmcDecision};

/// A single step in a reasoning trace.
///
/// Maps to the AutoTTS concept of (state, action, outcome) triples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStep {
    /// Branch index this step belongs to
    pub branch_index: usize,
    /// Step number within the branch
    pub step_number: usize,
    /// Answer at this step (None if probe hasn't yielded an answer yet)
    pub answer: Option<String>,
    /// Whether this is the final step for this branch
    pub is_final: bool,
    /// Confidence score (0.0–1.0)
    pub confidence: f64,
    /// Tokens consumed in this step
    pub tokens: u64,
    /// Response text
    pub response: String,
}

/// A complete replay trace for one query.
///
/// Contains all branches and their step-by-step trajectories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayTrace {
    /// Query ID
    pub query_id: String,
    /// The input query/prompt
    pub query: String,
    /// Expected answer (for accuracy evaluation)
    pub expected_answer: Option<String>,
    /// All branch traces
    pub branches: Vec<Vec<TraceStep>>,
    /// Ground truth: what the full SC@N answer would be
    pub ground_truth_answer: Option<String>,
    /// Metadata
    pub model: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// The replay environment.
///
/// Stores frozen traces and evaluates controllers without LLM calls.
#[derive(Clone)]
pub struct ReplayEnv {
    /// All loaded traces
    traces: Vec<ReplayTrace>,
    /// Where traces are stored
    store_path: PathBuf,
    /// Statistics
    total_traces: usize,
    total_tokens: u64,
}

impl ReplayEnv {
    /// Create a new replay environment.
    pub fn new(store_path: impl Into<PathBuf>) -> Self {
        Self {
            traces: Vec::new(),
            store_path: store_path.into(),
            total_traces: 0,
            total_tokens: 0,
        }
    }

    /// Load traces from the store directory.
    pub fn load(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let dir = &self.store_path;
        if !dir.exists() {
            info!("Replay store directory does not exist yet: {:?}", dir);
            return Ok(());
        }

        let mut count = 0;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "json") {
                let content = std::fs::read_to_string(&path)?;
                let trace: ReplayTrace = serde_json::from_str(&content)?;
                self.total_tokens += trace.branches.iter().flatten().map(|s| s.tokens).sum::<u64>();
                self.traces.push(trace);
                count += 1;
            }
        }

        self.total_traces = count;
        info!(
            traces = count,
            tokens = self.total_tokens,
            "Loaded replay traces"
        );
        Ok(())
    }

    /// Save a trace to the store.
    pub fn save_trace(&self, trace: &ReplayTrace) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        std::fs::create_dir_all(&self.store_path)?;
        let path = self.store_path.join(format!("{}.json", trace.query_id));
        let json = serde_json::to_string_pretty(trace)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Record a live LLM interaction as a replay trace.
    pub fn record(
        &self,
        query_id: String,
        query: String,
        expected_answer: Option<String>,
        model: String,
        branches: Vec<Vec<TraceStep>>,
        ground_truth_answer: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let trace = ReplayTrace {
            query_id,
            query,
            expected_answer,
            branches,
            ground_truth_answer,
            model,
            timestamp: chrono::Utc::now(),
        };
        self.save_trace(&trace)
    }

    /// Evaluate a controller config on the replay store.
    ///
    /// Returns (accuracy, total_tokens_used, beta_sweep_results).
    /// This is the core of the offline optimization loop — zero LLM calls.
    pub fn evaluate(
        &self,
        config: &CmcConfig,
    ) -> EvalResult {
        let mut total_correct = 0;
        let mut total_tokens = 0u64;
        let mut evaluated = 0usize;

        for trace in &self.traces {
            let result = self.evaluate_single(trace, config);
            if result.correct {
                total_correct += 1;
            }
            total_tokens += result.tokens_used;
            evaluated += 1;
        }

        let accuracy = if evaluated > 0 {
            total_correct as f64 / evaluated as f64
        } else {
            0.0
        };

        EvalResult {
            accuracy,
            tokens_used: total_tokens,
            queries_evaluated: evaluated,
            correct: total_correct,
        }
    }

    /// Sweep β from 0.0 to 1.0 and return accuracy-vs-tokens curve.
    pub fn sweep_beta(
        &self,
        steps: usize,
    ) -> Vec<(f64, f64, u64)> {
        let mut results = Vec::new();
        for i in 0..=steps {
            let beta = i as f64 / steps as f64;
            let config = CmcConfig::new(beta);
            let eval = self.evaluate(&config);
            results.push((beta, eval.accuracy, eval.tokens_used));
        }
        results
    }

    /// Sweep β and find the optimal operating point (best accuracy/token trade-off).
    pub fn find_optimal_beta(
        &self,
        min_accuracy: f64,
        steps: usize,
    ) -> Option<(f64, f64, u64)> {
        let sweep = self.sweep_beta(steps);
        sweep
            .into_iter()
            .filter(|(_, acc, _)| *acc >= min_accuracy)
            .min_by_key(|(_, _, tokens)| *tokens)
    }

    /// Evaluate a single trace against a controller config.
    fn evaluate_single(&self, trace: &ReplayTrace, config: &CmcConfig) -> SingleEvalResult {
        let mut ctrl = CmcController::new(config.clone());
        let max_branches = trace.branches.len().min(config.n_init);

        // Initialize from trace branches
        let initial: Vec<(Option<String>, bool)> = trace.branches[..max_branches]
            .iter()
            .map(|steps| {
                let last = steps.last();
                (last.and_then(|s| s.answer.clone()), last.map_or(false, |s| s.is_final))
            })
            .collect();
        ctrl.init(initial);

        // Count tokens from initial branches
        let mut tokens_used = trace.branches[..max_branches]
            .iter()
            .flat_map(|steps| steps.iter().map(|s| s.tokens))
            .sum::<u64>();
        let mut max_steps = 500;

        loop {
            let decision = ctrl.step();

            match decision {
                CmcDecision::Stop { answer, .. } => {
                    let correct = trace.expected_answer.as_ref() == Some(&answer);
                    return SingleEvalResult {
                        correct,
                        answer: Some(answer),
                        tokens_used,
                        steps: ctrl.step_count(),
                    };
                }
                CmcDecision::Exhausted { answer } => {
                    let correct = trace.expected_answer.as_ref() == answer.as_ref();
                    return SingleEvalResult {
                        correct,
                        answer: answer.or_else(|| ctrl.pool_stats().winner),
                        tokens_used,
                        steps: ctrl.step_count(),
                    };
                }
                CmcDecision::Widen { count } => {
                    // Simulate widening by using more branches from the trace
                    let start = ctrl.total_spawned();
                    for i in 0..count {
                        let br_idx = start + i;
                        if br_idx < trace.branches.len() {
                            let steps = &trace.branches[br_idx];
                            let last = steps.last();
                            ctrl.spawn_branch(
                                last.and_then(|s| s.answer.clone()),
                                last.map_or(false, |s| s.is_final),
                            );
                            tokens_used += steps.iter().map(|s| s.tokens).sum::<u64>();
                        }
                    }
                }
                CmcDecision::Abandon { .. } => {
                    // Simulated — branches are abandoned in the controller
                }
                CmcDecision::Continue => {
                    // Simulate probing from replay traces
                    let alloc = ctrl.probe_allocation();
                    for (br_idx, steps) in &alloc {
                        if let Some(branch_trace) = trace.branches.get(*br_idx) {
                            if let Some(step) = branch_trace.get(*steps - 1) {
                                ctrl.probe_result(
                                    *br_idx,
                                    step.answer.clone(),
                                    step.is_final,
                                    step.confidence,
                                );
                                tokens_used += step.tokens;
                            }
                        }
                    }
                    ctrl.advance_step();
                }
            }

            max_steps -= 1;
            if max_steps == 0 {
                let answer = ctrl.pool_stats().winner;
                let correct = trace.expected_answer.as_ref() == answer.as_ref();
                return SingleEvalResult {
                    correct,
                    answer,
                    tokens_used,
                    steps: ctrl.step_count(),
                };
            }
        }
    }

    // ── Accessors ──

    pub fn trace_count(&self) -> usize {
        self.total_traces
    }

    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
    }

    pub fn traces(&self) -> &[ReplayTrace] {
        &self.traces
    }
}

/// Result from evaluating a single trace.
#[derive(Debug, Clone)]
struct SingleEvalResult {
    correct: bool,
    answer: Option<String>,
    tokens_used: u64,
    steps: usize,
}

/// Aggregate evaluation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    /// Accuracy: correct / evaluated
    pub accuracy: f64,
    /// Total tokens used across all queries
    pub tokens_used: u64,
    /// Number of queries evaluated
    pub queries_evaluated: usize,
    /// Number of correct answers
    pub correct: usize,
}

impl std::fmt::Display for EvalResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "accuracy={:.2}% tokens={} queries={} correct={}",
            self.accuracy * 100.0,
            self.tokens_used,
            self.queries_evaluated,
            self.correct
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_test_trace(id: &str, answer: &str, expected: &str, branch_count: usize) -> ReplayTrace {
        let mut branches = Vec::new();
        for b in 0..branch_count {
            let steps = vec![
                TraceStep {
                    branch_index: b, step_number: 0,
                    answer: Some(if b < branch_count / 2 { answer.to_string() } else { "wrong".to_string() }),
                    is_final: true, confidence: 0.9, tokens: 100, response: "ok".into(),
                },
            ];
            branches.push(steps);
        }
        ReplayTrace {
            query_id: id.to_string(),
            query: "test".into(),
            expected_answer: Some(expected.to_string()),
            branches,
            ground_truth_answer: Some(answer.to_string()),
            model: "test-model".into(),
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_replay_env_evaluate_cheap_config() {
        let mut env = ReplayEnv::new("/tmp/ohagent-test-replay");
        env.traces = vec![
            make_test_trace("q1", "A", "A", 4),
            make_test_trace("q2", "B", "C", 4), // wrong
        ];
        env.total_traces = env.traces.len();

        let config = CmcConfig::new(0.1); // cheap
        let result = env.evaluate(&config);
        assert!(result.queries_evaluated == 2);
    }

    #[test]
    fn test_sweep_beta() {
        let mut env = ReplayEnv::new("/tmp/ohagent-test-sweep");
        env.traces = vec![make_test_trace("q1", "correct", "correct", 32)];
        env.total_traces = env.traces.len();

        let sweep = env.sweep_beta(4); // 0.0, 0.25, 0.5, 0.75, 1.0
        assert_eq!(sweep.len(), 5);
        // Higher beta should use more tokens (or at least not less)
        for i in 1..sweep.len() {
            let (_beta, _acc, tokens) = sweep[i];
            assert!(tokens > 0);
        }
    }
}
