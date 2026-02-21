use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use console::{style, truncate_str, Term};

/// Controls how step output is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Spinners + ring-buffer logs, clear logs on completion.
    Normal,
    /// Like Normal but keeps log lines after step completion.
    Verbose,
    /// Spinners only, no log lines.
    Quiet,
    /// No ANSI — plain println output (for piped/non-TTY).
    Plain,
}

const MAX_LOG_LINES: usize = 10;
const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Orchestrates numbered build steps with spinners and checkmarks.
pub struct StepProgress {
    term: Term,
    total_steps: usize,
    current_step: usize,
    mode: OutputMode,
}

/// Shared state for the active area (spinner line + log lines).
struct StepState {
    log_lines: VecDeque<String>,
    done_label: Option<String>,
    failed: bool,
    /// Number of logical lines currently in the active area.
    drawn_lines: usize,
    label: String,
    prefix: String,
    spinner_frame: usize,
}

/// Handle passed into the step closure for logging during execution.
///
/// Methods take `&self` — interior mutation goes through the shared `StepState`.
pub struct Step {
    state: Arc<Mutex<StepState>>,
    mode: OutputMode,
}

impl StepState {
    /// Redraw the entire active area (spinner line + log lines).
    ///
    /// Caller must hold the mutex. All terminal writes happen while locked
    /// so cursor movements are atomic with respect to the spinner tick task.
    fn redraw(&mut self, term: &Term, mode: OutputMode) {
        let width = (term.size().1 as usize).max(1);

        // Move to top of active area
        if self.drawn_lines > 0 {
            term.move_cursor_up(self.drawn_lines).ok();
        }

        // Spinner line — truncate to terminal width to prevent wrapping
        let ch = SPINNER_CHARS[self.spinner_frame % SPINNER_CHARS.len()];
        let spinner_line = format!(
            "[{}] {} {}",
            self.prefix,
            style(ch).cyan(),
            self.label
        );
        term.clear_line().ok();
        term.write_line(&truncate_str(&spinner_line, width, "\u{2026}"))
            .ok();

        // Log lines (skip in Quiet mode)
        let log_count = if mode != OutputMode::Quiet {
            for line in &self.log_lines {
                let log_line = format!("        {line}");
                term.clear_line().ok();
                term.write_line(&truncate_str(&log_line, width, "\u{2026}"))
                    .ok();
            }
            self.log_lines.len()
        } else {
            0
        };

        let new_drawn = 1 + log_count;

        // Clear any leftover rows below (from previous draws with more
        // log lines, or from wrapped lines after a terminal resize)
        term.clear_to_end_of_screen().ok();

        self.drawn_lines = new_drawn;
    }
}

impl StepProgress {
    pub fn new(total_steps: usize, mode: OutputMode) -> Self {
        Self {
            term: Term::stderr(),
            total_steps,
            current_step: 0,
            mode,
        }
    }

    /// Run an async task as a numbered step.
    ///
    /// Shows a spinner while running, checkmark on completion.
    /// The closure receives a [`Step`] handle for logging.
    pub async fn run<F, Fut, T>(&mut self, label: &str, f: F) -> T
    where
        F: FnOnce(Step) -> Fut,
        Fut: Future<Output = T>,
    {
        self.current_step += 1;
        let prefix = format!("{}/{}", self.current_step, self.total_steps);

        if self.mode == OutputMode::Plain {
            println!("[{prefix}] {label}");
        }

        let state = Arc::new(Mutex::new(StepState {
            log_lines: VecDeque::new(),
            done_label: None,
            failed: false,
            drawn_lines: 0,
            label: label.to_string(),
            prefix: prefix.clone(),
            spinner_frame: 0,
        }));

        // Draw initial spinner line for non-plain modes
        if self.mode != OutputMode::Plain {
            self.term.hide_cursor().ok();
            let mut st = state.lock().unwrap();
            st.redraw(&self.term, self.mode);
        }

        // Start spinner tick task — redraws on timer and on terminal resize
        let tick_handle = if self.mode != OutputMode::Plain {
            let state_clone = state.clone();
            let mode = self.mode;
            Some(tokio::spawn(async move {
                let term = Term::stderr();
                let mut interval = tokio::time::interval(Duration::from_millis(80));
                let mut sigwinch = tokio::signal::unix::signal(
                    tokio::signal::unix::SignalKind::window_change(),
                )
                .expect("failed to register SIGWINCH handler");
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let mut st = state_clone.lock().unwrap();
                            st.spinner_frame += 1;
                            st.redraw(&term, mode);
                        }
                        _ = sigwinch.recv() => {
                            // Immediate redraw at new terminal width
                            let mut st = state_clone.lock().unwrap();
                            st.redraw(&term, mode);
                        }
                    }
                }
            }))
        } else {
            None
        };

        let step = Step {
            state: state.clone(),
            mode: self.mode,
        };

        let result = f(step).await;

        // Stop spinner tick
        if let Some(handle) = tick_handle {
            handle.abort();
        }

        let st = state.lock().unwrap();
        let done_label = st
            .done_label
            .clone()
            .unwrap_or_else(|| label.to_string());
        let failed = st.failed;
        let log_lines: Vec<String> = st.log_lines.iter().cloned().collect();
        let drawn_lines = st.drawn_lines;
        drop(st);

        if self.mode != OutputMode::Plain {
            // Clear the active area
            if drawn_lines > 0 {
                self.term.move_cursor_up(drawn_lines).ok();
            }
            self.term.clear_to_end_of_screen().ok();

            // In Verbose mode, flush log lines as permanent output
            if self.mode == OutputMode::Verbose {
                for line in &log_lines {
                    self.term
                        .write_line(&format!("        {line}"))
                        .ok();
                }
            }

            // Print completed step — permanent, immune to resize
            if failed {
                self.term
                    .write_line(&format!(
                        "[{prefix}] \u{2717} {}",
                        style(&done_label).red()
                    ))
                    .ok();
            } else {
                self.term
                    .write_line(&format!(
                        "[{prefix}] \u{2713} {}",
                        style(&done_label).green()
                    ))
                    .ok();
            }

            self.term.show_cursor().ok();
        }

        if self.mode == OutputMode::Plain {
            if failed {
                println!("[{prefix}] \u{2717} {done_label}");
            } else {
                println!("[{prefix}] \u{2713} {done_label}");
            }
        }

        result
    }

    /// Instant completion — no task to run (cached/skipped items).
    pub fn skip(&mut self, label: &str) {
        self.current_step += 1;
        let prefix = format!("{}/{}", self.current_step, self.total_steps);

        if self.mode == OutputMode::Plain {
            println!("[{prefix}] \u{2014} {label}");
        } else {
            self.term
                .write_line(&format!(
                    "[{prefix}] \u{2014} {}",
                    style(label).blue()
                ))
                .ok();
        }
    }

    /// Print an info line (port forwards, etc.).
    pub fn info(&self, text: &str) {
        if self.mode == OutputMode::Plain {
            println!("      \u{2192} {text}");
        } else {
            self.term
                .write_line(&format!("      \u{2192} {text}"))
                .ok();
        }
    }

    /// Print a plain line (final messages).
    pub fn println(&self, text: &str) {
        if self.mode == OutputMode::Plain {
            println!("{text}");
        } else {
            self.term.write_line(text).ok();
        }
    }
}

impl Drop for StepProgress {
    fn drop(&mut self) {
        if self.mode != OutputMode::Plain {
            self.term.show_cursor().ok();
        }
    }
}

impl Step {
    /// Add a log line under this step (ring buffer of ~10).
    pub fn log(&self, line: &str) {
        if self.mode == OutputMode::Quiet {
            return;
        }

        if self.mode == OutputMode::Plain {
            for sub in line.split('\n') {
                println!("        {sub}");
            }
            return;
        }

        let term = Term::stderr();
        let mut st = self.state.lock().unwrap();

        for sub in line.split('\n') {
            if st.log_lines.len() >= MAX_LOG_LINES {
                st.log_lines.pop_front();
            }
            st.log_lines.push_back(sub.to_string());
        }

        st.redraw(&term, self.mode);
    }

    /// Override the completion label shown with the checkmark.
    pub fn set_done_label(&self, label: impl Into<String>) {
        self.state.lock().unwrap().done_label = Some(label.into());
    }

    /// Mark this step as failed — shows red ✗ instead of green ✓.
    pub fn set_failed(&self) {
        self.state.lock().unwrap().failed = true;
    }
}
