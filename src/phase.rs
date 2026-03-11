use bevy::ecs::prelude::*;
use ecsdk_macros::StateComponent;
use serde::{Deserialize, Serialize};

use crate::vm_state::VmState;

/// Active lifecycle phase for the VM entity.
///
/// `StateComponent` generates marker components in `vm_phase::*`
/// (e.g. `vm_phase::DownloadingImage`) plus `on_insert` hooks that
/// sync the enum value back to the entity.
#[derive(Component, StateComponent, PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
pub enum VmPhase {
    Virgin,
    DownloadingImage,
    Preparing,
    Booting,
    ConnectingAgent,
    Provisioning,
    StartingServices,
    Running,
    ShuttingDown,
    Destroying,
    Stopped,
    Destroyed,
    Failed,
}

impl VmPhase {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Stopped | Self::Destroyed | Self::Failed)
    }

    /// Map from old VmState to the initial VmPhase for a given flow intent.
    pub fn from_vm_state(state: VmState, intent: FlowIntent) -> Self {
        match intent {
            FlowIntent::FirstBoot => match state {
                VmState::Virgin | VmState::ImageCached => Self::Virgin,
                VmState::Prepared | VmState::PartialBoot => Self::Booting,
                _ => Self::Virgin,
            },
            FlowIntent::Reboot => Self::Booting,
            FlowIntent::Reattach => Self::StartingServices,
            FlowIntent::Shutdown => Self::ShuttingDown,
            FlowIntent::Destroy => Self::Destroying,
            FlowIntent::Reprovision => Self::Provisioning,
        }
    }
}

/// Which CLI command initiated this lifecycle.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowIntent {
    FirstBoot,
    Reboot,
    Reattach,
    Shutdown,
    Destroy,
    Reprovision,
}

/// Global flag: has a graceful shutdown been requested?
#[derive(Resource, Default)]
pub struct ShutdownRequested(pub bool);
