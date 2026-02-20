use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::signal::unix::{SignalKind, signal};
use tokio_vsock::{VsockAddr, VsockListener, VMADDR_CID_ANY};

use roam_stream::{HandshakeConfig, accept};
use rum_agent::{RumAgent, RumAgentDispatcher};

const RPC_PORT: u32 = 2222;
const FORWARD_PORT: u32 = 2223;

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

/// Handle a single port-forwarding connection over vsock.
///
/// Protocol: the first 2 bytes are a big-endian u16 target port.
/// After that, bidirectional byte proxying to 127.0.0.1:port.
async fn handle_forward(mut vsock: tokio_vsock::VsockStream) {
    let target_port = match vsock.read_u16().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rum-agent: forward: failed to read target port: {e}");
            return;
        }
    };

    let mut tcp = match TcpStream::connect(("127.0.0.1", target_port)).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rum-agent: forward: failed to connect to 127.0.0.1:{target_port}: {e}");
            return;
        }
    };

    match tokio::io::copy_bidirectional(&mut vsock, &mut tcp).await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("rum-agent: forward: proxy error for port {target_port}: {e}");
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    eprintln!("rum-agent v{} starting", env!("CARGO_PKG_VERSION"));

    let rpc_listener = VsockListener::bind(VsockAddr::new(VMADDR_CID_ANY, RPC_PORT))
        .expect("failed to bind vsock RPC listener");
    let fwd_listener = VsockListener::bind(VsockAddr::new(VMADDR_CID_ANY, FORWARD_PORT))
        .expect("failed to bind vsock forward listener");

    eprintln!("rum-agent: listening on vsock ports {RPC_PORT} (RPC), {FORWARD_PORT} (forward)");

    let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");

    loop {
        tokio::select! {
            result = rpc_listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        eprintln!("rum-agent: RPC connection from {addr:?}");
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
                    Err(e) => eprintln!("rum-agent: RPC accept error: {e}"),
                }
            }
            result = fwd_listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        eprintln!("rum-agent: forward connection from {addr:?}");
                        tokio::spawn(handle_forward(stream));
                    }
                    Err(e) => eprintln!("rum-agent: forward accept error: {e}"),
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
