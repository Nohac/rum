use async_trait::async_trait;
use guest::agent::ProvisionScript;
use machine::driver::{Driver, LibvirtDriver, RecoverableDriver};
use machine::error::Error;
use machine::guest::VsockConnector;
use std::sync::Arc;

pub type OutputCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Driver surface required by the orchestrator state machines.
///
/// This extends the machine-layer driver with the guest-facing steps the
/// orchestrator needs to model the documented runtime flows.
/// The orchestrator runs these methods inside `ecsdk` entity tasks, so the
/// futures must be `Send`. We use `async_trait` to keep the trait readable
/// while still making that requirement explicit at the trait boundary.
#[async_trait]
pub trait OrchestrationDriver:
    Driver<Error = Error> + RecoverableDriver<Error = Error> + Clone + Send + Sync + 'static
{
    /// Wait for the guest connection surface to become available.
    async fn connect_guest(&self) -> Result<(), Error>;

    /// Run the current provisioning plan.
    async fn provision(&self, scripts: Vec<ProvisionScript>) -> Result<(), Error>;

    /// Run the current provisioning plan and emit line-oriented output as it
    /// arrives from the guest.
    async fn provision_with_output(
        &self,
        scripts: Vec<ProvisionScript>,
        on_output: OutputCallback,
    ) -> Result<(), Error> {
        let _ = on_output;
        self.provision(scripts).await
    }
}

#[async_trait]
impl OrchestrationDriver for LibvirtDriver {
    async fn connect_guest(&self) -> Result<(), Error> {
        let cid = self.get_vsock_cid()?;
        guest::client::wait_for_agent(VsockConnector::new(cid))
            .await
            .map(|_| ())
            .map_err(map_guest_error)
    }

    async fn provision(&self, scripts: Vec<ProvisionScript>) -> Result<(), Error> {
        if scripts.is_empty() {
            return Ok(());
        }

        let cid = self.get_vsock_cid()?;
        let client = guest::client::wait_for_agent(VsockConnector::new(cid))
            .await
            .map_err(map_guest_error)?;

        client
            .provision(scripts, &self.layout().logs_dir)
            .await
            .map_err(map_guest_error)
    }

    async fn provision_with_output(
        &self,
        scripts: Vec<ProvisionScript>,
        on_output: OutputCallback,
    ) -> Result<(), Error> {
        if scripts.is_empty() {
            return Ok(());
        }

        let cid = self.get_vsock_cid()?;
        let client = guest::client::wait_for_agent(VsockConnector::new(cid))
            .await
            .map_err(map_guest_error)?;

        client
            .provision_with_output(scripts, &self.layout().logs_dir, move |line| {
                on_output(line);
            })
            .await
            .map_err(map_guest_error)
    }
}

fn map_guest_error(error: guest::client::ClientError) -> Error {
    match error {
        guest::client::ClientError::Io { context, source } => Error::Io { context, source },
        guest::client::ClientError::AgentTimeout { message, .. } => Error::AgentTimeout { message },
        guest::client::ClientError::Rpc { context, message } => Error::Daemon {
            message: format!("{context}: {message}"),
        },
        guest::client::ClientError::CopyFailed { message } => Error::CopyFailed { message },
        guest::client::ClientError::ProvisionFailed { script } => Error::ProvisionFailed { script },
    }
}
