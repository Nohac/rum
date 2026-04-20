use std::path::PathBuf;

use bevy::prelude::Deref;
use ecsdk::prelude::*;
use guest::agent::ProvisionScript;
use serde::{Deserialize, Serialize};

use crate::driver::OrchestrationDriver;

/// Component that attaches a concrete runtime instance to an ECS entity.
#[derive(Component, Clone, Deref)]
pub struct ManagedInstance<D: OrchestrationDriver>(pub machine::instance::Instance<D>);

/// Component that carries the recovered persistent/runtime state for an entity.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Deref, Serialize, Deserialize)]
pub struct RecoveredState(pub machine::instance::InstanceState);

/// Human-facing label for one managed instance entity.
#[derive(Component, Clone, Debug, Deref, Serialize, Deserialize)]
pub struct InstanceLabel(pub String);

/// Resolved base image path used by the prepare step.
#[derive(Component, Clone, Debug, Deref)]
pub struct ResolvedBaseImage(pub PathBuf);

/// Provisioning plan to run once guest connectivity is available.
#[derive(Component, Clone, Default, Debug, Deref)]
pub struct ProvisionPlan(pub Vec<ProvisionScript>);

/// Recorded orchestration error for an entity.
#[derive(Component, Clone, Debug, Deref, Serialize, Deserialize)]
pub struct EntityError(pub String);

/// Non-replicated buffer of line-oriented runtime output collected on the
/// server before it is drained into replicated log entries.
#[derive(Clone, Debug)]
pub struct LogLine {
    pub text: String,
}

/// Server-local log buffer for one managed instance.
#[derive(Component, Default)]
pub struct LogBuffer {
    pub lines: Vec<LogLine>,
}

impl LogBuffer {
    pub fn push(&mut self, text: impl Into<String>) {
        self.lines.push(LogLine { text: text.into() });
    }

    pub fn drain(&mut self) -> impl Iterator<Item = LogLine> + '_ {
        self.lines.drain(..)
    }
}

/// Replicated immutable log entry linked to one managed instance.
#[derive(Component, Clone, Debug, Serialize, Deserialize)]
#[relationship(relationship_target = ProvisionLogView)]
#[component(immutable)]
pub struct ProvisionLogEntry {
    #[entities]
    #[relationship]
    pub target: Entity,
    pub label: String,
    pub message: String,
}

/// Replicated relationship target that holds the ordered log entries for one
/// managed instance.
#[derive(Component, Default, Clone, Debug, Serialize, Deserialize)]
#[relationship_target(relationship = ProvisionLogEntry, linked_spawn)]
pub struct ProvisionLogView(Vec<Entity>);

/// Marker inserted once the prepare step has completed successfully.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PrepareFinished;

/// Marker inserted once the boot step has completed successfully.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct BootFinished;

/// Marker inserted once guest connectivity has been established.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct GuestConnected;

/// Marker inserted once provisioning has completed successfully.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ProvisionFinished;

/// Marker inserted once shutdown has completed successfully.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ShutdownFinished;

/// Per-entity lifecycle phase driven by the orchestrator state machine.
#[derive(Component, StateComponent, PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
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

impl InstancePhase {
    /// Plain-text label for the current lifecycle phase.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Recovering => "Recovering",
            Self::Preparing => "Preparing",
            Self::Booting => "Booting",
            Self::ConnectingGuest => "Connecting guest",
            Self::Provisioning => "Provisioning",
            Self::Running => "Running",
            Self::ShuttingDown => "Shutting down",
            Self::Stopped => "Stopped",
            Self::Failed => "Failed",
        }
    }
}
