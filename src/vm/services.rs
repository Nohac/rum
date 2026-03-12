use std::path::Path;

use tokio::task::JoinHandle;

use crate::config::SystemConfig;
use crate::error::RumError;

pub async fn run_provision(
    agent: &crate::agent::AgentClient,
    scripts: Vec<rum_agent::ProvisionScript>,
    logs_dir: &Path,
) -> Result<(), RumError> {
    let mut progress =
        crate::progress::StepProgress::new(scripts.len(), crate::progress::OutputMode::Silent);
    crate::agent::run_provision(agent, scripts, &mut progress, logs_dir).await
}

pub(crate) async fn start_services(
    cid: u32,
    sys_config: &SystemConfig,
) -> Result<crate::daemon::ServiceHandles, RumError> {
    let config = &sys_config.config;
    let agent_client = crate::agent::wait_for_agent(cid).await.ok();
    let log_handle = agent_client.as_ref().map(crate::agent::start_log_subscription);
    let forward_handles = if !config.ports.is_empty() {
        crate::agent::start_port_forwards(cid, &config.ports).await?
    } else {
        Vec::<JoinHandle<()>>::new()
    };

    Ok(crate::daemon::ServiceHandles {
        log_handle,
        forward_handles,
    })
}
