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
    ExecResult, LogEvent, LogLevel, LogStream, ProvisionEvent, ProvisionResult, ProvisionScript,
    RunOn, RumAgent, RumAgentDispatcher,
};

use std::path::Path;

const RPC_PORT: u32 = 2222;
const FORWARD_PORT: u32 = 2223;
const SCRIPTS_DIR: &str = "/var/lib/rum/scripts";
const SENTINEL_PATH: &str = "/var/lib/rum/.system-provisioned";

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
        run_script(&command, "exec", &output).await
    }

    async fn provision(
        &self,
        _cx: &roam::Context,
        scripts: Vec<ProvisionScript>,
        output: Tx<ProvisionEvent>,
    ) -> ProvisionResult {
        tracing::info!(count = scripts.len(), "provision");

        // Create scripts dir, clear old scripts
        let scripts_dir = Path::new(SCRIPTS_DIR);
        if let Err(e) = tokio::fs::create_dir_all(scripts_dir).await {
            tracing::error!(error = %e, "failed to create scripts dir");
            return ProvisionResult {
                success: false,
                failed_script: "(setup)".into(),
            };
        }
        if let Ok(mut entries) = tokio::fs::read_dir(scripts_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }

        // Write all scripts to disk
        for s in &scripts {
            let suffix = match s.run_on {
                RunOn::System => "system",
                RunOn::Boot => "boot",
            };
            let filename = format!("{:03}-{}.{suffix}.sh", s.order, s.name);
            let path = scripts_dir.join(&filename);
            if let Err(e) = tokio::fs::write(&path, &s.content).await {
                tracing::error!(error = %e, filename, "failed to write script");
                return ProvisionResult {
                    success: false,
                    failed_script: s.name.clone(),
                };
            }
        }

        // Run all received scripts in order — the host controls what to send
        let mut sorted: Vec<&ProvisionScript> = scripts.iter().collect();
        sorted.sort_by_key(|s| s.order);

        for s in &sorted {
            tracing::info!(script = %s.name, "running provision script");

            let exit_code = run_provision_script(&s.content, &output)
                .await
                .unwrap_or(-1);
            let _ = output.send(&ProvisionEvent::Done(exit_code)).await;

            if exit_code != 0 {
                tracing::error!(script = %s.name, exit_code, "script failed");
                return ProvisionResult {
                    success: false,
                    failed_script: s.name.clone(),
                };
            }
        }

        // Create sentinel on success so auto-boot scripts know system was provisioned
        if let Some(parent) = Path::new(SENTINEL_PATH).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Err(e) = tokio::fs::write(SENTINEL_PATH, "").await {
            tracing::error!(error = %e, "failed to create sentinel");
        }

        ProvisionResult {
            success: true,
            failed_script: String::new(),
        }
    }
}

async fn run_script(content: &str, name: &str, output: &Tx<LogEvent>) -> ExecResult {
    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(content)
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
                    target: name.into(),
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
                            target: name.into(),
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
                            target: name.into(),
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

async fn run_provision_script(content: &str, output: &Tx<ProvisionEvent>) -> Option<i32> {
    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(content)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            let _ = output
                .send(&ProvisionEvent::Stderr(format!("failed to spawn: {e}")))
                .await;
            return None;
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
                        let _ = output.send(&ProvisionEvent::Stdout(text)).await;
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            line = stderr_lines.next_line() => {
                match line {
                    Ok(Some(text)) => {
                        let _ = output.send(&ProvisionEvent::Stderr(text)).await;
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }
    }

    let status = child.wait().await.ok()?;
    status.code()
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

async fn run_cached_boot_scripts() {
    let scripts_dir = Path::new(SCRIPTS_DIR);
    let mut entries = match tokio::fs::read_dir(scripts_dir).await {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "failed to read scripts dir");
            return;
        }
    };

    let mut boot_scripts = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.ends_with(".boot.sh") {
            boot_scripts.push(entry.path());
        }
    }
    boot_scripts.sort();

    if boot_scripts.is_empty() {
        return;
    }

    tracing::info!(count = boot_scripts.len(), "running cached boot scripts");
    for path in &boot_scripts {
        let filename = path.file_name().unwrap_or_default().to_string_lossy();
        tracing::info!(script = %filename, "executing boot script");

        let content = match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(script = %filename, error = %e, "failed to read script");
                break;
            }
        };

        let status = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&content)
            .status()
            .await;

        match status {
            Ok(s) if s.success() => {
                tracing::info!(script = %filename, "boot script completed");
            }
            Ok(s) => {
                tracing::error!(script = %filename, exit_code = ?s.code(), "boot script failed");
                break;
            }
            Err(e) => {
                tracing::error!(script = %filename, error = %e, "failed to spawn boot script");
                break;
            }
        }
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

    // Run cached boot scripts on reboot (sentinel exists = not first boot)
    if Path::new(SENTINEL_PATH).exists() && Path::new(SCRIPTS_DIR).exists() {
        run_cached_boot_scripts().await;
    }

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
