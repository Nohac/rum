use crate::agent_client::AgentClient;
use crate::error::RumError;

pub async fn connect_agent(cid: u32) -> Result<AgentClient, RumError> {
    crate::agent_client::wait_for_agent(cid).await
}
