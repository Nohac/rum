use std::path::{Path, PathBuf};
use std::sync::Arc;

use roam_stream::{Client, Connector, HandshakeConfig, NoDispatcher, connect};
pub use rum_agent::{ProvisionScript, RunOn};
use rum_agent::{
    FileChunk, LogEvent, LogLevel, LogStream, ProvisionEvent, RumAgentClient, WriteFileInfo,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
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

pub struct VsockConnector {
    cid: u32,
}

impl Connector for VsockConnector {
    type Transport = VsockStream;

    async fn connect(&self) -> std::io::Result<VsockStream> {
        VsockStream::connect(VsockAddr::new(self.cid, RPC_PORT)).await
    }
}

/// Type alias for the agent RPC client over vsock.
pub type AgentClient = RumAgentClient<Client<VsockConnector, NoDispatcher>>;

/// Wait for the rum-agent in the guest to respond to a ping over vsock.
/// Retries until the agent is ready or the timeout expires.
/// Returns the connected client for further RPC calls.
pub async fn wait_for_agent(cid: u32) -> Result<AgentClient, RumError> {
    let connector = VsockConnector { cid };
    let client = connect(connector, HandshakeConfig::default(), NoDispatcher);
    let service = RumAgentClient::new(client);

    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(AGENT_TIMEOUT_SECS);

    loop {
        match service.ping().await {
            Ok(resp) => {
                tracing::debug!(
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

/// Direction of a copy operation.
pub enum CopyDirection {
    /// Host → guest: local source, guest destination
    Upload {
        local: PathBuf,
        guest: String,
    },
    /// Guest → host: guest source, local destination
    Download {
        guest: String,
        local: PathBuf,
    },
}

/// Parse `rum cp` arguments to determine direction.
///
/// A `:` prefix marks a guest path. Exactly one of src/dst must have it.
pub fn parse_copy_args(src: &str, dst: &str) -> Result<CopyDirection, RumError> {
    let src_guest = src.starts_with(':');
    let dst_guest = dst.starts_with(':');

    match (src_guest, dst_guest) {
        (false, true) => Ok(CopyDirection::Upload {
            local: PathBuf::from(src),
            guest: dst[1..].to_string(),
        }),
        (true, false) => Ok(CopyDirection::Download {
            guest: src[1..].to_string(),
            local: PathBuf::from(dst),
        }),
        (true, true) => Err(RumError::CopyFailed {
            message: "both paths have : prefix — guest-to-guest copy is not supported".into(),
        }),
        (false, false) => Err(RumError::CopyFailed {
            message: "neither path has a : prefix — prefix the guest path with :".into(),
        }),
    }
}

/// Copy a local file to the guest via the agent's write_file RPC.
pub async fn copy_to_guest(cid: u32, local: &Path, guest_path: &str) -> Result<u64, RumError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = tokio::fs::metadata(local).await.map_err(|e| RumError::CopyFailed {
        message: format!("{}: {e}", local.display()),
    })?;
    let mode = metadata.permissions().mode();
    let size = metadata.len();
    let filename = local
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let agent = wait_for_agent(cid).await?;

    let (tx, rx) = roam::channel::<FileChunk>();

    let local_owned = local.to_path_buf();
    let send_task = tokio::spawn(async move {
        let file = tokio::fs::File::open(&local_owned).await?;
        let mut reader = BufReader::new(file);
        const CHUNK_SIZE: usize = 10 * 1024 * 1024;
        let mut buf = vec![0u8; CHUNK_SIZE];

        loop {
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            let chunk = FileChunk {
                data: buf[..n].to_vec(),
            };
            if tx.send(&chunk).await.is_err() {
                break;
            }
        }

        Ok::<(), std::io::Error>(())
    });

    let info = WriteFileInfo {
        path: guest_path.to_string(),
        filename,
        mode,
        size,
    };

    let result = agent
        .write_file(info, rx)
        .await
        .map_err(|e| RumError::CopyFailed {
            message: format!("write_file RPC: {e}"),
        })?;

    send_task
        .await
        .map_err(|e| RumError::CopyFailed {
            message: format!("send task: {e}"),
        })?
        .map_err(|e| RumError::CopyFailed {
            message: format!("send: {e}"),
        })?;

    Ok(result.bytes_written)
}

/// Copy a file from the guest to the local host via the agent's read_file RPC.
pub async fn copy_from_guest(cid: u32, guest_path: &str, local: &Path) -> Result<u64, RumError> {
    use std::os::unix::fs::PermissionsExt;

    let agent = wait_for_agent(cid).await?;

    let (tx, mut rx) = roam::channel::<FileChunk>();

    let guest_owned = guest_path.to_string();
    let read_task = tokio::spawn(async move { agent.read_file(guest_owned, tx).await });

    // Determine final local path — if it's a directory, append the guest filename
    let guest_filename = Path::new(guest_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let final_path = if local.is_dir() {
        local.join(&guest_filename)
    } else {
        local.to_path_buf()
    };

    // Create parent dirs
    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| RumError::CopyFailed {
            message: format!("create dirs: {e}"),
        })?;
    }

    let file = tokio::fs::File::create(&final_path)
        .await
        .map_err(|e| RumError::CopyFailed {
            message: format!("{}: {e}", final_path.display()),
        })?;
    let mut writer = BufWriter::new(file);
    let mut bytes_written: u64 = 0;

    while let Ok(Some(chunk)) = rx.recv().await {
        writer.write_all(&chunk.data).await.map_err(|e| RumError::CopyFailed {
            message: format!("write: {e}"),
        })?;
        bytes_written += chunk.data.len() as u64;
    }

    writer.flush().await.map_err(|e| RumError::CopyFailed {
        message: format!("flush: {e}"),
    })?;

    let result = read_task
        .await
        .map_err(|e| RumError::CopyFailed {
            message: format!("read task: {e}"),
        })?
        .map_err(|e| RumError::CopyFailed {
            message: format!("read_file RPC: {e}"),
        })?;

    // Set permissions from guest file mode
    tokio::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(result.mode))
        .await
        .map_err(|e| RumError::CopyFailed {
            message: format!("chmod: {e}"),
        })?;

    Ok(bytes_written)
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
pub async fn run_provision(
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
