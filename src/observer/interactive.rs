//! Interactive TTY observer — spinners, progress bars, colored output.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use console::{Term, truncate_str};

use crate::flow::Event;

use super::{EffectData, Observer, Transition};

const MAX_LOG_LINES: usize = 10;

/// Shared mutable state for the interactive observer.
struct ObserverState {
    step: usize,
    total_steps: usize,
    /// Number of log lines currently drawn in the terminal's active area.
    /// Used by the stream handler to erase log lines before the transition
    /// handler prints the completion checkmark.
    drawn_log_lines: usize,
}

/// Interactive TTY observer with step-based progress output.
///
/// Uses `Arc<Mutex<_>>` for shared state so `clone_for_stream()` is cheap.
#[derive(Clone)]
pub struct InteractiveObserver {
    state: Arc<Mutex<ObserverState>>,
    term: Term,
}

impl InteractiveObserver {
    /// Create a new interactive observer.
    ///
    /// `total_steps` is the estimated number of steps for the current flow
    /// (e.g. 5 for a full first-boot: image, prepare, boot, provision, ready).
    pub fn new(total_steps: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(ObserverState {
                step: 0,
                total_steps,
                drawn_log_lines: 0,
            })),
            term: Term::stderr(),
        }
    }

    /// Increment the step counter and return `[step/total]` prefix.
    fn next_step(&self) -> String {
        let mut state = self.state.lock().unwrap();
        state.step += 1;
        format!("[{}/{}]", state.step, state.total_steps)
    }

    /// Clear any log lines currently drawn in the active area.
    fn clear_log_area(&self) {
        let mut state = self.state.lock().unwrap();
        if state.drawn_log_lines > 0 {
            self.term.move_cursor_up(state.drawn_log_lines).ok();
            self.term.clear_to_end_of_screen().ok();
            state.drawn_log_lines = 0;
        }
    }
}

impl Observer for InteractiveObserver {
    fn on_transition(
        &mut self,
        t: &Transition,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        // Clear any leftover log lines before printing the step completion.
        self.clear_log_area();

        let message = match &t.event {
            Event::ImageReady(_) => {
                Some(format!("{} \u{2713} Base image ready", self.next_step()))
            }
            Event::VmPrepared => {
                Some(format!("{} \u{2713} VM prepared", self.next_step()))
            }
            Event::DomainStarted => {
                Some(format!("{} \u{2713} VM booted", self.next_step()))
            }
            Event::ScriptCompleted { name } => {
                Some(format!("{} \u{2713} {}", self.next_step(), name))
            }
            Event::ServicesStarted => {
                Some(format!("{} \u{2713} Ready", self.next_step()))
            }
            Event::ShutdownComplete => {
                Some(format!("{} \u{2713} Shut down", self.next_step()))
            }
            _ => None,
        };

        Box::pin(async move {
            if let Some(msg) = message {
                self.term.write_line(&msg).ok();
            }
        })
    }

    fn on_effect_stream(
        &mut self,
        name: &str,
        mut rx: roam::Rx<EffectData>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let stream_name = name.to_string();
        let state = self.state.clone();
        let term = self.term.clone();
        Box::pin(async move {
            if stream_name == "image_download" {
                while let Ok(Some(data)) = rx.recv().await {
                    if let EffectData::Progress { current, total } = data
                        && total > 0
                    {
                        let pct = (current as f64 / total as f64 * 100.0) as u64;
                        eprint!("\r  downloading... {pct}%");
                    }
                }
                eprintln!(); // newline after progress
            } else if stream_name.starts_with("script:") {
                let mut log_lines: VecDeque<String> = VecDeque::new();
                let width = (term.size().1 as usize).max(1);

                while let Ok(Some(data)) = rx.recv().await {
                    if let EffectData::LogLine(line) = data {
                        if log_lines.len() >= MAX_LOG_LINES {
                            log_lines.pop_front();
                        }
                        log_lines.push_back(line);

                        // Erase previously drawn lines
                        let mut st = state.lock().unwrap();
                        if st.drawn_log_lines > 0 {
                            term.move_cursor_up(st.drawn_log_lines).ok();
                            term.clear_to_end_of_screen().ok();
                        }

                        // Redraw log lines
                        for l in &log_lines {
                            let display = format!("        {l}");
                            term.write_line(&truncate_str(&display, width, "\u{2026}"))
                                .ok();
                        }
                        st.drawn_log_lines = log_lines.len();
                    }
                }

                // Stream closed — clear log area so the transition handler
                // can print the completion checkmark cleanly.
                let mut st = state.lock().unwrap();
                if st.drawn_log_lines > 0 {
                    term.move_cursor_up(st.drawn_log_lines).ok();
                    term.clear_to_end_of_screen().ok();
                    st.drawn_log_lines = 0;
                }
            }
            // Unknown streams: silently drain
        })
    }

    fn clone_for_stream(&self) -> Box<dyn Observer> {
        Box::new(self.clone())
    }
}
