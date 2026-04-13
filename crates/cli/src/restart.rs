use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ecsdk::bevy_replicon::shared::protocol::ProtocolMismatch;
use ecsdk::prelude::*;

/// Shared flag toggled when the client wants the daemon to be restarted after
/// a protocol mismatch.
#[derive(Resource, Clone)]
pub struct RestartRequested(pub Arc<AtomicBool>);

/// Install client-side protocol mismatch handling.
pub struct ProtocolRestartPlugin {
    requested: Arc<AtomicBool>,
}

impl ProtocolRestartPlugin {
    pub fn new(requested: Arc<AtomicBool>) -> Self {
        Self { requested }
    }
}

impl Plugin for ProtocolRestartPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(RestartRequested(self.requested.clone()));
        app.add_observer(on_protocol_mismatch);
    }
}

fn on_protocol_mismatch(
    _trigger: On<ProtocolMismatch>,
    requested: Res<RestartRequested>,
    mut exit: MessageWriter<AppExit>,
) {
    eprintln!("Daemon version differs from client. Restart daemon to update? [y/N]");
    eprint!("> ");
    let _ = io::stderr().flush();

    let mut input = String::new();
    let restart = io::stdin()
        .read_line(&mut input)
        .map(|_| matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
        .unwrap_or(false);

    if restart {
        requested.0.store(true, Ordering::SeqCst);
    }

    exit.write(AppExit::Success);
}
