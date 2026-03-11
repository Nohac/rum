use bevy::ecs::prelude::*;
use console::Term;

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
    let term = Term::stderr();
    for (phase, progress, error) in &query {
        if state.last_phase == Some(*phase) {
            continue;
        }
        state.last_phase = Some(*phase);

        let prefix = format!("[{}/{}]", progress.current, progress.total);
        let msg = match phase {
            VmPhase::DownloadingImage => Some(format!("{prefix} \u{2713} Base image ready")),
            VmPhase::Preparing => Some(format!("{prefix} \u{2713} Downloading image")),
            VmPhase::Booting => Some(format!("{prefix} \u{2713} VM prepared")),
            VmPhase::ConnectingAgent => Some(format!("{prefix} \u{2713} VM booted")),
            VmPhase::Provisioning => Some(format!("{prefix} \u{2713} Agent connected")),
            VmPhase::StartingServices => Some(format!("{prefix} \u{2713} Provisioned")),
            VmPhase::Running => Some(format!("{prefix} \u{2713} Ready")),
            VmPhase::Stopped => Some(format!("{prefix} \u{2713} Shut down")),
            VmPhase::Destroyed => Some(format!("{prefix} \u{2713} Destroyed")),
            VmPhase::Failed => {
                let err_msg = error.map(|e| e.0.as_str()).unwrap_or("unknown error");
                Some(format!("{prefix} \u{2717} Failed: {err_msg}"))
            }
            _ => None,
        };

        if let Some(msg) = msg {
            term.write_line(&msg).ok();
        }
    }
}
