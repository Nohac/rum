use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

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

/// Orchestrates numbered build steps with spinners and checkmarks.
pub struct StepProgress {
    multi: MultiProgress,
    total_steps: usize,
    current_step: usize,
    mode: OutputMode,
}

/// Shared state between `Step` and `StepProgress::run()`.
///
/// The closure may drop `Step` before the future completes (when the async
/// block doesn't capture it). `run()` keeps its own `Arc` clone so it can
/// finalize the bar and clean up log lines regardless.
struct StepState {
    log_lines: VecDeque<String>,
    done_label: Option<String>,
}

/// Handle passed into the step closure for logging during execution.
///
/// Methods take `&self` — interior mutation goes through the shared `StepState`.
///
/// Log lines are encoded as extra lines in the spinner bar's message
/// (multi-line `ProgressBar`). This avoids adding/removing separate bars
/// from the `MultiProgress`, which can cause indicatif to miscount terminal
/// lines and clear too much on redraw.
pub struct Step {
    bar: ProgressBar,
    multi: MultiProgress,
    state: Arc<Mutex<StepState>>,
    label: String,
    mode: OutputMode,
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::default_spinner()
        .template("[{prefix}] {spinner:.cyan} {msg}")
        .unwrap()
}

fn done_style() -> ProgressStyle {
    ProgressStyle::default_spinner()
        .template("[{prefix}] \u{2713} {msg:.green}")
        .unwrap()
}

const MAX_LOG_LINES: usize = 10;

impl StepProgress {
    pub fn new(total_steps: usize, mode: OutputMode) -> Self {
        let multi = if mode == OutputMode::Plain {
            MultiProgress::with_draw_target(ProgressDrawTarget::hidden())
        } else {
            MultiProgress::new()
        };
        Self {
            multi,
            total_steps,
            current_step: 0,
            mode,
        }
    }

    /// Run an async task as a numbered step.
    ///
    /// Shows a spinner while running, checkmark on completion.
    /// The closure receives a [`Step`] handle for logging.
    /// Finalization (checkmark, log cleanup) happens here — **not** in
    /// `Step::drop` — so steps that don't capture the handle in their
    /// async block still get the correct spinner→checkmark transition.
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

        let bar = self.multi.add(ProgressBar::new_spinner());
        bar.set_style(spinner_style());
        bar.set_prefix(prefix.clone());
        bar.set_message(label.to_string());
        bar.enable_steady_tick(std::time::Duration::from_millis(80));

        let state = Arc::new(Mutex::new(StepState {
            log_lines: VecDeque::new(),
            done_label: None,
        }));

        let step = Step {
            bar: bar.clone(),
            multi: self.multi.clone(),
            state: state.clone(),
            label: label.to_string(),
            mode: self.mode,
        };

        let result = f(step).await;

        // Finalize — Step may already have been dropped, but we still own
        // the bar and state through our Arc clone.
        let st = state.lock().unwrap();

        // In Verbose mode, flush log lines above the managed area so they
        // persist after the bar shrinks back to a single line.
        if self.mode == OutputMode::Verbose {
            for line in &st.log_lines {
                self.multi.println(format!("        {line}")).ok();
            }
        }

        let done_label = st
            .done_label
            .clone()
            .unwrap_or_else(|| label.to_string());
        drop(st);

        if self.mode == OutputMode::Plain {
            println!("[{prefix}] \u{2713} {done_label}");
        }

        // Setting the message to just the done_label collapses the bar from
        // N+1 lines (label + log lines) back to 1 line. indicatif handles
        // the terminal line delta internally — no multi.remove() needed.
        bar.set_style(done_style());
        bar.finish_with_message(done_label);

        result
    }

    /// Instant completion — no task to run (cached/skipped items).
    pub fn skip(&mut self, label: &str) {
        self.current_step += 1;
        let prefix = format!("{}/{}", self.current_step, self.total_steps);

        if self.mode == OutputMode::Plain {
            println!("[{prefix}] \u{2713} {label}");
            return;
        }

        let bar = self.multi.add(ProgressBar::new_spinner());
        bar.set_style(done_style());
        bar.set_prefix(prefix);
        bar.set_message(label.to_string());
        bar.finish();
    }

    /// Print an info line (port forwards, etc.).
    pub fn info(&self, text: &str) {
        if self.mode == OutputMode::Plain {
            println!("      \u{2192} {text}");
        } else {
            self.multi
                .println(format!("      \u{2192} {text}"))
                .ok();
        }
    }

    /// Print a plain line via multi.println (final messages).
    pub fn println(&self, text: &str) {
        if self.mode == OutputMode::Plain {
            println!("{text}");
        } else {
            self.multi.println(text).ok();
        }
    }
}

impl Step {
    /// Add a log line under this step (ring buffer of ~10).
    ///
    /// The line is appended to the spinner bar's message as an extra line,
    /// keeping the label as the first line. Old lines are evicted when the
    /// buffer is full. On step completion, `run()` sets the message back to
    /// just the done label, collapsing all log lines.
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

        let mut state = self.state.lock().unwrap();

        // Split on newlines so each visual line is one ring-buffer entry.
        // This keeps the line count accurate for indicatif's terminal tracking.
        for sub in line.split('\n') {
            if state.log_lines.len() >= MAX_LOG_LINES {
                state.log_lines.pop_front();
            }
            state.log_lines.push_back(sub.to_string());
        }

        // Rebuild the bar's message: label on the first line, then indented
        // log lines. indicatif tracks the line count per bar and handles the
        // terminal delta when the count changes.
        let mut msg = self.label.clone();
        for log_line in &state.log_lines {
            msg.push_str("\n        ");
            msg.push_str(log_line);
        }
        self.bar.set_message(msg);
    }

    /// Override the completion label shown with the checkmark.
    pub fn set_done_label(&self, label: impl Into<String>) {
        self.state.lock().unwrap().done_label = Some(label.into());
    }

    /// Access MultiProgress for adding child bars (e.g., download progress).
    pub fn multi(&self) -> &MultiProgress {
        &self.multi
    }
}
