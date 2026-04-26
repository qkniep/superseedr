use super::super::*;
use super::test_support::*;
use super::*;

#[test]
fn demand_planner_invariants_accept_normal_active_and_draining_state() {
    let now = Instant::now();
    let mut planner = DemandPlannerModel::new(now);
    let active_hash = hash_index(100);
    let drain_hash = hash_index(101);

    planner.update(DemandPlannerAction::DemandRegistered {
        info_hash: active_hash,
        demand: DhtDemandState {
            awaiting_metadata: false,
            connected_peers: 0,
        },
        now,
    });
    planner.update(DemandPlannerAction::DemandRegistered {
        info_hash: drain_hash,
        demand: DhtDemandState {
            awaiting_metadata: false,
            connected_peers: 0,
        },
        now,
    });
    assert!(planner.scheduler.mark_in_progress(active_hash));
    assert!(planner.scheduler.mark_in_progress(drain_hash));
    planner.active.insert(
        active_hash,
        active_lookup(LookupId(100), DemandSliceClass::NoConnectedPeers),
    );
    insert_synthetic_drain(
        &mut planner.draining_demands,
        drain_hash,
        101,
        LookupId(101),
        DemandSliceClass::NoConnectedPeers,
        2,
        now,
    );

    check_demand_planner_invariants(&planner).expect("valid planner invariants");
}

#[test]
fn demand_planner_invariants_reject_active_without_scheduler_entry() {
    let now = Instant::now();
    let mut planner = DemandPlannerModel::new(now);
    let info_hash = hash_index(102);
    planner.active.insert(
        info_hash,
        active_lookup(LookupId(102), DemandSliceClass::NoConnectedPeers),
    );

    let violation =
        check_demand_planner_invariants(&planner).expect_err("expected invariant violation");

    assert_eq!(violation.kind, "active_without_scheduler_entry");
    assert_eq!(violation.info_hash, Some(info_hash));
}

#[test]
fn demand_planner_invariants_reject_duplicate_lookup_id() {
    let now = Instant::now();
    let mut planner = DemandPlannerModel::new(now);
    let left_hash = hash_index(103);
    let right_hash = hash_index(104);

    for info_hash in [left_hash, right_hash] {
        planner.update(DemandPlannerAction::DemandRegistered {
            info_hash,
            demand: DhtDemandState {
                awaiting_metadata: false,
                connected_peers: 0,
            },
            now,
        });
        assert!(planner.scheduler.mark_in_progress(info_hash));
        planner.active.insert(
            info_hash,
            active_lookup(LookupId(103), DemandSliceClass::NoConnectedPeers),
        );
    }

    let violation =
        check_demand_planner_invariants(&planner).expect_err("expected invariant violation");

    assert_eq!(violation.kind, "duplicate_lookup_id");
}

#[test]
fn demand_planner_invariants_reject_scheduler_in_progress_without_lookup_state() {
    let now = Instant::now();
    let mut planner = DemandPlannerModel::new(now);
    let info_hash = hash_index(105);
    planner.update(DemandPlannerAction::DemandRegistered {
        info_hash,
        demand: DhtDemandState {
            awaiting_metadata: false,
            connected_peers: 0,
        },
        now,
    });
    assert!(planner.scheduler.mark_in_progress(info_hash));

    let violation =
        check_demand_planner_invariants(&planner).expect_err("expected invariant violation");

    assert_eq!(violation.kind, "scheduler_in_progress_without_lookup");
    assert_eq!(violation.info_hash, Some(info_hash));
}
