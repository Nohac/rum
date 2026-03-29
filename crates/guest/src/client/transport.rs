use std::time::Duration;

use roam_stream::{Client as StreamClient, Connector, HandshakeConfig, NoDispatcher, connect};

use crate::agent::{AgentClient as RpcAgentClient, ReadyResponse};

use super::ClientError;

const AGENT_TIMEOUT_SECS: u64 = 120;
const AGENT_RETRY_INTERVAL_MS: u64 = 500;

pub type RpcClient<C> = RpcAgentClient<StreamClient<C, NoDispatcher>>;

#[derive(Clone)]
pub struct Client<C: Connector> {
    rpc: RpcClient<C>,
}

impl<C: Connector> Client<C> {
    pub fn connect(connector: C) -> Self {
        let client = connect(connector, HandshakeConfig::default(), NoDispatcher);
        Self {
            rpc: RpcAgentClient::new(client),
        }
    }

    pub fn rpc(&self) -> &RpcClient<C> {
        &self.rpc
    }

    pub async fn wait_ready(&self) -> Result<ReadyResponse, ClientError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(AGENT_TIMEOUT_SECS);

        loop {
            match self.rpc.ping().await {
                Ok(resp) => {
                    tracing::debug!(
                        version = %resp.version,
                        hostname = %resp.hostname,
                        "agent ready"
                    );
                    return Ok(resp);
                }
                Err(_) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(AGENT_RETRY_INTERVAL_MS)).await;
                }
                Err(e) => {
                    return Err(ClientError::AgentTimeout {
                        timeout_secs: AGENT_TIMEOUT_SECS,
                        message: e.to_string(),
                    });
                }
            }
        }
    }
}

pub async fn wait_for_agent<C: Connector>(connector: C) -> Result<Client<C>, ClientError> {
    let client = Client::connect(connector);
    client.wait_ready().await?;
    Ok(client)
}
