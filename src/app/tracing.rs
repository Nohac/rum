use std::path::Path;

use ecsdk_core::WakeSignal;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::logging;
use crate::progress::OutputMode;

pub struct DaemonTracing {
    pub tracing_receiver: ecsdk_tracing::TracingReceiver,
}

pub fn init_tracing(mode: OutputMode) {
    let terminal_filter = match mode {
        OutputMode::Verbose => EnvFilter::new("debug"),
        OutputMode::Normal | OutputMode::Quiet | OutputMode::Silent => EnvFilter::new("off"),
        OutputMode::Plain => EnvFilter::from_default_env()
            .add_directive("rum=debug".parse().expect("valid log directive")),
    };

    let terminal_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(terminal_filter);

    let workspace_client_log = std::env::current_dir()
        .ok()
        .map(|dir| dir.join("rum-client.log"));

    let (client_writer, client_handle) = logging::DeferredFileWriter::new();
    if let Some(path) = workspace_client_log.as_deref() {
        client_handle
            .set_file(path)
            .expect("workspace client log should be writable");
    }

    let client_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(client_writer)
        .with_filter(EnvFilter::new("rum=debug"));

    tracing_subscriber::registry()
        .with(terminal_layer)
        .with(client_layer)
        .init();
}

pub fn init_daemon_tracing(
    wake: WakeSignal,
    workspace_daemon_log: Option<&Path>,
) -> DaemonTracing {
    let (tracing_layer, tracing_receiver) = ecsdk_tracing::setup(wake);

    let (daemon_writer, daemon_handle) = logging::DeferredFileWriter::new();
    if let Some(path) = workspace_daemon_log {
        daemon_handle
            .set_file(path)
            .expect("workspace daemon log should be writable");
    }

    let daemon_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(daemon_writer)
        .with_filter(EnvFilter::new("rum=debug"));

    tracing_subscriber::registry()
        .with(
            tracing_layer.with_filter(
                tracing_subscriber::filter::Targets::new()
                    .with_target("rum", tracing::Level::DEBUG),
            ),
        )
        .with(daemon_layer)
        .init();

    DaemonTracing { tracing_receiver }
}
