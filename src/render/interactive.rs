use std::time::Duration;

use bevy::app::prelude::*;
use bevy::ecs::prelude::*;
use console::truncate_str;
use ecsdk_core::{AppExit, WakeSignal};
use ecsdk_tasks::SpawnCmdTask;

use crate::lifecycle::VmError;
use crate::phase::VmPhase;

use super::format;
use super::state::{InteractiveRenderState, InteractiveTerminal};

#[derive(Component)]
struct RenderTickDriver;

#[derive(Event)]
struct RenderTick;

const SPINNER_TICK_MS: u64 = 60;

fn line_width(terminal: &InteractiveTerminal) -> usize {
    usize::from(terminal.term.size().1).max(1)
}

fn write_transient_line(
    terminal: &mut InteractiveTerminal,
    state: &mut InteractiveRenderState,
    line: &str,
) {
    let width = line_width(terminal);
    if state.active_line_drawn {
        terminal.term.move_cursor_up(1).ok();
    } else {
        terminal.hide_cursor();
    }

    terminal.term.clear_line().ok();
    terminal
        .term
        .write_line(&truncate_str(line, width, "\u{2026}"))
        .ok();
    state.active_line_drawn = true;
    state.last_width = width;
}

fn flush_permanent_line(
    terminal: &mut InteractiveTerminal,
    state: &mut InteractiveRenderState,
    line: &str,
) {
    let width = line_width(terminal);
    if state.active_line_drawn {
        terminal.term.move_cursor_up(1).ok();
    }

    terminal.term.clear_line().ok();
    terminal
        .term
        .write_line(&truncate_str(line, width, "\u{2026}"))
        .ok();
    state.active_line_drawn = false;
    state.last_width = width;
}

fn start_render_tick_driver(mut commands: Commands) {
    commands
        .spawn(RenderTickDriver)
        .spawn_cmd_task(|cmd| async move {
            loop {
                tokio::time::sleep(Duration::from_millis(SPINNER_TICK_MS)).await;
                cmd.send(|world: &mut World| {
                    world.trigger(RenderTick);
                })
                .wake();
            }
        });
}

fn sync_phase_view(
    query: Query<(&VmPhase, Option<&VmError>), Changed<VmPhase>>,
    mut terminal: ResMut<InteractiveTerminal>,
    mut state: ResMut<InteractiveRenderState>,
) {
    for (phase, error) in &query {
        if Some(*phase) == state.active_phase {
            continue;
        }

        if let Some(previous) = state.active_phase.take()
            && *phase != VmPhase::Failed
            && let Some(label) = format::completed_phase_label(previous)
        {
            flush_permanent_line(&mut terminal, &mut state, &format::completed_line(label));
        }

        match phase {
            VmPhase::Failed => {
                let message = error
                    .map(|err| format!("Failed: {}", err.0))
                    .unwrap_or_else(|| "Failed".to_string());
                flush_permanent_line(&mut terminal, &mut state, &format::failed_line(&message));
                state.dirty = false;
            }
            VmPhase::Running | VmPhase::Stopped | VmPhase::Destroyed => {
                if let Some(label) = format::completed_phase_label(*phase) {
                    flush_permanent_line(&mut terminal, &mut state, &format::completed_line(label));
                }
                state.dirty = false;
            }
            _ => {
                if format::active_phase_label(*phase).is_some() {
                    state.active_phase = Some(*phase);
                    state.spinner_frame = 0;
                    state.dirty = true;
                }
            }
        }
    }
}

fn advance_spinner_frame(_trigger: On<RenderTick>, mut state: ResMut<InteractiveRenderState>) {
    if state.active_phase.is_some() {
        state.spinner_frame += 1;
        state.dirty = true;
    }
}

fn redraw_active_step(
    mut terminal: ResMut<InteractiveTerminal>,
    mut state: ResMut<InteractiveRenderState>,
) {
    let Some(active_phase) = state.active_phase else {
        return;
    };

    let width = line_width(&terminal);
    if width != state.last_width {
        state.dirty = true;
    }

    if !state.dirty {
        return;
    }

    let Some(label) = format::active_phase_label(active_phase) else {
        return;
    };

    let line = format::spinner_line(state.spinner_frame, label);
    write_transient_line(&mut terminal, &mut state, &line);
    state.dirty = false;
}

fn cleanup_terminal(
    exit: Res<AppExit>,
    wake: Res<WakeSignal>,
    mut terminal: ResMut<InteractiveTerminal>,
    mut state: ResMut<InteractiveRenderState>,
) {
    if !exit.0 {
        return;
    }

    if let Some(active_phase) = state.active_phase.take()
        && let Some(label) = format::completed_phase_label(active_phase)
    {
        flush_permanent_line(&mut terminal, &mut state, &format::completed_line(label));
    }

    terminal.show_cursor();
    state.dirty = false;
    wake.0.notify_one();
}

pub(super) fn build(app: &mut App) {
    app.init_resource::<InteractiveTerminal>();
    app.init_resource::<InteractiveRenderState>();
    app.add_systems(Startup, start_render_tick_driver);
    app.add_systems(
        PostUpdate,
        (sync_phase_view, redraw_active_step, cleanup_terminal).chain(),
    );
    app.add_observer(advance_spinner_frame);
}
