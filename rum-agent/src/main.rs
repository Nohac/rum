use tokio::signal::unix::{signal, SignalKind};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    eprintln!("rum-agent v{} starting", env!("CARGO_PKG_VERSION"));

    // Wait for SIGTERM or SIGINT
    let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");

    tokio::select! {
        _ = sigterm.recv() => eprintln!("rum-agent: received SIGTERM, shutting down"),
        _ = sigint.recv() => eprintln!("rum-agent: received SIGINT, shutting down"),
    }
}
