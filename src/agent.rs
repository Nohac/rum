use roam_stream::{Client, Connector, HandshakeConfig, NoDispatcher, connect};
use rum_agent::{LogEvent, LogLevel, LogStream, RumAgentClient};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_vsock::{VsockAddr, VsockStream};

use crate::config::PortForward;
use crate::error::RumError;

/// Static musl rum-agent binary, embedded at compile time via artifact dependency.
/// No glibc dependency — runs on any Linux guest.
pub const AGENT_BINARY: &[u8] = include_bytes!(env!("CARGO_BIN_FILE_RUM_AGENT"));

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

pub(crate) struct VsockConnector {
    cid: u32,
}

impl Connector for VsockConnector {
    type Transport = VsockStream;

    async fn connect(&self) -> std::io::Result<VsockStream> {
        VsockStream::connect(VsockAddr::new(self.cid, RPC_PORT)).await
    }
}

/// Type alias for the agent RPC client over vsock.
pub(crate) type AgentClient = RumAgentClient<Client<VsockConnector, NoDispatcher>>;

/// Wait for the rum-agent in the guest to respond to a ping over vsock.
/// Retries until the agent is ready or the timeout expires.
/// Returns the connected client for further RPC calls.
pub(crate) async fn wait_for_agent(cid: u32) -> Result<AgentClient, RumError> {
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
                return Ok(service);
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

/// Start a background task that subscribes to the agent's log stream
/// and prints events to the host console.
pub(crate) fn start_log_subscription(agent: &AgentClient) -> JoinHandle<()> {
    let (tx, mut rx) = roam::channel::<LogEvent>();
    let agent = agent.clone();

    tokio::spawn(async move {
        // Fire-and-forget: subscribe_logs blocks on the agent side until disconnect
        let subscribe_fut = agent.subscribe_logs(tx);

        tokio::spawn(async move {
            let _ = subscribe_fut.await;
        });

        while let Ok(Some(event)) = rx.recv().await {
            let stream = match event.stream {
                LogStream::Log => "log",
                LogStream::Stdout => "stdout",
                LogStream::Stderr => "stderr",
            };
            match event.level {
                LogLevel::Trace => tracing::trace!(target: "guest", stream, "{}", event.message),
                LogLevel::Debug => tracing::debug!(target: "guest", stream, "{}", event.message),
                LogLevel::Info => tracing::info!(target: "guest", stream, "{}", event.message),
                LogLevel::Warn => tracing::warn!(target: "guest", stream, "{}", event.message),
                LogLevel::Error => tracing::error!(target: "guest", stream, "{}", event.message),
            }
        }
    })
}

/// Run provisioning scripts on the guest via the agent RPC.
///
/// Streams log output from the agent via the `on_log` callback and returns
/// an error if any script fails.
pub(crate) async fn run_provision(
    agent: &AgentClient,
    scripts: Vec<rum_agent::ProvisionScript>,
    mut on_log: impl FnMut(&str),
) -> Result<(), RumError> {
    let (tx, mut rx) = roam::channel::<LogEvent>();
    let agent = agent.clone();

    let provision_handle = tokio::spawn(async move { agent.provision(scripts, tx).await });

    while let Ok(Some(event)) = rx.recv().await {
        on_log(&event.message);
    }

    let result = provision_handle
        .await
        .map_err(|e| RumError::Io {
            context: format!("provision task panicked: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?
        .map_err(|e| RumError::Io {
            context: format!("provision RPC failed: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?;

    if !result.success {
        return Err(RumError::ProvisionFailed {
            script: result.failed_script,
        });
    }

    Ok(())
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
