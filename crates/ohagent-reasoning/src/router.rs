//! Reasoning router — bridge between CMC controller and ModelRouter.
//!
//! Routes queries through the CMC controller, adapting ModelRouter's
//! model selection decisions to CMC's branching, probing, and stopping.

use std::sync::Arc;
use tracing::{debug, info};

use crate::budget::BudgetTracker;
use crate::cmc::{CmcConfig, CmcController, CmcDecision, PoolStats};
use crate::replay::ReplayEnv;

/// A single reasoning step result from a model.
#[derive(Debug, Clone)]
pub struct ReasoningStep {
    /// The answer text
    pub answer: Option<String>,
    /// Whether this branch believes it's done
    pub finished: bool,
    /// Confidence score (0.0–1.0)
    pub confidence: f64,
    /// Tokens consumed in this step
    pub tokens: u64,
    /// Which model was used
    pub model: String,
}

/// The high-level reasoning router.
///
/// Wraps the CMC controller and manages the full reasoning lifecycle
/// across multiple model calls.
pub struct ReasoningRouter {
    controller: CmcController,
    budget: BudgetTracker,
    replay: Option<ReplayEnv>,
    /// Record traces for replay optimization (enabled by default)
    record_traces: bool,
}

impl ReasoningRouter {
    /// Create a new reasoning router.
    pub fn new(config: CmcConfig, budget: BudgetTracker) -> Self {
        let controller = CmcController::new(config);
        Self {
            controller,
            budget,
            replay: None,
            record_traces: true,
        }
    }

    /// Attach a replay environment for recording and optimization.
    pub fn with_replay(mut self, replay: ReplayEnv) -> Self {
        self.replay = Some(replay);
        self
    }

    /// Enable or disable trace recording.
    pub fn with_trace_recording(mut self, enabled: bool) -> Self {
        self.record_traces = enabled;
        self
    }

    /// Update the controller config (e.g., after budget changes).
    pub fn update_config(&mut self, config: CmcConfig) {
        self.controller = CmcController::new(config);
    }

    /// Sync beta from budget.
    pub fn sync_beta(&mut self) {
        let beta = self.budget.budget_to_beta();
        self.update_config(CmcConfig::new(beta));
        debug!(%beta, "CMC beta synced from budget");
    }

    /// Initialize the controller with initial batch results.
    pub fn init(&mut self, initial: Vec<ReasoningStep>) {
        let results: Vec<(Option<String>, bool)> = initial
            .into_iter()
            .map(|s| (s.answer, s.finished))
            .collect();
        self.controller.init(results);
    }

    /// Process a reasoning step result and get the next CMC decision.
    ///
    /// This is the main loop interface. Callers:
    /// 1. Call `decide()` to get the next action
    /// 2. Execute LLM calls based on the decision
    /// 3. Feed results back via `apply_result()`
    /// 4. Repeat until Stop
    pub fn decide(&mut self) -> ReasoningAction {
        // Sync budget state before deciding
        if self.budget.remaining_fraction() < self.controller.beta() - 0.1 {
            self.sync_beta();
        }

        if self.budget.is_exceeded() {
            let stats = self.controller.pool_stats();
            return ReasoningAction::Stop {
                answer: stats.winner,
                confidence: stats.confidence,
                reason: "budget_exhausted".into(),
            };
        }

        let decision = self.controller.step();

        match decision {
            CmcDecision::Stop { answer, confidence } => {
                ReasoningAction::Stop {
                    answer: Some(answer),
                    confidence,
                    reason: "ema_gate".into(),
                }
            }
            CmcDecision::Exhausted { answer } => {
                ReasoningAction::Stop {
                    answer,
                    confidence: 0.0,
                    reason: "all_exhausted".into(),
                }
            }
            CmcDecision::Widen { count } => {
                ReasoningAction::Widen { count }
            }
            CmcDecision::Abandon { indices } => {
                ReasoningAction::Abandon { indices }
            }
            CmcDecision::Continue => {
                let alloc = self.controller.probe_allocation();
                ReasoningAction::Probe {
                    allocations: alloc,
                }
            }
        }
    }

    /// Apply results from LLM calls back to the controller.
    pub fn apply_results(&mut self, results: &[(usize, ReasoningStep)]) {
        for (branch_idx, step) in results {
            self.controller.probe_result(
                *branch_idx,
                step.answer.clone(),
                step.finished,
                step.confidence,
            );
            self.budget.record(&step.model, step.tokens / 2, step.tokens / 2);
            self.controller.add_budget(step.tokens);
        }
        self.controller.advance_step();
    }

    /// Get the controller reference.
    pub fn controller(&self) -> &CmcController {
        &self.controller
    }

    /// Get controller mutably.
    pub fn controller_mut(&mut self) -> &mut CmcController {
        &mut self.controller
    }

    /// Get budget reference.
    pub fn budget(&self) -> &BudgetTracker {
        &self.budget
    }

    /// Get budget mutably.
    pub fn budget_mut(&mut self) -> &mut BudgetTracker {
        &mut self.budget
    }

    /// Get pool statistics.
    pub fn pool_stats(&self) -> PoolStats {
        self.controller.pool_stats()
    }

    /// Format a status line for logging/monitoring.
    pub fn status_line(&self) -> String {
        let stats = self.pool_stats();
        format!(
            "CMC[β={:.2}] branches={}/{}(+{}F) pool_conf={:.3} ema_conf={:.3} completed={}/{} | {}",
            self.controller.beta(),
            self.controller.total_spawned(),
            self.controller.branches().len(),
            self.controller.completed_answers().len(),
            stats.confidence,
            self.controller.pool_stats().confidence,
            stats.completed,
            self.controller.total_spawned(),
            self.budget.status_line()
        )
    }
}

/// Action the CMC controller decides on.
#[derive(Debug, Clone)]
pub enum ReasoningAction {
    /// Probe specific branches with given step counts per branch.
    Probe {
        /// (branch_index, steps_to_probe)
        allocations: Vec<(usize, usize)>,
    },
    /// Stop reasoning and return the answer.
    Stop {
        answer: Option<String>,
        confidence: f64,
        reason: String,
    },
    /// Spawn new branches.
    Widen {
        count: usize,
    },
    /// Abandon specific branches.
    Abandon {
        indices: Vec<usize>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::BudgetConfig;

    #[test]
    fn test_router_initialization() {
        let config = CmcConfig::balanced();
        let budget = BudgetTracker::new(BudgetConfig::default());
        let mut router = ReasoningRouter::new(config, budget);

        // After init with one result, should be ready to decide
        router.init(vec![ReasoningStep {
            answer: Some("test".into()),
            finished: false,
            confidence: 0.5,
            tokens: 100,
            model: "test".into(),
        }]);

        let action = router.decide();
        assert!(matches!(action, ReasoningAction::Probe { .. } | ReasoningAction::Stop { .. }));
    }

    #[test]
    fn test_budget_sync() {
        let config = CmcConfig::thorough();
        let mut budget = BudgetTracker::new(BudgetConfig {
            max_tokens: 10,
            max_cost_cents: 1000,
            enforce: true,
        });
        budget.record("deepseek-chat", 8, 2); // 10 tokens used

        let mut router = ReasoningRouter::new(config, budget);
        let initial_beta = router.controller().beta();
        router.sync_beta();
        let new_beta = router.controller().beta();
        assert!(new_beta < initial_beta, "Beta should decrease when budget is low");
    }
}
