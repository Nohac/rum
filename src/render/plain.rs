use bevy::ecs::prelude::*;

use crate::lifecycle::VmError;
use crate::phase::VmPhase;

use super::StepProgress;

#[derive(Default)]
pub(super) struct State {
    last_phase: Option<VmPhase>,
}

pub(super) fn render(
    query: Query<(&VmPhase, &StepProgress, Option<&VmError>), Changed<VmPhase>>,
    mut state: Local<State>,
) {
    for (phase, progress, error) in &query {
        if state.last_phase == Some(*phase) {
            continue;
        }
        state.last_phase = Some(*phase);

        let prefix = format!("[{}/{}]", progress.current, progress.total);
        let msg = match phase {
            VmPhase::DownloadingImage => Some(format!("{prefix} Downloading image")),
            VmPhase::Preparing => Some(format!("{prefix} Base image ready")),
            VmPhase::Booting => Some(format!("{prefix} VM prepared")),
            VmPhase::ConnectingAgent => Some(format!("{prefix} VM booted")),
            VmPhase::Provisioning => Some(format!("{prefix} Agent connected")),
            VmPhase::StartingServices => Some(format!("{prefix} Provisioned")),
            VmPhase::Running => Some(format!("{prefix} Ready")),
            VmPhase::Stopped => Some(format!("{prefix} Shut down")),
            VmPhase::Destroyed => Some(format!("{prefix} Destroyed")),
            VmPhase::Failed => {
                let err_msg = error.map(|e| e.0.as_str()).unwrap_or("unknown error");
                Some(format!("{prefix} Failed: {err_msg}"))
            }
            _ => None,
        };

        if let Some(msg) = msg {
            eprintln!("{msg}");
        }
    }
}
