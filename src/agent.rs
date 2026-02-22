use std::path::Path;
use std::sync::Arc;

use roam_stream::{Client, Connector, HandshakeConfig, NoDispatcher, connect};
use rum_agent::{LogEvent, LogLevel, LogStream, ProvisionEvent, RumAgentClient};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_vsock::{VsockAddr, VsockStream};

use crate::config::PortForward;
use crate::error::RumError;
use crate::logging::ScriptLogger;
use crate::progress::StepProgress;

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

/// Run a command inside the VM via the agent's exec RPC.
///
/// Connects to the agent over vsock, streams stdout/stderr back to the host,
/// and returns the command's exit code.
pub async fn run_exec(cid: u32, command: String) -> Result<i32, RumError> {
    let agent = wait_for_agent(cid).await?;

    let (tx, mut rx) = roam::channel::<LogEvent>();

    let exec_task = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.exec(command, tx).await })
    };

    // Stream output from the agent to host stdout/stderr
    let stream_task = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        let mut stderr = tokio::io::stderr();
        while let Ok(Some(event)) = rx.recv().await {
            match event.stream {
                LogStream::Stdout => {
                    let _ = stdout.write_all(event.message.as_bytes()).await;
                    let _ = stdout.write_all(b"\n").await;
                    let _ = stdout.flush().await;
                }
                LogStream::Stderr => {
                    let _ = stderr.write_all(event.message.as_bytes()).await;
                    let _ = stderr.write_all(b"\n").await;
                    let _ = stderr.flush().await;
                }
                LogStream::Log => {
                    // Print agent log lines to stderr for visibility
                    let _ = stderr.write_all(event.message.as_bytes()).await;
                    let _ = stderr.write_all(b"\n").await;
                    let _ = stderr.flush().await;
                }
            }
        }
    });

    let result = exec_task
        .await
        .map_err(|e| RumError::Io {
            context: format!("exec task panicked: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?
        .map_err(|e| RumError::Io {
            context: format!("exec RPC failed: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?;

    // Wait for the stream task to finish draining
    let _ = stream_task.await;

    Ok(result.exit_code.unwrap_or(1))
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

/// Run provisioning scripts on the guest via a single agent RPC call.
///
/// Creates one progress step per script. Each step reads `ProvisionEvent`s
/// from the shared channel until it receives `Done`.
pub(crate) async fn run_provision(
    agent: &AgentClient,
    scripts: Vec<rum_agent::ProvisionScript>,
    progress: &mut StepProgress,
    logs_dir: &Path,
) -> Result<(), RumError> {
    let script_names: Vec<String> = scripts.iter().map(|s| s.name.clone()).collect();
    let titles: Vec<String> = scripts.iter().map(|s| s.title.clone()).collect();

    let (tx, rx) = roam::channel::<ProvisionEvent>();
    let agent = agent.clone();
    let task = tokio::spawn(async move { agent.provision(scripts, tx).await });

    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let mut failed = false;

    for (i, title) in titles.iter().enumerate() {
        let rx = rx.clone();
        let title_owned = title.clone();
        let mut logger = ScriptLogger::new(logs_dir, &script_names[i]).ok();
        let success = progress
            .run(title, |step| async move {
                let mut rx = rx.lock().await;
                while let Ok(Some(event)) = rx.recv().await {
                    match event {
                        ProvisionEvent::Done(code) => {
                            if let Some(lg) = logger.take() {
                                lg.finish(code == 0);
                            }
                            if code != 0 {
                                step.set_failed();
                                step.set_done_label(format!(
                                    "{title_owned} (exit code {code})"
                                ));
                                return false;
                            }
                            return true;
                        }
                        ProvisionEvent::Stdout(ref line)
                        | ProvisionEvent::Stderr(ref line) => {
                            if let Some(ref mut lg) = logger {
                                lg.write_line(line);
                            }
                            step.log(line);
                        }
                    }
                }
                // Channel closed without Done
                if let Some(lg) = logger.take() {
                    lg.finish(false);
                }
                step.set_failed();
                step.set_done_label(format!("{title_owned} (connection lost)"));
                false
            })
            .await;

        if !success {
            for remaining in &titles[i + 1..] {
                progress.skip(&format!("{remaining} (skipped)"));
            }
            failed = true;
            break;
        }
    }

    // Rotate logs for each script name
    for name in &script_names {
        crate::logging::rotate_logs(logs_dir, name, 10);
    }

    let result = task
        .await
        .map_err(|e| RumError::Io {
            context: format!("provision task panicked: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?
        .map_err(|e| RumError::Io {
            context: format!("provision RPC failed: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?;

    if failed || !result.success {
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
