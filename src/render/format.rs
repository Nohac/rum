use console::style;

use crate::phase::VmPhase;

pub const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn active_phase_label(phase: VmPhase) -> Option<&'static str> {
    match phase {
        VmPhase::DownloadingImage => Some("Downloading image"),
        VmPhase::Preparing => Some("Preparing VM"),
        VmPhase::Booting => Some("Booting VM"),
        VmPhase::ConnectingAgent => Some("Connecting agent"),
        VmPhase::Provisioning => Some("Provisioning"),
        VmPhase::StartingServices => Some("Starting services"),
        VmPhase::ShuttingDown => Some("Shutting down"),
        VmPhase::Destroying => Some("Destroying"),
        _ => None,
    }
}

pub fn completed_phase_label(phase: VmPhase) -> Option<&'static str> {
    match phase {
        VmPhase::DownloadingImage => Some("Downloaded image"),
        VmPhase::Preparing => Some("Prepared VM"),
        VmPhase::Booting => Some("Booted VM"),
        VmPhase::ConnectingAgent => Some("Connected agent"),
        VmPhase::Provisioning => Some("Provisioned"),
        VmPhase::StartingServices => Some("Started services"),
        VmPhase::ShuttingDown => Some("Shut down"),
        VmPhase::Destroying => Some("Destroyed"),
        VmPhase::Running => Some("Ready"),
        VmPhase::Stopped => Some("Shut down"),
        VmPhase::Destroyed => Some("Destroyed"),
        _ => None,
    }
}

pub fn spinner_line(frame: usize, label: &str) -> String {
    let spinner = style(SPINNER_FRAMES[frame % SPINNER_FRAMES.len()]).cyan();
    format!("{spinner} {label}")
}

pub fn completed_line(label: &str) -> String {
    format!("✓ {}", style(label).green())
}

pub fn failed_line(message: &str) -> String {
    format!("✗ {}", style(message).red())
}
