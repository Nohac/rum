mod libvirt;

use std::path::Path;

use crate::state::MachineState;

#[allow(async_fn_in_trait)]
pub trait VirtualMachine: Clone {
    type Error;

    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn recover_state(&self) -> Result<MachineState, Self::Error>;

    async fn prepare(&self, base_image: &Path) -> Result<(), Self::Error>;
    async fn boot(&self) -> Result<u32, Self::Error>;
    async fn shutdown(&self) -> Result<(), Self::Error>;
    async fn destroy(&self) -> Result<(), Self::Error>;
}

pub use libvirt::LibvirtDriver;
