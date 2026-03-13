use bevy::ecs::prelude::*;
use console::Term;

use crate::cli::OutputFormat;
use crate::phase::VmPhase;

#[derive(Component, serde::Serialize, serde::Deserialize)]
pub struct StepProgress {
    pub current: usize,
    pub total: usize,
}

#[derive(Resource)]
pub struct RenderMode(pub OutputFormat);

#[derive(Resource)]
pub struct InteractiveRenderState {
    pub last_seen_phase: Option<VmPhase>,
    pub active_phase: Option<VmPhase>,
    pub spinner_frame: usize,
    pub active_line_drawn: bool,
    pub dirty: bool,
    pub last_width: usize,
}

impl Default for InteractiveRenderState {
    fn default() -> Self {
        Self {
            last_seen_phase: None,
            active_phase: None,
            spinner_frame: 0,
            active_line_drawn: false,
            dirty: false,
            last_width: 0,
        }
    }
}

#[derive(Resource)]
pub struct InteractiveTerminal {
    pub term: Term,
    pub cursor_hidden: bool,
}

impl Default for InteractiveTerminal {
    fn default() -> Self {
        Self {
            term: Term::stderr(),
            cursor_hidden: false,
        }
    }
}

impl InteractiveTerminal {
    pub fn hide_cursor(&mut self) {
        if !self.cursor_hidden {
            self.term.hide_cursor().ok();
            self.cursor_hidden = true;
        }
    }

    pub fn show_cursor(&mut self) {
        if self.cursor_hidden {
            self.term.show_cursor().ok();
            self.cursor_hidden = false;
        }
    }
}

impl Drop for InteractiveTerminal {
    fn drop(&mut self) {
        self.show_cursor();
    }
}
