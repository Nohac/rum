use std::marker::PhantomData;
use std::path::PathBuf;

use ecsdk::prelude::*;
use guest::agent::ProvisionScript;

use crate::driver::OrchestrationDriver;

/// Component that attaches a concrete runtime instance to an ECS entity.
#[derive(Component, Clone)]
pub struct ManagedInstance<D: OrchestrationDriver>(pub machine::instance::Instance<D>);

/// Component that carries the recovered persistent/runtime state for an entity.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecoveredState(pub machine::instance::InstanceState);

/// Resolved base image path used by the prepare step.
#[derive(Component, Clone, Debug)]
pub struct ResolvedBaseImage(pub PathBuf);

/// Provisioning plan to run once guest connectivity is available.
#[derive(Component, Clone, Default, Debug)]
pub struct ProvisionPlan(pub Vec<ProvisionScript>);

/// Recorded orchestration error for an entity.
#[derive(Component, Clone, Debug)]
pub struct EntityError(pub String);

/// Per-entity lifecycle phase driven by the orchestrator state machine.
#[derive(Component, StateComponent, PartialEq, Eq, Clone, Copy, Debug)]
pub enum InstancePhase {
    Recovering,
    Preparing,
    Booting,
    ConnectingGuest,
    Provisioning,
    Running,
    ShuttingDown,
    Stopped,
    Failed,
}

/// Top-level orchestration phase for the app as a whole.
#[derive(Component, StateComponent, PartialEq, Eq, Clone, Copy, Debug)]
pub enum OrchestratorPhase {
    Starting,
    Running,
    ShuttingDown,
    Stopped,
    Failed,
}

/// Marker component that carries the concrete driver type into the ECS world.
///
/// This is useful when spawning orchestration entities from bootstrap code.
#[derive(Component, Default)]
pub struct BackendDriver<D: OrchestrationDriver>(pub PhantomData<D>);
