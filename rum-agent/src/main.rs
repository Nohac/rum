use tokio::signal::unix::{SignalKind, signal};
use tokio_vsock::{VsockAddr, VsockListener, VMADDR_CID_ANY};

use roam_stream::{HandshakeConfig, accept};
use rum_agent::{RumAgent, RumAgentDispatcher};

const VSOCK_PORT: u32 = 2222;

#[derive(Clone)]
struct RumAgentImpl;

impl RumAgent for RumAgentImpl {
    async fn ping(
        &self,
        _cx: &roam::Context,
    ) -> Result<rum_agent::ReadyResponse, String> {
        let hostname = std::fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "unknown".into())
            .trim()
            .to_string();

        Ok(rum_agent::ReadyResponse {
            version: env!("CARGO_PKG_VERSION").into(),
            hostname,
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    eprintln!("rum-agent v{} starting", env!("CARGO_PKG_VERSION"));

    let listener = VsockListener::bind(VsockAddr::new(VMADDR_CID_ANY, VSOCK_PORT))
        .expect("failed to bind vsock listener");

    eprintln!("rum-agent: listening on vsock port {VSOCK_PORT}");

    let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        eprintln!("rum-agent: accepted connection from {addr:?}");
                        let dispatcher = RumAgentDispatcher::new(RumAgentImpl);
                        tokio::spawn(async move {
                            match accept(stream, HandshakeConfig::default(), dispatcher).await {
                                Ok((_handle, _incoming, driver)) => {
                                    if let Err(e) = driver.run().await {
                                        eprintln!("rum-agent: driver error: {e}");
                                    }
                                }
                                Err(e) => eprintln!("rum-agent: handshake failed: {e}"),
                            }
                        });
                    }
                    Err(e) => eprintln!("rum-agent: accept error: {e}"),
                }
            }
            _ = sigterm.recv() => {
                eprintln!("rum-agent: received SIGTERM, shutting down");
                break;
            }
            _ = sigint.recv() => {
                eprintln!("rum-agent: received SIGINT, shutting down");
                break;
            }
        }
    }
}
