// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Instant;

use super::observe_action_effect_reduction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DhtLifecycleAction {
    StartupBootstrapDue {
        now: Instant,
        due: Instant,
        active_user_lookup_count: usize,
    },
    StartupBootstrapSucceeded,
    StartupBootstrapFailed {
        warning: String,
        retry_at: Instant,
    },
    MaintenanceTick {
        active_user_lookup_count: Option<usize>,
    },
    MaintenanceFailed {
        warning: String,
    },
    HealthTick,
    RuntimeStepFailed {
        warning: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DhtLifecycleEffect {
    RunStartupBootstrap,
    ClearStartupBootstrapDue,
    SetStartupBootstrapDue(Instant),
    RunMaintenance,
    RecordRuntimeWarning {
        warning: String,
        publish_status: bool,
    },
    PublishStatus,
    ExpireRecentUniquePeers,
    SaveRuntimeState,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct DhtLifecycleReduction {
    pub(super) effects: Vec<DhtLifecycleEffect>,
}

pub(super) struct DhtLifecycleModel;

impl DhtLifecycleAction {
    fn kind(&self) -> &'static str {
        match self {
            DhtLifecycleAction::StartupBootstrapDue { .. } => "startup_bootstrap_due",
            DhtLifecycleAction::StartupBootstrapSucceeded => "startup_bootstrap_succeeded",
            DhtLifecycleAction::StartupBootstrapFailed { .. } => "startup_bootstrap_failed",
            DhtLifecycleAction::MaintenanceTick { .. } => "maintenance_tick",
            DhtLifecycleAction::MaintenanceFailed { .. } => "maintenance_failed",
            DhtLifecycleAction::HealthTick => "health_tick",
            DhtLifecycleAction::RuntimeStepFailed { .. } => "runtime_step_failed",
            DhtLifecycleAction::Shutdown => "shutdown",
        }
    }
}

impl DhtLifecycleEffect {
    fn kind(&self) -> &'static str {
        match self {
            DhtLifecycleEffect::RunStartupBootstrap => "run_startup_bootstrap",
            DhtLifecycleEffect::ClearStartupBootstrapDue => "clear_startup_bootstrap_due",
            DhtLifecycleEffect::SetStartupBootstrapDue(_) => "set_startup_bootstrap_due",
            DhtLifecycleEffect::RunMaintenance => "run_maintenance",
            DhtLifecycleEffect::RecordRuntimeWarning { .. } => "record_runtime_warning",
            DhtLifecycleEffect::PublishStatus => "publish_status",
            DhtLifecycleEffect::ExpireRecentUniquePeers => "expire_recent_unique_peers",
            DhtLifecycleEffect::SaveRuntimeState => "save_runtime_state",
        }
    }
}

impl DhtLifecycleModel {
    pub(super) fn update(action: DhtLifecycleAction) -> DhtLifecycleReduction {
        let action_kind = action.kind();
        let effects = match action {
            DhtLifecycleAction::StartupBootstrapDue {
                now,
                due,
                active_user_lookup_count,
            } => {
                if now >= due && active_user_lookup_count == 0 {
                    vec![DhtLifecycleEffect::RunStartupBootstrap]
                } else {
                    Vec::new()
                }
            }
            DhtLifecycleAction::StartupBootstrapSucceeded => {
                vec![DhtLifecycleEffect::ClearStartupBootstrapDue]
            }
            DhtLifecycleAction::StartupBootstrapFailed { warning, retry_at } => {
                vec![
                    DhtLifecycleEffect::RecordRuntimeWarning {
                        warning,
                        publish_status: false,
                    },
                    DhtLifecycleEffect::SetStartupBootstrapDue(retry_at),
                ]
            }
            DhtLifecycleAction::MaintenanceTick {
                active_user_lookup_count: Some(0),
            } => vec![DhtLifecycleEffect::RunMaintenance],
            DhtLifecycleAction::MaintenanceTick { .. } => Vec::new(),
            DhtLifecycleAction::MaintenanceFailed { warning }
            | DhtLifecycleAction::RuntimeStepFailed { warning } => {
                vec![DhtLifecycleEffect::RecordRuntimeWarning {
                    warning,
                    publish_status: true,
                }]
            }
            DhtLifecycleAction::HealthTick => vec![
                DhtLifecycleEffect::PublishStatus,
                DhtLifecycleEffect::ExpireRecentUniquePeers,
                DhtLifecycleEffect::SaveRuntimeState,
            ],
            DhtLifecycleAction::Shutdown => vec![DhtLifecycleEffect::SaveRuntimeState],
        };
        observe_action_effect_reduction(
            "lifecycle",
            action_kind,
            effects.iter().map(DhtLifecycleEffect::kind).collect(),
        );
        DhtLifecycleReduction { effects }
    }
}
