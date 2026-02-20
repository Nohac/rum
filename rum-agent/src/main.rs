mod log_layer;

use std::time::{SystemTime, UNIX_EPOCH};

use roam::Tx;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::broadcast;
use tokio_vsock::{VMADDR_CID_ANY, VsockAddr, VsockListener};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use roam_stream::{HandshakeConfig, accept};
use rum_agent::{
    ExecResult, LogEvent, LogLevel, LogStream, RumAgent, RumAgentDispatcher,
};

const RPC_PORT: u32 = 2222;
const FORWARD_PORT: u32 = 2223;

#[derive(Clone)]
struct RumAgentImpl {
    log_tx: broadcast::Sender<LogEvent>,
}

impl RumAgent for RumAgentImpl {
    async fn ping(&self, _cx: &roam::Context) -> Result<rum_agent::ReadyResponse, String> {
        let hostname = std::fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "unknown".into())
            .trim()
            .to_string();

        Ok(rum_agent::ReadyResponse {
            version: env!("CARGO_PKG_VERSION").into(),
            hostname,
        })
    }

    async fn subscribe_logs(&self, _cx: &roam::Context, output: Tx<LogEvent>) {
        let mut rx = self.log_tx.subscribe();
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if output.send(&event).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "log subscriber lagged");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn exec(
        &self,
        _cx: &roam::Context,
        command: String,
        output: Tx<LogEvent>,
    ) -> ExecResult {
        tracing::info!(command, "exec");

        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();

        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = output
                    .send(&LogEvent {
                        timestamp_us: now_us(),
                        level: LogLevel::Error,
                        target: "exec".into(),
                        message: format!("failed to spawn: {e}"),
                        stream: LogStream::Stderr,
                    })
                    .await;
                return ExecResult { exit_code: None };
            }
        };

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let mut stdout_lines = BufReader::new(stdout).lines();
        let mut stderr_lines = BufReader::new(stderr).lines();

        loop {
            tokio::select! {
                line = stdout_lines.next_line() => {
                    match line {
                        Ok(Some(text)) => {
                            let _ = output.send(&LogEvent {
                                timestamp_us: now_us(),
                                level: LogLevel::Info,
                                target: "exec".into(),
                                message: text,
                                stream: LogStream::Stdout,
                            }).await;
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
                line = stderr_lines.next_line() => {
                    match line {
                        Ok(Some(text)) => {
                            let _ = output.send(&LogEvent {
                                timestamp_us: now_us(),
                                level: LogLevel::Warn,
                                target: "exec".into(),
                                message: text,
                                stream: LogStream::Stderr,
                            }).await;
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
        }

        let status = child.wait().await.ok();
        ExecResult {
            exit_code: status.and_then(|s| s.code()),
        }
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
            tracing::error!(error = %e, "forward: failed to read target port");
            return;
        }
    };

    let mut tcp = match TcpStream::connect(("127.0.0.1", target_port)).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(port = target_port, error = %e, "forward: failed to connect");
            return;
        }
    };

    if let Err(e) = tokio::io::copy_bidirectional(&mut vsock, &mut tcp).await {
        tracing::debug!(port = target_port, error = %e, "forward: proxy error");
    }
}

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (broadcast_layer, log_tx) = log_layer::log_broadcast_layer();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(broadcast_layer)
        .init();

    let version = env!("CARGO_PKG_VERSION");
    tracing::info!(version, "rum-agent starting");

    let rpc_listener = VsockListener::bind(VsockAddr::new(VMADDR_CID_ANY, RPC_PORT))
        .expect("failed to bind vsock RPC listener");
    let fwd_listener = VsockListener::bind(VsockAddr::new(VMADDR_CID_ANY, FORWARD_PORT))
        .expect("failed to bind vsock forward listener");

    tracing::info!(rpc_port = RPC_PORT, fwd_port = FORWARD_PORT, "listening");

    let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");

    let agent = RumAgentImpl { log_tx };

    loop {
        tokio::select! {
            result = rpc_listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        tracing::info!(?addr, "RPC connection");
                        let dispatcher = RumAgentDispatcher::new(agent.clone());
                        tokio::spawn(async move {
                            match accept(stream, HandshakeConfig::default(), dispatcher).await {
                                Ok((_handle, _incoming, driver)) => {
                                    if let Err(e) = driver.run().await {
                                        tracing::error!(error = %e, "driver error");
                                    }
                                }
                                Err(e) => tracing::error!(error = %e, "handshake failed"),
                            }
                        });
                    }
                    Err(e) => tracing::error!(error = %e, "RPC accept error"),
                }
            }
            result = fwd_listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        tracing::debug!(?addr, "forward connection");
                        tokio::spawn(handle_forward(stream));
                    }
                    Err(e) => tracing::error!(error = %e, "forward accept error"),
                }
            }
            _ = sigterm.recv() => {
                tracing::info!("received SIGTERM, shutting down");
                break;
            }
            _ = sigint.recv() => {
                tracing::info!("received SIGINT, shutting down");
                break;
            }
        }
    }
}
