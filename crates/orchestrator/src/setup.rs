use std::path::PathBuf;

use ecsdk::prelude::*;
use guest::agent::ProvisionScript;

use crate::driver::OrchestrationDriver;
use crate::instance::{
    InstanceLabel, LogBuffer, ManagedInstance, ProvisionLogView, ProvisionPlan, ResolvedBaseImage,
    instance_phase::Recovering,
};
use crate::lifecycle::build_instance_sm;

/// Startup description for one managed orchestration entity.
///
/// Bootstrap code constructs this value from a concrete machine instance and
/// the inputs already resolved outside the state machine, such as the base
/// image path and provisioning plan.
pub struct ManagedInstanceSpec<D: OrchestrationDriver> {
    instance: machine::instance::Instance<D>,
    label: Option<String>,
    resolved_base_image: Option<PathBuf>,
    provision_plan: Vec<ProvisionScript>,
}

impl<D: OrchestrationDriver> ManagedInstanceSpec<D> {
    /// Create a new managed instance spec for the given runtime instance.
    pub fn new(instance: machine::instance::Instance<D>) -> Self {
        Self {
            instance,
            label: None,
            resolved_base_image: None,
            provision_plan: Vec::new(),
        }
    }

    /// Attach the human-facing label that renderers should show for this
    /// managed instance.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Attach the base image path that the prepare step should consume.
    pub fn with_resolved_base_image(mut self, resolved_base_image: impl Into<PathBuf>) -> Self {
        self.resolved_base_image = Some(resolved_base_image.into());
        self
    }

    /// Attach the provisioning plan to run after guest connectivity is ready.
    pub fn with_provision_plan(mut self, provision_plan: Vec<ProvisionScript>) -> Self {
        self.provision_plan = provision_plan;
        self
    }
}

/// Spawn one managed orchestration entity into the world.
///
/// The entity starts in the `Recovering` phase with the instance lifecycle
/// state machine already attached, so bootstrap callers do not need to know
/// the internal orchestrator component layout.
pub fn spawn_managed_instance<D: OrchestrationDriver>(
    world: &mut World,
    spec: ManagedInstanceSpec<D>,
) -> Entity {
    let mut entity = world.spawn((
        Replicated,
        LogBuffer::default(),
        ManagedInstance(spec.instance),
        ProvisionLogView::default(),
        ProvisionPlan(spec.provision_plan),
        build_instance_sm::<D>(),
        Recovering,
    ));

    if let Some(label) = spec.label {
        entity.insert(InstanceLabel(label));
    }

    if let Some(image) = spec.resolved_base_image {
        entity.insert(ResolvedBaseImage(image));
    }

    entity.id()
}
