use std::sync::LazyLock;

use roam_stream::{Connector, HandshakeConfig, NoDispatcher, connect};
use rum_agent::RumAgentClient;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_vsock::{VsockAddr, VsockStream};

use crate::config::PortForward;
use crate::error::RumError;

/// Raw rum-agent binary, embedded at compile time via artifact dependency.
const AGENT_BINARY_RAW: &[u8] = include_bytes!(env!("CARGO_BIN_FILE_RUM_AGENT"));

/// Patched rum-agent binary with standard Linux interpreter/rpath.
/// NixOS builds have `/nix/store/...` paths that don't exist on Ubuntu etc.
/// arwen rewrites these to standard paths; no-op if already standard.
pub static AGENT_BINARY: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let mut writer = arwen::elf::Writer::read(AGENT_BINARY_RAW)
        .expect("rum-agent should be a valid ELF binary");
    writer
        .elf_set_interpreter(b"/lib64/ld-linux-x86-64.so.2".to_vec())
        .expect("failed to set interpreter");
    writer
        .elf_set_runpath(b"/lib/x86_64-linux-gnu:/lib64:/usr/lib".to_vec())
        .expect("failed to set rpath");
    let mut out = Vec::new();
    writer.write(&mut out).expect("failed to write patched ELF");
    out
});

pub const AGENT_SERVICE: &str = "\
[Unit]
Description=rum guest agent
After=local-fs.target

[Service]
Type=simple
ExecStart=/usr/local/bin/rum-agent
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
";

const RPC_PORT: u32 = 2222;
const FORWARD_PORT: u32 = 2223;
const AGENT_TIMEOUT_SECS: u64 = 120;
const AGENT_RETRY_INTERVAL_MS: u64 = 500;

struct VsockConnector {
    cid: u32,
}

impl Connector for VsockConnector {
    type Transport = VsockStream;

    async fn connect(&self) -> std::io::Result<VsockStream> {
        VsockStream::connect(VsockAddr::new(self.cid, RPC_PORT)).await
    }
}

/// Wait for the rum-agent in the guest to respond to a ping over vsock.
/// Retries until the agent is ready or the timeout expires.
pub async fn wait_for_agent(cid: u32) -> Result<(), RumError> {
    let connector = VsockConnector { cid };
    let client = connect(connector, HandshakeConfig::default(), NoDispatcher);
    let service = RumAgentClient::new(client);

    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(AGENT_TIMEOUT_SECS);

    loop {
        match service.ping().await {
            Ok(resp) => {
                tracing::info!(
                    version = %resp.version,
                    hostname = %resp.hostname,
                    "agent ready"
                );
                return Ok(());
            }
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(AGENT_RETRY_INTERVAL_MS)).await;
            }
            Err(e) => {
                return Err(RumError::AgentTimeout {
                    message: format!("agent did not respond within {AGENT_TIMEOUT_SECS}s: {e}"),
                });
            }
        }
    }
}

/// Start TCP listeners on the host that forward connections to the guest via vsock.
///
/// Each `PortForward` binds a TCP listener on `bind:host_port`. When a connection
/// arrives, it opens a vsock stream to `cid:2223`, sends the 2-byte big-endian
/// guest port, then proxies bytes bidirectionally.
///
/// Returns handles that can be aborted on shutdown.
pub async fn start_port_forwards(
    cid: u32,
    ports: &[PortForward],
) -> Result<Vec<JoinHandle<()>>, RumError> {
    let mut handles = Vec::new();

    for pf in ports {
        let bind_addr = format!("{}:{}", pf.bind_addr(), pf.host);
        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| RumError::Io {
            context: format!("binding port forward on {bind_addr}"),
            source: e,
        })?;

        let guest_port = pf.guest;
        let host_port = pf.host;

        let handle = tokio::spawn(async move {
            loop {
                let (tcp_stream, _addr) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!(port = host_port, "forward accept error: {e}");
                        continue;
                    }
                };

                let cid = cid;
                tokio::spawn(async move {
                    if let Err(e) =
                        proxy_connection(cid, guest_port, tcp_stream).await
                    {
                        tracing::error!(
                            host_port,
                            guest_port,
                            "forward proxy error: {e}"
                        );
                    }
                });
            }
        });

        handles.push(handle);
    }

    Ok(handles)
}

async fn proxy_connection(
    cid: u32,
    guest_port: u16,
    mut tcp: tokio::net::TcpStream,
) -> Result<(), std::io::Error> {
    let mut vsock = VsockStream::connect(VsockAddr::new(cid, FORWARD_PORT)).await?;

    // Send 2-byte big-endian target port header
    vsock.write_u16(guest_port).await?;

    tokio::io::copy_bidirectional(&mut tcp, &mut vsock).await?;
    Ok(())
}
