mod libvirt;

use std::path::Path;

use crate::instance::InstanceState;

/// Standard operational surface for one runtime backend handle.
///
/// Recovery and persisted state inspection are intentionally not part of this
/// trait. Those are owned by the instance layer, while a driver focuses on
/// long-running backend operations once the runtime has been selected.
#[allow(async_fn_in_trait)]
pub trait Driver: Clone {
    type Error;

    /// Stable configured identity for the managed runtime.
    fn id(&self) -> &str;
    /// Human-facing backend name for the managed runtime.
    fn name(&self) -> &str;

    /// Prepare backend state and artifacts needed before boot.
    async fn prepare(&self, base_image: &Path) -> Result<(), Self::Error>;
    /// Boot the runtime and return the guest-agent endpoint identifier.
    async fn boot(&self) -> Result<u32, Self::Error>;
    /// Request a graceful shutdown.
    async fn shutdown(&self) -> Result<(), Self::Error>;
    /// Tear down the runtime and its backend-managed resources.
    async fn destroy(&self) -> Result<(), Self::Error>;
}

/// Backend-specific recovery boundary used by the instance layer.
///
/// The instance owns persisted identity and the recovered state value, while
/// each backend implementation knows how to inspect its own runtime state.
pub trait RecoverableDriver: Driver {
    /// Recover the current persisted/runtime state for this driver.
    fn recover(&self) -> Result<InstanceState, Self::Error>;
}

pub use libvirt::LibvirtDriver;
