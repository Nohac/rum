use roam_stream::{Client, Connector, HandshakeConfig, NoDispatcher, connect};
use agent::AgentClient as RpcAgentClient;
use tokio_vsock::{VsockAddr, VsockStream};

use crate::error::Error;

pub const AGENT_BINARY: &[u8] = include_bytes!(env!("CARGO_BIN_FILE_AGENT"));

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

pub type AgentClient = RpcAgentClient<Client<VsockConnector, NoDispatcher>>;

pub async fn wait_for_agent(cid: u32) -> Result<AgentClient, Error> {
    let connector = VsockConnector { cid };
    let client = connect(connector, HandshakeConfig::default(), NoDispatcher);
    let service = RpcAgentClient::new(client);
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
                return Err(Error::AgentTimeout {
                    message: format!("agent did not respond within {AGENT_TIMEOUT_SECS}s: {e}"),
                });
            }
        }
    }
}
