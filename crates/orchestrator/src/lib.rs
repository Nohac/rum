pub mod driver;
pub mod instance;
pub mod lifecycle;
pub mod setup;

pub use driver::OrchestrationDriver;
pub use instance::{
    BackendDriver, BootFinished, EntityError, GuestConnected, InstancePhase, ManagedInstance,
    OrchestratorPhase, PrepareFinished, ProvisionFinished, ProvisionPlan, RecoveredState,
    ResolvedBaseImage, ShutdownFinished,
};
pub use lifecycle::{OrchestratorMessage, OrchestratorPlugin, ShutdownRequested, build_instance_sm};
pub use setup::{ManagedInstanceSpec, spawn_managed_instance};
