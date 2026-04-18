use std::path::{Path, PathBuf};
use std::time::Duration;

use facet::Facet;
use roam_stream::{Connector, HandshakeConfig, NoDispatcher, accept, connect};
use tokio::net::{UnixListener, UnixStream};

/// Minimal daemon-control sidechannel kept separate from the ECS protocol.
///
/// This socket exists specifically so the client can recover from ECS protocol
/// mismatches and ask the daemon process to exit without touching the VM.
#[derive(Debug, Clone, Facet)]
pub struct ShutdownDaemonReply {
    pub pid: u32,
}

#[roam::service]
pub trait Control {
    async fn shutdown_daemon(&self) -> ShutdownDaemonReply;
}

#[derive(Clone)]
struct ControlService {
    main_socket_path: PathBuf,
    control_socket_path: PathBuf,
}

impl Control for ControlService {
    async fn shutdown_daemon(&self, _cx: &roam::Context) -> ShutdownDaemonReply {
        let pid = std::process::id();
        let main_socket_path = self.main_socket_path.clone();
        let control_socket_path = self.control_socket_path.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = std::fs::remove_file(&main_socket_path);
            let _ = std::fs::remove_file(&control_socket_path);
            std::process::exit(0);
        });

        ShutdownDaemonReply { pid }
    }
}

/// Spawn the daemon control sidechannel listener.
pub async fn run_control_server(
    control_socket_path: PathBuf,
    main_socket_path: PathBuf,
) -> anyhow::Result<()> {
    let _ = std::fs::remove_file(&control_socket_path);
    let listener = UnixListener::bind(&control_socket_path)?;
    tracing::info!(socket = %control_socket_path.display(), "rum daemon control listening");

    loop {
        let (stream, _addr) = listener.accept().await?;
        let dispatcher = ControlDispatcher::new(ControlService {
            main_socket_path: main_socket_path.clone(),
            control_socket_path: control_socket_path.clone(),
        });

        tokio::spawn(async move {
            match accept(stream, HandshakeConfig::default(), dispatcher).await {
                Ok((_handle, _incoming, driver)) => {
                    if let Err(error) = driver.run().await {
                        tracing::error!(error = %error, "control driver error");
                    }
                }
                Err(error) => tracing::error!(error = %error, "control handshake failed"),
            }
        });
    }
}

/// Request that the daemon process exit and return its pid.
pub async fn shutdown_daemon(control_socket_path: &Path) -> anyhow::Result<u32> {
    let connector = UnixConnector {
        path: control_socket_path.to_path_buf(),
    };
    let client = ControlClient::new(connect(connector, HandshakeConfig::default(), NoDispatcher));
    let reply = client.shutdown_daemon().await?;
    Ok(reply.pid)
}

#[derive(Clone)]
struct UnixConnector {
    path: PathBuf,
}

impl Connector for UnixConnector {
    type Transport = UnixStream;

    async fn connect(&self) -> std::io::Result<UnixStream> {
        UnixStream::connect(&self.path).await
    }
}
