use bevy::ecs::prelude::*;

use crate::lifecycle::VmError;
use crate::phase::VmPhase;

#[derive(Default)]
pub(super) struct State {
    last_phase: Option<VmPhase>,
}

pub(super) fn render(
    query: Query<(&VmPhase, Option<&VmError>), Changed<VmPhase>>,
    mut state: Local<State>,
) {
    for (phase, error) in &query {
        if state.last_phase == Some(*phase) {
            continue;
        }
        state.last_phase = Some(*phase);

        let phase_str = format!("{phase:?}");
        if let Some(err) = error {
            let escaped = err.0.replace('\\', "\\\\").replace('"', "\\\"");
            println!(
                r#"{{"type":"transition","phase":"{phase_str}","error":"{escaped}"}}"#,
            );
        } else {
            println!(r#"{{"type":"transition","phase":"{phase_str}"}}"#);
        }
    }
}
