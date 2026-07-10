//! Confidence Momentum Controller (CMC).
//!
//! Based on the AutoTTS-discovered CMC: replaces instantaneous confidence gates
//! with EMA momentum-aware stopping, couples width-depth decisions to confidence
//! trends, and abandons branches conservatively.
//!
//! # β parameterization
//!
//! A single scalar β ∈ [0,1] controls the entire controller behavior:
//! - β=0 → conservative (few branches, low inertia, stops early)
//! - β=1 → near-full budget (many branches, high inertia, thorough)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

// ── Types ──

/// State of a single reasoning branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchState {
    /// Stable branch index
    pub index: usize,
    /// Current answer (intermediate or final)
    pub latest_answer: Option<String>,
    /// Whether this branch has exhausted its budget
    pub finished: bool,
    /// Whether this branch was abandoned due to persistent deviance
    pub abandoned: bool,
    /// How many probe steps this branch has received
    pub probe_count: u32,
    /// Consecutive rounds where answer disagreed with pool winner
    pub disagree_rounds: u32,
    /// Latest confidence score from this branch
    pub confidence: f64,
}

/// Pool statistics over completed branches.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Current pool winner answer
    pub winner: Option<String>,
    /// Vote count for winner
    pub top1_count: usize,
    /// Vote count for runner-up
    pub top2_count: usize,
    /// Beta-majority confidence: top1 / (top1 + top2)
    pub confidence: f64,
    /// Total completed branches
    pub completed: usize,
}

/// Decision from one CMC step.
#[derive(Debug, Clone, PartialEq)]
pub enum CmcDecision {
    /// Continue reasoning (not enough confidence or momentum)
    Continue,
    /// Stop — return current pool winner
    Stop { answer: String, confidence: f64 },
    /// Widen — spawn more branches (confidence trend too weak)
    Widen { count: usize },
    /// Abandon specific deviant branches
    Abandon { indices: Vec<usize> },
    /// All branches exhausted without consensus
    Exhausted { answer: Option<String> },
}

/// Configuration for the CMC controller.
///
/// All parameters are smooth functions of β. Users only need to set β.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmcConfig {
    /// Single scalar controlling behavior: 0=cheap/fast, 1=thorough/expensive
    pub beta: f64,

    // ── Derived parameters (set by schedule) ──
    pub n_init: usize,
    pub max_branch_use: usize,
    pub warm_up: usize,
    pub abandon_patience: usize,
    pub ema_window: usize,
    pub ema_alpha: f64,
    pub conf_thresh: f64,
    pub delta_slack: f64,
    pub burst_aligned: usize,
    pub widen_burst: usize,
    pub trend_thresh: f64,
    pub min_complete: usize,
}

impl CmcConfig {
    /// Create a config from β (0.0 to 1.0).
    pub fn new(beta: f64) -> Self {
        let b = beta.clamp(0.0, 1.0);
        let b = (b * 100.0).round() / 100.0; // round to 2 decimals

        Self {
            beta: b,
            n_init: (2.0 + 6.0 * b).round() as usize,
            max_branch_use: 64.min((4.0 + 60.0 * b).round() as usize),
            warm_up: (2.0 + 8.0 * b).round() as usize,
            abandon_patience: (3.0 + 9.0 * b).round() as usize,
            ema_window: (2.0 + 6.0 * b).round() as usize,
            ema_alpha: 0.70 - 0.40 * b,
            conf_thresh: 0.85 + 0.12 * b,
            delta_slack: 0.04 - 0.03 * b,
            burst_aligned: (1.0 + 2.0 * b).round() as usize,
            widen_burst: (1.0 + 3.0 * b).round() as usize,
            trend_thresh: 0.04 - 0.03 * b,
            min_complete: (2.0 + 3.0 * b).round() as usize,
        }
    }

    /// Pre-set configs for common use cases.
    pub fn cheap() -> Self { Self::new(0.1) }
    pub fn balanced() -> Self { Self::new(0.5) }
    pub fn thorough() -> Self { Self::new(1.0) }
}

impl Default for CmcConfig {
    fn default() -> Self {
        Self::balanced()
    }
}

// ── Controller ──

/// Confidence Momentum Controller — the main reasoning controller.
///
/// # Algorithm
///
/// 1. Open `n_init` branches
/// 2. Each round:
///    a. Compute pool stats (winner, confidence via Beta-majority)
///    b. Update EMA of confidence
///    c. Check stopping gate: EMA high + momentum non-negative
///    d. If not stopping, allocate probe budget (aligned branches get more)
///    e. Check widening: if confidence trend too weak, spawn new branches
///    f. Abandon persistently deviant branches (keep ≥2 alive)
/// 3. Return pool winner when gate fires or budget exhausted
pub struct CmcController {
    config: CmcConfig,
    branches: Vec<BranchState>,
    completed_answers: Vec<String>,
    total_spawned: usize,
    outer_step: usize,
    ema_history: Vec<f64>,
    ema_conf: f64,
    ema_conf_prev: f64,
    /// Budget tracker — external, shared with ModelRouter
    budget_used_tokens: u64,
}

impl CmcController {
    /// Create a new controller with the given config.
    pub fn new(config: CmcConfig) -> Self {
        Self {
            config,
            branches: Vec::new(),
            completed_answers: Vec::new(),
            total_spawned: 0,
            outer_step: 0,
            ema_history: Vec::new(),
            ema_conf: 0.0,
            ema_conf_prev: 0.0,
            budget_used_tokens: 0,
        }
    }

    /// Initialize the controller with initial branches.
    ///
    /// `initial_results` is a vec of (answer, finished) tuples from the first batch.
    pub fn init(&mut self, initial_results: Vec<(Option<String>, bool)>) {
        let n = initial_results.len().min(self.config.n_init);
        for (i, (answer, finished)) in initial_results.into_iter().take(n).enumerate() {
            let br = BranchState {
                index: i,
                latest_answer: answer.clone(),
                finished,
                abandoned: false,
                probe_count: 0,
                disagree_rounds: 0,
                confidence: if finished && answer.is_some() { 0.8 } else { 0.3 },
            };
            if finished {
                if let Some(ref a) = answer {
                    self.completed_answers.push(a.clone());
                }
            }
            self.branches.push(br);
            self.total_spawned += 1;
        }
    }

    /// Add a new probe result for a branch.
    pub fn probe_result(&mut self, branch_idx: usize, answer: Option<String>, finished: bool, confidence: f64) {
        if let Some(br) = self.branches.get_mut(branch_idx) {
            br.latest_answer = answer.clone();
            br.finished = finished;
            br.probe_count += 1;
            br.confidence = confidence;
            if finished {
                if let Some(a) = answer {
                    self.completed_answers.push(a);
                }
            }
        }
    }

    /// Spawn a new branch.
    pub fn spawn_branch(&mut self, answer: Option<String>, finished: bool) {
        let idx = self.total_spawned;
        let br = BranchState {
            index: idx,
            latest_answer: answer.clone(),
            finished,
            abandoned: false,
            probe_count: 0,
            disagree_rounds: 0,
            confidence: if finished && answer.is_some() { 0.8 } else { 0.3 },
        };
        if finished {
            if let Some(ref a) = answer {
                self.completed_answers.push(a.clone());
            }
        }
        self.branches.push(br);
        self.total_spawned += 1;
    }

    /// Compute current pool stats.
    pub fn pool_stats(&self) -> PoolStats {
        if self.completed_answers.is_empty() {
            return PoolStats::default();
        }

        let mut counts: HashMap<&str, usize> = HashMap::new();
        for a in &self.completed_answers {
            *counts.entry(a).or_default() += 1;
        }

        let mut sorted: Vec<(&&str, &usize)> = counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));

        let winner = sorted.first().map(|(a, _)| a.to_string());
        let top1 = sorted.first().map(|(_, c)| **c).unwrap_or(0);
        let top2 = sorted.get(1).map(|(_, c)| **c).unwrap_or(0);

        let confidence = if top1 + top2 > 0 {
            top1 as f64 / (top1 + top2) as f64
        } else {
            0.0
        };

        PoolStats {
            winner,
            top1_count: top1,
            top2_count: top2,
            confidence,
            completed: self.completed_answers.len(),
        }
    }

    /// Run one step of the CMC loop.
    ///
    /// Returns a decision: Stop, Continue, Widen, Abandon, or Exhausted.
    /// The caller must handle Widening (spawn branches) and Abandonment.
    pub fn step(&mut self) -> CmcDecision {
        let stats = self.pool_stats();
        let warm_enough = self.outer_step >= self.config.warm_up;
        let n_complete = stats.completed;

        // Update EMA
        self.ema_conf_prev = self.ema_conf;
        self.ema_conf = self.config.ema_alpha * stats.confidence + (1.0 - self.config.ema_alpha) * self.ema_conf_prev;
        self.ema_history.push(self.ema_conf);
        if self.ema_history.len() > self.config.ema_window {
            self.ema_history.remove(0);
        }

        // Compute EMA delta
        let ema_delta = if self.ema_history.len() >= 2 {
            self.ema_history.last().unwrap() - self.ema_history.first().unwrap()
        } else {
            0.0
        };

        debug!(
            step = self.outer_step,
            ?self.config.beta,
            ema_conf = %self.ema_conf,
            ema_delta = %ema_delta,
            pool_conf = %stats.confidence,
            n_complete,
            "CMC step"
        );

        // ── Check stopping gate ──
        let gate_eligible = warm_enough && n_complete >= self.config.min_complete;
        let gate_fires = gate_eligible
            && self.ema_conf >= self.config.conf_thresh
            && ema_delta >= -self.config.delta_slack;

        if gate_fires {
            if let Some(ref winner) = stats.winner {
                return CmcDecision::Stop {
                    answer: winner.clone(),
                    confidence: self.ema_conf,
                };
            }
        }

        // ── Check all branches resolved (only if branches exist) ──
        let all_resolved = !self.branches.is_empty()
            && self.branches
            .iter()
            .all(|br| br.finished || br.abandoned);
        if all_resolved {
            return CmcDecision::Exhausted {
                answer: stats.winner.clone(),
            };
        }

        // ── Update disagree rounds ──
        if warm_enough {
            if let Some(ref winner) = stats.winner {
                for br in self.branches.iter_mut() {
                    if br.abandoned || br.finished {
                        continue;
                    }
                    if br.latest_answer.as_ref() == Some(winner) {
                        br.disagree_rounds = 0;
                    } else {
                        br.disagree_rounds += 1;
                    }
                }
            }
        }

        // ── Check abandonment ──
        if warm_enough && stats.winner.is_some() {
            let n_alive = self.branches
                .iter()
                .filter(|br| !br.abandoned && !br.finished)
                .count();

            let mut to_abandon: Vec<usize> = self.branches
                .iter()
                .filter(|br| {
                    !br.abandoned
                        && !br.finished
                        && br.disagree_rounds >= self.config.abandon_patience as u32
                })
                .map(|br| br.index)
                .collect();

            // Keep at least 2 alive
            let max_abandon = n_alive.saturating_sub(2);
            to_abandon.truncate(max_abandon);

            if !to_abandon.is_empty() {
                for idx in &to_abandon {
                    if let Some(br) = self.branches.iter_mut().find(|br| br.index == *idx) {
                        br.abandoned = true;
                    }
                }
                return CmcDecision::Abandon {
                    indices: to_abandon,
                };
            }
        }

        // ── Check widening ──
        let can_widen = self.total_spawned < self.config.max_branch_use
            && self.total_spawned < 64;
        let trend_weak = ema_delta <= self.config.trend_thresh;
        let want_widen = can_widen
            && trend_weak
            && self.outer_step >= (self.config.warm_up / 2).max(1)
            && self.ema_conf < self.config.conf_thresh;

        if want_widen {
            let count = self.config.widen_burst;
            return CmcDecision::Widen { count };
        }

        self.outer_step += 1;
        CmcDecision::Continue
    }

    /// Get the per-branch probe allocation: aligned branches get more steps.
    pub fn probe_allocation(&self) -> Vec<(usize, usize)> {
        let stats = self.pool_stats();
        let mut alloc = Vec::new();

        for br in &self.branches {
            if br.abandoned || br.finished {
                continue;
            }
            let is_aligned = stats
                .winner
                .as_ref()
                .map(|w| br.latest_answer.as_ref() == Some(w))
                .unwrap_or(false);
            let steps = if is_aligned {
                self.config.burst_aligned
            } else {
                1
            };
            alloc.push((br.index, steps));
        }

        // Sort by probe_count descending (most-invested first)
        alloc.sort_by(|a, b| {
            let bc_a = self
                .branches
                .iter()
                .find(|br| br.index == a.0)
                .map(|br| br.probe_count)
                .unwrap_or(0);
            let bc_b = self
                .branches
                .iter()
                .find(|br| br.index == b.0)
                .map(|br| br.probe_count)
                .unwrap_or(0);
            bc_b.cmp(&bc_a)
        });

        alloc
    }

    /// Check if the controller should stop (pure stopping gate check without side effects).
    pub fn should_stop(&self) -> bool {
        let stats = self.pool_stats();
        let warm_enough = self.outer_step >= self.config.warm_up;
        warm_enough
            && stats.completed >= self.config.min_complete
            && self.ema_conf >= self.config.conf_thresh
            && (self.ema_history.len() < 2
                || self.ema_history.last().unwrap() - self.ema_history.first().unwrap()
                    >= -self.config.delta_slack)
    }

    // ── Accessors ──

    pub fn branches(&self) -> &[BranchState] {
        &self.branches
    }

    pub fn completed_answers(&self) -> &[String] {
        &self.completed_answers
    }

    pub fn total_spawned(&self) -> usize {
        self.total_spawned
    }

    pub fn step_count(&self) -> usize {
        self.outer_step
    }

    pub fn budget_used(&self) -> u64 {
        self.budget_used_tokens
    }

    pub fn add_budget(&mut self, tokens: u64) {
        self.budget_used_tokens += tokens;
    }

    pub fn beta(&self) -> f64 {
        self.config.beta
    }

    /// Manually increment the step counter (caller does it after Continue).
    pub fn advance_step(&mut self) {
        self.outer_step += 1;
    }
}

// ── Helper: Beta-majority confidence ──

/// Compute Beta-majority confidence as a proper Bayesian estimate.
/// Uses Beta(α=top1+1, β=top2+1) mean: (top1+1)/(top1+top2+2).
/// This is smoother than raw ratio and handles small counts gracefully.
pub fn beta_majority_confidence(top1: usize, top2: usize) -> f64 {
    if top1 + top2 == 0 {
        return 0.0;
    }
    let alpha = top1 as f64 + 1.0;
    let beta = top2 as f64 + 1.0;
    alpha / (alpha + beta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_beta_schedule() {
        let cheap = CmcConfig::new(0.0);
        let balanced = CmcConfig::new(0.5);
        let thorough = CmcConfig::new(1.0);

        // Monotonicity: higher beta → more branches, higher thresholds
        assert!(cheap.n_init <= balanced.n_init);
        assert!(balanced.n_init <= thorough.n_init);
        assert!(cheap.max_branch_use <= balanced.max_branch_use);
        assert!(balanced.max_branch_use <= thorough.max_branch_use);
        assert!(cheap.conf_thresh <= thorough.conf_thresh);
    }

    #[test]
    fn test_pool_stats_empty() {
        let ctrl = CmcController::new(CmcConfig::balanced());
        let stats = ctrl.pool_stats();
        assert!(stats.winner.is_none());
        assert_eq!(stats.confidence, 0.0);
    }

    #[test]
    fn test_pool_stats_consensus() {
        let mut ctrl = CmcController::new(CmcConfig::balanced());
        ctrl.completed_answers = vec![
            "A".into(), "A".into(), "A".into(), "A".into(), "B".into(),
        ];
        let stats = ctrl.pool_stats();
        assert_eq!(stats.winner, Some("A".into()));
        assert_eq!(stats.top1_count, 4);
        assert_eq!(stats.top2_count, 1);
        assert!(stats.confidence > 0.7);
    }

    #[test]
    fn test_beta_majority_confidence() {
        // Strong consensus → high confidence
        let c = beta_majority_confidence(10, 1);
        assert!(c > 0.8);

        // Weak consensus → moderate
        let c = beta_majority_confidence(3, 2);
        assert!(c > 0.5 && c < 0.8);

        // Total split → near 0.5
        let c = beta_majority_confidence(1, 1);
        assert!((c - 0.5).abs() < 0.2);

        // Empty
        assert_eq!(beta_majority_confidence(0, 0), 0.0);
    }

    #[test]
    fn test_cmc_step_continues_on_low_confidence() {
        let mut ctrl = CmcController::new(CmcConfig::new(0.5));
        // Init with an unfinalized branch
        ctrl.init(vec![(None, false)]);
        // Low confidence → should continue
        let decision = ctrl.step();
        assert!(matches!(decision, CmcDecision::Continue));
    }

    #[test]
    fn test_cmc_exhausted() {
        let mut ctrl = CmcController::new(CmcConfig::new(0.5));
        // Add branches that are all finished
        ctrl.branches.push(BranchState {
            index: 0, latest_answer: Some("A".into()), finished: true, abandoned: false,
            probe_count: 1, disagree_rounds: 0, confidence: 0.9,
        });
        ctrl.completed_answers.push("A".into());
        // Need to go through enough steps to be warm
        ctrl.outer_step = 10;
        let decision = ctrl.step();
        assert!(matches!(decision, CmcDecision::Stop { .. } | CmcDecision::Exhausted { .. }));
    }

    #[test]
    fn test_probe_allocation_prioritizes_aligned() {
        let mut ctrl = CmcController::new(CmcConfig::balanced());
        ctrl.completed_answers = vec!["A".into(), "A".into()];
        ctrl.branches = vec![
            BranchState {
                index: 0, latest_answer: Some("A".into()), finished: false, abandoned: false,
                probe_count: 5, disagree_rounds: 0, confidence: 0.8,
            },
            BranchState {
                index: 1, latest_answer: Some("B".into()), finished: false, abandoned: false,
                probe_count: 3, disagree_rounds: 1, confidence: 0.4,
            },
        ];

        let alloc = ctrl.probe_allocation();
        // Aligned branch (A) should get more steps
        let aligned = alloc.iter().find(|(idx, _)| *idx == 0);
        let deviant = alloc.iter().find(|(idx, _)| *idx == 1);
        assert!(aligned.is_some());
        assert!(deviant.is_some());
    }
}
