// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use rand::seq::SliceRandom;
use rand::Rng;

use crate::app::CalculatedLimits;
use crate::resource_manager::ResourceType;

pub(crate) const MIN_STEP_RATE: f64 = 0.01;
pub(crate) const MAX_STEP_RATE: f64 = 0.10;
pub(crate) const BASELINE_ALPHA: f64 = 0.1;
pub(crate) const REALITY_CHECK_FACTOR: f64 = 2.0;

pub(crate) const MIN_PEERS: usize = 20;
pub(crate) const MIN_DISK: usize = 2;
pub(crate) const MIN_RESERVE: usize = 0;

pub(crate) const MAX_TRADE_ATTEMPTS: usize = 5;

pub(crate) fn normalize_limits_for_mode(
    limits: &CalculatedLimits,
    is_seeding: bool,
) -> CalculatedLimits {
    if is_seeding {
        let total_budget = limits
            .reserve_permits
            .saturating_add(limits.max_connected_peers)
            .saturating_add(limits.disk_read_permits)
            .saturating_add(limits.disk_write_permits);
        let peer_slots = total_budget.saturating_mul(70) / 100;
        let read_slots = total_budget.saturating_sub(peer_slots);
        return CalculatedLimits {
            reserve_permits: 0,
            max_connected_peers: peer_slots,
            disk_read_permits: read_slots,
            disk_write_permits: 0,
        };
    }

    // Downloading mode: keep total disk budget, targeting 30% read / 70% write.
    let disk_budget = limits.disk_read_permits.saturating_add(limits.disk_write_permits);
    let read_slots = disk_budget.saturating_mul(30) / 100;
    let write_slots = disk_budget.saturating_sub(read_slots);
    CalculatedLimits {
        reserve_permits: limits.reserve_permits,
        max_connected_peers: limits.max_connected_peers,
        disk_read_permits: read_slots,
        disk_write_permits: write_slots,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TuningEvaluation {
    pub(crate) new_raw_score: u64,
    pub(crate) penalty_factor: f64,
    pub(crate) new_score: u64,
    pub(crate) updated_baseline_speed_ema: f64,
    pub(crate) best_score_before: u64,
    pub(crate) baseline_u64: u64,
    pub(crate) updated_last_tuning_score: u64,
    pub(crate) updated_last_tuning_limits: CalculatedLimits,
    pub(crate) effective_limits: CalculatedLimits,
    pub(crate) accepted_improvement: bool,
    pub(crate) reality_check_applied: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TuningScore {
    pub(crate) new_raw_score: u64,
    pub(crate) penalty_factor: f64,
    pub(crate) new_score: u64,
}

pub(crate) fn compute_tuning_score(
    relevant_history: &[u64],
    current_scpb: f64,
    scpb_max: f64,
) -> TuningScore {
    let new_raw_score = if relevant_history.is_empty() {
        0
    } else {
        relevant_history.iter().sum::<u64>() / relevant_history.len() as u64
    };
    let penalty_factor = (current_scpb / scpb_max - 1.0).max(0.0);
    let new_score = (new_raw_score as f64 / (1.0 + penalty_factor)) as u64;
    TuningScore {
        new_raw_score,
        penalty_factor,
        new_score,
    }
}

pub(crate) fn evaluate_tuning_cycle(
    current_limits: &CalculatedLimits,
    last_tuning_limits: &CalculatedLimits,
    last_tuning_score: u64,
    baseline_speed_ema: f64,
    relevant_history: &[u64],
    current_scpb: f64,
    scpb_max: f64,
) -> TuningEvaluation {
    let score = compute_tuning_score(relevant_history, current_scpb, scpb_max);
    evaluate_tuning_cycle_from_score(
        current_limits,
        last_tuning_limits,
        last_tuning_score,
        baseline_speed_ema,
        score,
    )
}

pub(crate) fn evaluate_tuning_cycle_from_score(
    current_limits: &CalculatedLimits,
    last_tuning_limits: &CalculatedLimits,
    last_tuning_score: u64,
    baseline_speed_ema: f64,
    score: TuningScore,
) -> TuningEvaluation {
    let new_score_f64 = score.new_score as f64;
    let updated_baseline_speed_ema = if baseline_speed_ema == 0.0 {
        new_score_f64
    } else {
        (new_score_f64 * BASELINE_ALPHA) + (baseline_speed_ema * (1.0 - BASELINE_ALPHA))
    };

    let best_score_before = last_tuning_score;
    let baseline_u64 = updated_baseline_speed_ema as u64;
    let mut updated_last_tuning_score = last_tuning_score;
    let mut updated_last_tuning_limits = last_tuning_limits.clone();
    let mut effective_limits = current_limits.clone();
    let mut accepted_improvement = false;
    let mut reality_check_applied = false;

    if score.new_score > best_score_before {
        updated_last_tuning_score = score.new_score;
        updated_last_tuning_limits = current_limits.clone();
        accepted_improvement = true;
    } else {
        effective_limits = last_tuning_limits.clone();
        if best_score_before > 10_000
            && best_score_before > (updated_baseline_speed_ema * REALITY_CHECK_FACTOR) as u64
        {
            updated_last_tuning_score = baseline_u64;
            reality_check_applied = true;
        }
    }

    TuningEvaluation {
        new_raw_score: score.new_raw_score,
        penalty_factor: score.penalty_factor,
        new_score: score.new_score,
        updated_baseline_speed_ema,
        best_score_before,
        baseline_u64,
        updated_last_tuning_score,
        updated_last_tuning_limits,
        effective_limits,
        accepted_improvement,
        reality_check_applied,
    }
}

fn get_limit(limits: &CalculatedLimits, resource: ResourceType) -> usize {
    match resource {
        ResourceType::PeerConnection => limits.max_connected_peers,
        ResourceType::DiskRead => limits.disk_read_permits,
        ResourceType::DiskWrite => limits.disk_write_permits,
        ResourceType::Reserve => limits.reserve_permits,
    }
}

fn set_limit(limits: &mut CalculatedLimits, resource: ResourceType, value: usize) {
    match resource {
        ResourceType::PeerConnection => limits.max_connected_peers = value,
        ResourceType::DiskRead => limits.disk_read_permits = value,
        ResourceType::DiskWrite => limits.disk_write_permits = value,
        ResourceType::Reserve => limits.reserve_permits = value,
    }
}

pub(crate) fn make_random_adjustment(
    limits: CalculatedLimits,
    is_seeding: bool,
) -> (CalculatedLimits, String) {
    let mut rng = rand::rng();
    make_random_adjustment_with_rng(limits, is_seeding, &mut rng)
}

pub(crate) fn make_random_adjustment_with_rng<R: Rng + ?Sized>(
    limits: CalculatedLimits,
    is_seeding: bool,
    rng: &mut R,
) -> (CalculatedLimits, String) {
    let mut limits = if is_seeding {
        normalize_limits_for_mode(&limits, true)
    } else {
        limits
    };
    let mut parameters = vec![
        ResourceType::PeerConnection,
        ResourceType::DiskRead,
        ResourceType::Reserve,
    ];
    if !is_seeding {
        parameters.push(ResourceType::DiskWrite);
    }

    if parameters.len() < 2 {
        return (
            limits,
            "Skipped all trade attempts (0): insufficient adjustable resources".to_string(),
        );
    }

    for attempt in 0..MAX_TRADE_ATTEMPTS {
        parameters.shuffle(rng);
        let source_param = parameters[0];
        let dest_param = parameters[1];

        let source_val = get_limit(&limits, source_param);
        let dest_val = get_limit(&limits, dest_param);

        let source_min = match source_param {
            ResourceType::PeerConnection => MIN_PEERS,
            ResourceType::DiskRead => MIN_DISK,
            ResourceType::DiskWrite => MIN_DISK,
            ResourceType::Reserve => MIN_RESERVE,
        };

        let step_rate = rng.random_range(MIN_STEP_RATE..=MAX_STEP_RATE);
        let amount_to_trade = ((source_val as f64 * step_rate).ceil() as usize).max(1);
        let can_give = source_val >= source_min.saturating_add(amount_to_trade);

        if can_give {
            set_limit(
                &mut limits,
                source_param,
                source_val.saturating_sub(amount_to_trade),
            );
            set_limit(
                &mut limits,
                dest_param,
                dest_val.saturating_add(amount_to_trade),
            );

            let description = format!(
                "Traded {} from {:?} to {:?} (Attempt {})",
                amount_to_trade,
                source_param,
                dest_param,
                attempt + 1
            );
            return (limits, description);
        }
    }

    let description = format!(
        "Skipped all trade attempts ({}) this cycle: blocked by bounds",
        MAX_TRADE_ATTEMPTS
    );
    (limits, description)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[derive(Clone, Debug)]
    struct SyntheticWorkload {
        optimum: CalculatedLimits,
        peak_score: u64,
        peer_penalty: u64,
        read_penalty: u64,
        write_penalty: u64,
        base_scpb: f64,
        scpb_slope: f64,
    }

    impl SyntheticWorkload {
        fn sample(&self, limits: &CalculatedLimits) -> (u64, f64) {
            let peer_delta = limits.max_connected_peers.abs_diff(self.optimum.max_connected_peers);
            let read_delta = limits.disk_read_permits.abs_diff(self.optimum.disk_read_permits);
            let write_delta = limits
                .disk_write_permits
                .abs_diff(self.optimum.disk_write_permits);

            let raw_penalty = (peer_delta as u64)
                .saturating_mul(peer_delta as u64)
                .saturating_mul(self.peer_penalty)
                .saturating_add(
                    (read_delta as u64)
                        .saturating_mul(read_delta as u64)
                        .saturating_mul(self.read_penalty),
                )
                .saturating_add(
                    (write_delta as u64)
                        .saturating_mul(write_delta as u64)
                        .saturating_mul(self.write_penalty),
                );

            let raw_score = self.peak_score.saturating_sub(raw_penalty);

            let disk_delta = limits
                .disk_read_permits
                .saturating_add(limits.disk_write_permits)
                .abs_diff(self.optimum.disk_read_permits + self.optimum.disk_write_permits);
            let scpb = self.base_scpb + (disk_delta as f64 * self.scpb_slope);
            (raw_score, scpb)
        }
    }

    #[derive(Debug)]
    struct SimulationResult {
        best_limits: CalculatedLimits,
        best_score: u64,
        accepted_count: usize,
        reverted_count: usize,
        score_trace: Vec<u64>,
    }

    fn simulate_tuning_cycles(
        initial_limits: CalculatedLimits,
        cycles: usize,
        seed: u64,
        workload: &SyntheticWorkload,
    ) -> SimulationResult {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut limits = initial_limits.clone();
        let mut last_tuning_limits = initial_limits;
        let mut last_tuning_score = 0;
        let mut baseline_speed_ema = 0.0;
        let mut accepted_count = 0usize;
        let mut reverted_count = 0usize;
        let mut score_trace = Vec::with_capacity(cycles);
        let adaptive_max_scpb = 10.0;

        for _ in 0..cycles {
            let (raw_score, scpb) = workload.sample(&limits);
            let history = [raw_score; 60];
            let evaluation = evaluate_tuning_cycle(
                &limits,
                &last_tuning_limits,
                last_tuning_score,
                baseline_speed_ema,
                &history,
                scpb,
                adaptive_max_scpb,
            );

            if evaluation.accepted_improvement {
                accepted_count = accepted_count.saturating_add(1);
            } else {
                reverted_count = reverted_count.saturating_add(1);
            }

            score_trace.push(evaluation.new_score);
            baseline_speed_ema = evaluation.updated_baseline_speed_ema;
            last_tuning_score = evaluation.updated_last_tuning_score;
            last_tuning_limits = evaluation.updated_last_tuning_limits;
            limits = evaluation.effective_limits;

            let (next_limits, _desc) = make_random_adjustment_with_rng(limits, false, &mut rng);
            limits = next_limits;
        }

        SimulationResult {
            best_limits: last_tuning_limits,
            best_score: last_tuning_score,
            accepted_count,
            reverted_count,
            score_trace,
        }
    }

    #[test]
    fn tuner_simulation_converges_toward_known_optimum_no_noise() {
        let initial_limits = CalculatedLimits {
            reserve_permits: 20,
            max_connected_peers: 110,
            disk_read_permits: 30,
            disk_write_permits: 20,
        };
        let workload = SyntheticWorkload {
            optimum: CalculatedLimits {
                reserve_permits: 20,
                max_connected_peers: 72,
                disk_read_permits: 14,
                disk_write_permits: 10,
            },
            peak_score: 120_000,
            peer_penalty: 4,
            read_penalty: 70,
            write_penalty: 80,
            base_scpb: 4.0,
            scpb_slope: 0.15,
        };
        let result = simulate_tuning_cycles(initial_limits, 500, 7, &workload);

        assert!(result.best_score > 100_000, "Expected strong improvement in best score");
        assert!(
            result.best_limits.max_connected_peers.abs_diff(workload.optimum.max_connected_peers)
                <= 12
        );
        assert!(
            result.best_limits.disk_read_permits.abs_diff(workload.optimum.disk_read_permits) <= 4
        );
        assert!(
            result.best_limits.disk_write_permits.abs_diff(workload.optimum.disk_write_permits)
                <= 4
        );
    }

    #[test]
    fn tuner_evaluation_reverts_when_candidate_is_worse() {
        let current_limits = CalculatedLimits {
            reserve_permits: 30,
            max_connected_peers: 140,
            disk_read_permits: 40,
            disk_write_permits: 32,
        };
        let good_limits = CalculatedLimits {
            reserve_permits: 30,
            max_connected_peers: 80,
            disk_read_permits: 14,
            disk_write_permits: 10,
        };
        let history = [10_000u64; 60];
        let eval = evaluate_tuning_cycle(
            &current_limits,
            &good_limits,
            40_000,
            15_000.0,
            &history,
            12.0,
            10.0,
        );

        assert!(!eval.accepted_improvement);
        assert_eq!(
            eval.effective_limits.max_connected_peers,
            good_limits.max_connected_peers
        );
        assert_eq!(eval.effective_limits.disk_read_permits, good_limits.disk_read_permits);
        assert_eq!(
            eval.effective_limits.disk_write_permits,
            good_limits.disk_write_permits
        );
    }

    #[test]
    fn tuner_simulation_plateau_stays_stable_without_runaway() {
        let initial_limits = CalculatedLimits {
            reserve_permits: 25,
            max_connected_peers: 80,
            disk_read_permits: 12,
            disk_write_permits: 10,
        };
        let workload = SyntheticWorkload {
            optimum: CalculatedLimits {
                reserve_permits: 25,
                max_connected_peers: 80,
                disk_read_permits: 12,
                disk_write_permits: 10,
            },
            peak_score: 50_000,
            peer_penalty: 0,
            read_penalty: 0,
            write_penalty: 0,
            base_scpb: 3.0,
            scpb_slope: 0.0,
        };
        let result = simulate_tuning_cycles(initial_limits, 120, 13, &workload);

        assert!(result.reverted_count > result.accepted_count);
        assert!(result.score_trace.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn random_adjustment_respects_min_bounds_over_many_steps() {
        let mut limits = CalculatedLimits {
            reserve_permits: 40,
            max_connected_peers: MIN_PEERS + 10,
            disk_read_permits: MIN_DISK + 5,
            disk_write_permits: MIN_DISK + 5,
        };
        let mut rng = StdRng::seed_from_u64(99);

        for _ in 0..2_000 {
            let (next, _desc) = make_random_adjustment_with_rng(limits, false, &mut rng);
            limits = next;

            assert!(limits.max_connected_peers >= MIN_PEERS);
            assert!(limits.disk_read_permits >= MIN_DISK);
            assert!(limits.disk_write_permits >= MIN_DISK);
            assert!(limits.reserve_permits >= MIN_RESERVE);
        }
    }

    #[test]
    fn tuner_evaluation_reality_check_resets_stale_best_score() {
        let limits = CalculatedLimits {
            reserve_permits: 20,
            max_connected_peers: 90,
            disk_read_permits: 16,
            disk_write_permits: 12,
        };
        let history = [800u64; 60];
        let eval = evaluate_tuning_cycle(&limits, &limits, 60_000, 1_000.0, &history, 10.0, 10.0);

        assert!(eval.reality_check_applied);
        assert_eq!(eval.updated_last_tuning_score, eval.baseline_u64);
        assert!(!eval.accepted_improvement);
    }

    #[test]
    fn seeding_adjustment_disables_disk_write_trades_and_sets_zero_write_slots() {
        let limits = CalculatedLimits {
            reserve_permits: 20,
            max_connected_peers: 64,
            disk_read_permits: 12,
            disk_write_permits: 10,
        };
        let mut rng = StdRng::seed_from_u64(123);

        for _ in 0..200 {
            let (next, _desc) = make_random_adjustment_with_rng(limits.clone(), true, &mut rng);
            assert_eq!(next.disk_write_permits, 0);
        }
    }

    #[test]
    fn seeding_adjustment_preserves_total_disk_slots_by_moving_write_to_read() {
        let limits = CalculatedLimits {
            reserve_permits: 20,
            max_connected_peers: 64,
            disk_read_permits: 12,
            disk_write_permits: 10,
        };
        let expected_total = limits
            .reserve_permits
            .saturating_add(limits.max_connected_peers)
            .saturating_add(limits.disk_read_permits)
            .saturating_add(limits.disk_write_permits);
        let mut rng = StdRng::seed_from_u64(321);

        for _ in 0..200 {
            let (next, _desc) = make_random_adjustment_with_rng(limits.clone(), true, &mut rng);
            assert_eq!(next.disk_write_permits, 0);
            let next_total = next
                .reserve_permits
                .saturating_add(next.max_connected_peers)
                .saturating_add(next.disk_read_permits)
                .saturating_add(next.disk_write_permits);
            assert_eq!(next_total, expected_total);
        }
    }

    #[test]
    fn normalize_limits_for_mode_seeding_zeros_write_and_preserves_total() {
        let limits = CalculatedLimits {
            reserve_permits: 20,
            max_connected_peers: 64,
            disk_read_permits: 12,
            disk_write_permits: 10,
        };
        let normalized = normalize_limits_for_mode(&limits, true);
        let before_total = limits
            .reserve_permits
            .saturating_add(limits.max_connected_peers)
            .saturating_add(limits.disk_read_permits)
            .saturating_add(limits.disk_write_permits);
        let after_total = normalized
            .reserve_permits
            .saturating_add(normalized.max_connected_peers)
            .saturating_add(normalized.disk_read_permits)
            .saturating_add(normalized.disk_write_permits);
        assert_eq!(normalized.disk_write_permits, 0);
        assert_eq!(before_total, after_total);
    }

    #[test]
    fn normalize_limits_for_mode_seeding_targets_70_30_peer_read_and_zero_reserve_write() {
        let limits = CalculatedLimits {
            reserve_permits: 20,
            max_connected_peers: 64,
            disk_read_permits: 12,
            disk_write_permits: 10,
        };
        let normalized = normalize_limits_for_mode(&limits, true);
        assert_eq!(normalized.max_connected_peers, 74);
        assert_eq!(normalized.disk_read_permits, 32);
        assert_eq!(normalized.reserve_permits, 0);
        assert_eq!(normalized.disk_write_permits, 0);
    }

    #[test]
    fn normalize_limits_for_mode_downloading_targets_30_70_read_write_split() {
        let limits = CalculatedLimits {
            reserve_permits: 20,
            max_connected_peers: 64,
            disk_read_permits: 12,
            disk_write_permits: 10,
        };
        let normalized = normalize_limits_for_mode(&limits, false);
        let before_disk_total = limits.disk_read_permits + limits.disk_write_permits;
        let after_disk_total = normalized.disk_read_permits + normalized.disk_write_permits;
        assert_eq!(before_disk_total, after_disk_total);
        assert_eq!(normalized.disk_read_permits, 6);
        assert_eq!(normalized.disk_write_permits, 16);
    }
}
