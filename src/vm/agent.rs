use crate::agent::AgentClient;
use crate::error::RumError;

pub async fn connect_agent(cid: u32) -> Result<AgentClient, RumError> {
    crate::agent::wait_for_agent(cid).await
}
