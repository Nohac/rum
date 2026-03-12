use std::io;
use std::path::PathBuf;

use roam_stream::{Client, Connector, NoDispatcher, HandshakeConfig};
use tokio::net::UnixStream;

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::paths;

use super::rpc::RumDaemonClient;

// ── Connector: always Unix socket ───────────────────────────────────

pub struct DaemonConnector {
    path: PathBuf,
}

impl Connector for DaemonConnector {
    type Transport = UnixStream;

    async fn connect(&self) -> io::Result<UnixStream> {
        UnixStream::connect(&self.path).await
    }
}

// ── Client type alias ───────────────────────────────────────────────

pub type DaemonClient = RumDaemonClient<Client<DaemonConnector, NoDispatcher>>;

// ── connect(): create a client connected to the daemon ──────────────

pub fn connect(sys_config: &SystemConfig) -> Result<DaemonClient, RumError> {
    if !is_daemon_running(sys_config) {
        return Err(RumError::Daemon {
            message: format!(
                "no daemon running for '{}'. Run `rum up` first.",
                sys_config.display_name()
            ),
        });
    }
    let sock = paths::socket_path(&sys_config.id, sys_config.name.as_deref());
    let connector = DaemonConnector { path: sock };
    let client = roam_stream::connect(connector, HandshakeConfig::default(), NoDispatcher);
    Ok(RumDaemonClient::new(client))
}

pub fn is_daemon_running(sys_config: &SystemConfig) -> bool {
    let pid_file = paths::pid_path(&sys_config.id, sys_config.name.as_deref());
    let Ok(contents) = std::fs::read_to_string(&pid_file) else {
        return false;
    };
    let Ok(pid) = contents.trim().parse::<i32>() else {
        return false;
    };
    // Check if process is alive via /proc
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}
