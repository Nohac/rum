use crate::config::SystemConfig;
use crate::driver::{Driver, LibvirtDriver, RecoverableDriver};
use crate::error::Error;
use crate::layout::MachineLayout;

/// Selected runtime backend for an instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Libvirt,
}

/// Recoverable lifecycle state for one persisted runtime instance.
///
/// The state is instance-owned rather than driver-owned because it is derived
/// from both backend state and on-disk artifacts such as overlays, seed ISOs,
/// and provision markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceState {
    Missing,
    ImageCached,
    Prepared,
    PartialBoot,
    Stopped,
    Running,
    StaleConfig,
}

/// Persistent instance view used by higher-level orchestration.
///
/// `Instance` is the boundary between orchestration and backend operations:
/// it owns recovered state and persisted identity, while the contained driver
/// performs backend-specific actions.
#[derive(Clone)]
pub struct Instance<D: Driver> {
    backend: BackendKind,
    driver: D,
}

impl Instance<LibvirtDriver> {
    /// Create a libvirt-backed instance view for the given system config.
    ///
    /// This does not mutate backend state. It prepares the persistent identity
    /// and backend handle needed for recovery and later operations.
    pub fn new(system: SystemConfig) -> Self {
        Self {
            backend: BackendKind::Libvirt,
            driver: LibvirtDriver::new(system),
        }
    }
}

impl<D: Driver> Instance<D> {
    /// Return the selected backend kind for this instance.
    pub fn backend_kind(&self) -> BackendKind {
        self.backend
    }

    /// Return a clonable backend driver for this instance.
    pub fn driver(&self) -> D {
        self.driver.clone()
    }

    /// Access the backend driver by reference.
    pub fn driver_ref(&self) -> &D {
        &self.driver
    }
}

impl<D> Instance<D>
where
    D: RecoverableDriver<Error = Error>,
{
    /// Recover the current lifecycle state through the selected backend.
    ///
    /// The instance remains the orchestration-facing owner of the recovered
    /// state value, while the backend-specific inspection logic lives behind
    /// the `RecoverableDriver` trait.
    pub fn recover(&self) -> Result<InstanceState, Error> {
        self.driver.recover()
    }
}

impl Instance<LibvirtDriver> {
    /// Access the resolved system config for this instance.
    pub fn system(&self) -> &SystemConfig {
        self.driver.system()
    }

    /// Access the derived on-disk layout for this instance.
    pub fn layout(&self) -> &MachineLayout {
        self.driver.layout()
    }
}
