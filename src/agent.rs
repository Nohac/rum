use std::sync::LazyLock;

use roam_stream::{Connector, HandshakeConfig, NoDispatcher, connect};
use rum_agent::RumAgentClient;
use tokio_vsock::{VsockAddr, VsockStream};

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

const VSOCK_PORT: u32 = 2222;
const AGENT_TIMEOUT_SECS: u64 = 120;
const AGENT_RETRY_INTERVAL_MS: u64 = 500;

struct VsockConnector {
    cid: u32,
}

impl Connector for VsockConnector {
    type Transport = VsockStream;

    async fn connect(&self) -> std::io::Result<VsockStream> {
        VsockStream::connect(VsockAddr::new(self.cid, VSOCK_PORT)).await
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
