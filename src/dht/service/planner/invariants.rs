// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::dht::service) struct DemandPlannerInvariantViolation {
    pub(in crate::dht::service) kind: &'static str,
    pub(in crate::dht::service) info_hash: Option<InfoHash>,
    pub(in crate::dht::service) detail: String,
}

impl DemandPlannerInvariantViolation {
    fn new(kind: &'static str, info_hash: Option<InfoHash>, detail: impl Into<String>) -> Self {
        Self {
            kind,
            info_hash,
            detail: detail.into(),
        }
    }

    fn info_hash_label(&self) -> String {
        optional_info_hash_label(self.info_hash)
    }
}

pub(in crate::dht::service) fn check_demand_planner_invariants(
    model: &DemandPlannerModel,
) -> Result<(), DemandPlannerInvariantViolation> {
    let mut occupied = HashSet::new();
    let mut lookup_ids = HashSet::new();

    for (&info_hash, active) in &model.active {
        if !occupied.insert(info_hash) {
            return Err(DemandPlannerInvariantViolation::new(
                "duplicate_active_or_draining_demand",
                Some(info_hash),
                "demand is present more than once in active/draining state",
            ));
        }
        let Some(snapshot) = model.scheduler.entry_snapshot(info_hash) else {
            return Err(DemandPlannerInvariantViolation::new(
                "active_without_scheduler_entry",
                Some(info_hash),
                "active demand has no scheduler entry",
            ));
        };
        if !snapshot.in_progress {
            return Err(DemandPlannerInvariantViolation::new(
                "active_scheduler_not_in_progress",
                Some(info_hash),
                "active demand scheduler entry is not in progress",
            ));
        }

        let Ok(active_ids) = active.lookup_ids.lock() else {
            return Err(DemandPlannerInvariantViolation::new(
                "active_lookup_ids_lock_poisoned",
                Some(info_hash),
                "active lookup ids lock was poisoned",
            ));
        };
        if active_ids.is_empty() {
            return Err(DemandPlannerInvariantViolation::new(
                "active_without_lookup_ids",
                Some(info_hash),
                "active demand has no lookup ids",
            ));
        }
        for lookup_id in active_ids.iter().copied() {
            if !lookup_ids.insert(lookup_id) {
                return Err(DemandPlannerInvariantViolation::new(
                    "duplicate_lookup_id",
                    Some(info_hash),
                    format!("lookup id {:?} is tracked more than once", lookup_id),
                ));
            }
        }
    }

    for (&info_hash, drain) in &model.draining_demands {
        if !occupied.insert(info_hash) {
            return Err(DemandPlannerInvariantViolation::new(
                "duplicate_active_or_draining_demand",
                Some(info_hash),
                "demand is present in both active and draining state",
            ));
        }
        let Some(snapshot) = model.scheduler.entry_snapshot(info_hash) else {
            return Err(DemandPlannerInvariantViolation::new(
                "draining_without_scheduler_entry",
                Some(info_hash),
                "draining demand has no scheduler entry",
            ));
        };
        if !snapshot.in_progress {
            return Err(DemandPlannerInvariantViolation::new(
                "draining_scheduler_not_in_progress",
                Some(info_hash),
                "draining demand scheduler entry is not in progress",
            ));
        }
        if drain.lookup_ids.is_empty() {
            return Err(DemandPlannerInvariantViolation::new(
                "draining_without_lookup_ids",
                Some(info_hash),
                "draining demand has no lookup ids",
            ));
        }
        for lookup_id in drain.lookup_ids.iter().copied() {
            if !lookup_ids.insert(lookup_id) {
                return Err(DemandPlannerInvariantViolation::new(
                    "duplicate_lookup_id",
                    Some(info_hash),
                    format!("lookup id {:?} is tracked more than once", lookup_id),
                ));
            }
        }
        if drain.deadline < drain.started_at {
            return Err(DemandPlannerInvariantViolation::new(
                "drain_deadline_before_start",
                Some(info_hash),
                "drain deadline is earlier than its start time",
            ));
        }
        if drain.no_late_yield_deadline > drain.deadline {
            return Err(DemandPlannerInvariantViolation::new(
                "drain_no_late_yield_after_deadline",
                Some(info_hash),
                "no-late-yield deadline exceeds drain deadline",
            ));
        }
        if drain.unique_peer_count() < drain.initial_unique_peers {
            return Err(DemandPlannerInvariantViolation::new(
                "drain_unique_count_below_initial",
                Some(info_hash),
                "drain unique peer count is lower than initial unique peers",
            ));
        }
        if drain.total_peers < drain.unique_peer_count() {
            return Err(DemandPlannerInvariantViolation::new(
                "drain_total_below_unique",
                Some(info_hash),
                "drain total peer count is lower than unique peer count",
            ));
        }
        if drain.initial_inflight_queries == 0 {
            return Err(DemandPlannerInvariantViolation::new(
                "drain_without_initial_inflight",
                Some(info_hash),
                "draining demand has no initial inflight queries",
            ));
        }
    }

    let scheduler_snapshots = model.scheduler.entry_snapshots();
    for snapshot in &scheduler_snapshots {
        if snapshot.subscriber_count == 0 {
            return Err(DemandPlannerInvariantViolation::new(
                "scheduler_entry_without_subscribers",
                Some(snapshot.info_hash),
                "scheduler entry has no subscribers",
            ));
        }
        if snapshot.in_progress
            && !model.active.contains_key(&snapshot.info_hash)
            && !model.draining_demands.contains_key(&snapshot.info_hash)
        {
            return Err(DemandPlannerInvariantViolation::new(
                "scheduler_in_progress_without_lookup",
                Some(snapshot.info_hash),
                "scheduler entry is in progress without active or draining lookup state",
            ));
        }
        if !snapshot.in_progress
            && (model.active.contains_key(&snapshot.info_hash)
                || model.draining_demands.contains_key(&snapshot.info_hash))
        {
            return Err(DemandPlannerInvariantViolation::new(
                "scheduler_idle_with_lookup",
                Some(snapshot.info_hash),
                "scheduler entry is idle while lookup state is tracked",
            ));
        }
    }

    let expected_metadata_waiters = scheduler_snapshots
        .iter()
        .filter(|snapshot| snapshot.demand.awaiting_metadata)
        .count();
    if model.metadata_waiter_count() != expected_metadata_waiters {
        return Err(DemandPlannerInvariantViolation::new(
            "metadata_waiter_count_mismatch",
            None,
            format!(
                "metadata waiter count {} did not match scheduler count {}",
                model.metadata_waiter_count(),
                expected_metadata_waiters
            ),
        ));
    }

    let active_counts = active_demand_lookup_slot_counts(&model.active);
    if active_counts.awaiting_metadata > DHT_AWAITING_METADATA_SLOT_CAP {
        return Err(DemandPlannerInvariantViolation::new(
            "awaiting_metadata_slot_cap_exceeded",
            None,
            format!(
                "awaiting metadata active slots {} exceeded cap {}",
                active_counts.awaiting_metadata, DHT_AWAITING_METADATA_SLOT_CAP
            ),
        ));
    }
    if active_counts.no_connected_peers > DHT_NO_CONNECTED_PEERS_SLOT_CAP {
        return Err(DemandPlannerInvariantViolation::new(
            "no_connected_peers_slot_cap_exceeded",
            None,
            format!(
                "no-peer active slots {} exceeded cap {}",
                active_counts.no_connected_peers, DHT_NO_CONNECTED_PEERS_SLOT_CAP
            ),
        ));
    }
    if active_counts.routine_refresh > DHT_ROUTINE_LOOKUP_SLOT_CAP {
        return Err(DemandPlannerInvariantViolation::new(
            "routine_refresh_slot_cap_exceeded",
            None,
            format!(
                "routine active slots {} exceeded cap {}",
                active_counts.routine_refresh, DHT_ROUTINE_LOOKUP_SLOT_CAP
            ),
        ));
    }

    let consumed_slots = model
        .active
        .len()
        .saturating_add(drain_virtual_slot_count(model.draining_demands.len()));
    if consumed_slots > DHT_DEMAND_LOOKUP_SLOT_COUNT {
        return Err(DemandPlannerInvariantViolation::new(
            "lookup_slot_budget_exceeded",
            None,
            format!(
                "active plus virtual drain slots {} exceeded cap {}",
                consumed_slots, DHT_DEMAND_LOOKUP_SLOT_COUNT
            ),
        ));
    }

    Ok(())
}

pub(in crate::dht::service) fn observe_demand_planner_invariants(
    action: &'static str,
    model: &DemandPlannerModel,
) {
    if !dht_invariant_checks_enabled() {
        return;
    }

    if let Err(violation) = check_demand_planner_invariants(model) {
        tracing::error!(
            target: "superseedr::dht_invariant",
            event = "violation",
            action,
            invariant = violation.kind,
            info_hash = %violation.info_hash_label(),
            detail = %violation.detail,
            "DHT planner invariant violation",
        );
    }
}
