use super::monitor::*;
use super::{
    trace_env_names_enabled, DHT_ACTOR_MONITOR_ENV, DHT_PLANNER_MONITOR_ENV, DHT_TRACE_ENV,
};

#[test]
fn action_effect_snapshot_records_reduction_shape() {
    let snapshot =
        action_effect_snapshot("service", "reconfigure_requested", vec!["build_runtime"]);

    assert_eq!(snapshot.domain, "service");
    assert_eq!(snapshot.action, "reconfigure_requested");
    assert_eq!(snapshot.effect_count, 1);
    assert_eq!(snapshot.effects, vec!["build_runtime"]);
}

#[test]
fn trace_env_switches_support_global_actor_and_legacy_planner_flags() {
    assert!(trace_env_names_enabled(&[DHT_TRACE_ENV], |name| name == DHT_TRACE_ENV));
    assert!(trace_env_names_enabled(
        &[DHT_ACTOR_MONITOR_ENV, DHT_PLANNER_MONITOR_ENV],
        |name| name == DHT_ACTOR_MONITOR_ENV
    ));
    assert!(trace_env_names_enabled(
        &[DHT_ACTOR_MONITOR_ENV, DHT_PLANNER_MONITOR_ENV],
        |name| name == DHT_PLANNER_MONITOR_ENV
    ));
    assert!(!trace_env_names_enabled(
        &[DHT_ACTOR_MONITOR_ENV, DHT_PLANNER_MONITOR_ENV],
        |_| false
    ));
}
