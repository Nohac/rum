use ecsdk::prelude::*;
use ecsdk::tasks::SpawnTask;
use seldom_state::prelude::*;

use crate::driver::OrchestrationDriver;
use crate::instance::{
    EntityError, ManagedInstance, ProvisionPlan, RecoveredState, ResolvedBaseImage,
    instance_phase::{Booting, ConnectingGuest, Failed, Preparing, Provisioning, Recovering, Running, ShuttingDown, Stopped},
};

/// Resource toggled when a shutdown has been requested.
#[derive(Resource, Default)]
pub struct ShutdownRequested(pub bool);

/// Domain messages emitted by orchestrator tasks and applied back into ECS.
#[derive(Clone, Debug)]
pub enum OrchestratorMessage {
    MarkDone { entity: Entity },
    MarkFailed { entity: Entity, message: String },
    RequestShutdown,
}

impl ApplyMessage for OrchestratorMessage {
    fn apply(&self, world: &mut World) {
        match self {
            Self::MarkDone { entity } => {
                if let Ok(mut entity) = world.get_entity_mut(*entity) {
                    entity.insert(Done::Success);
                }
            }
            Self::MarkFailed { entity, message } => {
                if let Ok(mut entity) = world.get_entity_mut(*entity) {
                    entity.insert((EntityError(message.clone()), Done::Failure));
                }
            }
            Self::RequestShutdown => {
                world.resource_mut::<ShutdownRequested>().0 = true;
            }
        }
    }
}

fn needs_prepare<D: OrchestrationDriver>(
    In(entity): In<Entity>,
    recovered: Query<&RecoveredState, With<ManagedInstance<D>>>,
) -> bool {
    matches!(
        recovered.get(entity),
        Ok(RecoveredState(
            machine::instance::InstanceState::Missing
                | machine::instance::InstanceState::ImageCached
                | machine::instance::InstanceState::Prepared
                | machine::instance::InstanceState::PartialBoot
        ))
    )
}

fn needs_boot<D: OrchestrationDriver>(
    In(entity): In<Entity>,
    recovered: Query<&RecoveredState, With<ManagedInstance<D>>>,
) -> bool {
    matches!(
        recovered.get(entity),
        Ok(RecoveredState(machine::instance::InstanceState::Stopped))
    )
}

fn needs_guest_connect<D: OrchestrationDriver>(
    In(entity): In<Entity>,
    recovered: Query<&RecoveredState, With<ManagedInstance<D>>>,
) -> bool {
    matches!(
        recovered.get(entity),
        Ok(RecoveredState(machine::instance::InstanceState::Running))
    )
}

fn failed_recovery<D: OrchestrationDriver>(
    In(entity): In<Entity>,
    recovered: Query<&RecoveredState, With<ManagedInstance<D>>>,
    errors: Query<(), With<EntityError>>,
) -> bool {
    errors.get(entity).is_ok()
        || matches!(
            recovered.get(entity),
            Ok(RecoveredState(machine::instance::InstanceState::StaleConfig))
        )
}

fn shutdown_requested(In(_entity): In<Entity>, shutdown: Res<ShutdownRequested>) -> bool {
    shutdown.0
}

/// Build the per-instance lifecycle state machine.
pub fn build_instance_sm<D: OrchestrationDriver>() -> StateMachine {
    StateMachine::default()
        .trans::<Recovering, _>(needs_prepare::<D>, Preparing)
        .trans::<Recovering, _>(needs_boot::<D>, Booting)
        .trans::<Recovering, _>(needs_guest_connect::<D>, ConnectingGuest)
        .trans::<Recovering, _>(failed_recovery::<D>, Failed)
        .trans::<Preparing, _>(done(Some(Done::Success)), Booting)
        .trans::<Preparing, _>(done(Some(Done::Failure)), Failed)
        .trans::<Booting, _>(done(Some(Done::Success)), ConnectingGuest)
        .trans::<Booting, _>(done(Some(Done::Failure)), Failed)
        .trans::<ConnectingGuest, _>(done(Some(Done::Success)), Provisioning)
        .trans::<ConnectingGuest, _>(done(Some(Done::Failure)), Failed)
        .trans::<Provisioning, _>(done(Some(Done::Success)), Running)
        .trans::<Provisioning, _>(done(Some(Done::Failure)), Failed)
        .trans::<Running, _>(shutdown_requested, ShuttingDown)
        .trans::<ShuttingDown, _>(done(Some(Done::Success)), Stopped)
        .trans::<ShuttingDown, _>(done(Some(Done::Failure)), Failed)
        .set_trans_logging(true)
}

fn on_recovering<D: OrchestrationDriver>(
    trigger: On<Insert, Recovering>,
    mut commands: Commands,
    instances: Query<&ManagedInstance<D>>,
) {
    let entity = trigger.event_target();
    let Ok(instance) = instances.get(entity) else {
        return;
    };

    match instance.0.recover() {
        Ok(state) => {
            commands.entity(entity).insert(RecoveredState(state));
        }
        Err(error) => {
            commands.entity(entity).insert(EntityError(error.to_string()));
        }
    }
}

fn on_preparing<D: OrchestrationDriver>(
    trigger: On<Insert, Preparing>,
    mut commands: Commands,
    instances: Query<&ManagedInstance<D>>,
    images: Query<&ResolvedBaseImage>,
) {
    let entity = trigger.event_target();
    let Ok(instance) = instances.get(entity) else {
        return;
    };
    let Ok(image) = images.get(entity) else {
        commands.send_msg(OrchestratorMessage::MarkFailed {
            entity,
            message: "missing resolved base image".into(),
        });
        return;
    };

    let driver = instance.0.driver();
    let image_path = image.0.clone();
    commands.entity(entity).spawn_task(move |task| async move {
        match driver.prepare(&image_path).await {
            Ok(()) => task.send_msg(OrchestratorMessage::MarkDone { entity }),
            Err(error) => task.send_msg(OrchestratorMessage::MarkFailed {
                entity,
                message: error.to_string(),
            }),
        }
    });
}

fn on_booting<D: OrchestrationDriver>(
    trigger: On<Insert, Booting>,
    mut commands: Commands,
    instances: Query<&ManagedInstance<D>>,
) {
    let entity = trigger.event_target();
    let Ok(instance) = instances.get(entity) else {
        return;
    };

    let driver = instance.0.driver();
    commands.entity(entity).spawn_task(move |task| async move {
        match driver.boot().await {
            Ok(_) => task.send_msg(OrchestratorMessage::MarkDone { entity }),
            Err(error) => task.send_msg(OrchestratorMessage::MarkFailed {
                entity,
                message: error.to_string(),
            }),
        }
    });
}

fn on_connecting_guest<D: OrchestrationDriver>(
    trigger: On<Insert, ConnectingGuest>,
    mut commands: Commands,
    instances: Query<&ManagedInstance<D>>,
) {
    let entity = trigger.event_target();
    let Ok(instance) = instances.get(entity) else {
        return;
    };

    let driver = instance.0.driver();
    commands.entity(entity).spawn_task(move |task| async move {
        match driver.connect_guest().await {
            Ok(()) => task.send_msg(OrchestratorMessage::MarkDone { entity }),
            Err(error) => task.send_msg(OrchestratorMessage::MarkFailed {
                entity,
                message: error.to_string(),
            }),
        }
    });
}

fn on_provisioning<D: OrchestrationDriver>(
    trigger: On<Insert, Provisioning>,
    mut commands: Commands,
    instances: Query<&ManagedInstance<D>>,
    plans: Query<Option<&ProvisionPlan>>,
) {
    let entity = trigger.event_target();
    let Ok(instance) = instances.get(entity) else {
        return;
    };

    let scripts = plans
        .get(entity)
        .ok()
        .flatten()
        .map(|plan| plan.0.clone())
        .unwrap_or_default();

    let driver = instance.0.driver();
    commands.entity(entity).spawn_task(move |task| async move {
        match driver.provision(scripts).await {
            Ok(()) => task.send_msg(OrchestratorMessage::MarkDone { entity }),
            Err(error) => task.send_msg(OrchestratorMessage::MarkFailed {
                entity,
                message: error.to_string(),
            }),
        }
    });
}

fn on_shutting_down<D: OrchestrationDriver>(
    trigger: On<Insert, ShuttingDown>,
    mut commands: Commands,
    instances: Query<&ManagedInstance<D>>,
) {
    let entity = trigger.event_target();
    let Ok(instance) = instances.get(entity) else {
        return;
    };

    let driver = instance.0.driver();
    commands.entity(entity).spawn_task(move |task| async move {
        match driver.shutdown().await {
            Ok(()) => task.send_msg(OrchestratorMessage::MarkDone { entity }),
            Err(error) => task.send_msg(OrchestratorMessage::MarkFailed {
                entity,
                message: error.to_string(),
            }),
        }
    });
}

/// Registers the orchestrator state machine and side-effect observers.
pub struct OrchestratorPlugin<D: OrchestrationDriver>(std::marker::PhantomData<D>);

impl<D: OrchestrationDriver> Default for OrchestratorPlugin<D> {
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<D: OrchestrationDriver> IsomorphicPlugin for OrchestratorPlugin<D> {
    fn build_shared(&self, app: &mut App) {
        app.add_plugins(StateMachinePlugin::default().schedule(PreUpdate));
        app.init_resource::<ShutdownRequested>();

        app.add_observer(on_recovering::<D>);
        app.add_observer(on_preparing::<D>);
        app.add_observer(on_booting::<D>);
        app.add_observer(on_connecting_guest::<D>);
        app.add_observer(on_provisioning::<D>);
        app.add_observer(on_shutting_down::<D>);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use ecsdk::core::{CmdQueue, MessageQueue};
    use machine::error::Error;
    use machine::driver::{Driver, RecoverableDriver};

    use super::*;
    use crate::driver::OrchestrationDriver;
    use crate::instance::{ManagedInstance, RecoveredState};

    #[derive(Clone)]
    struct MockDriver {
        state: machine::instance::InstanceState,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl MockDriver {
        fn new(state: machine::instance::InstanceState) -> Self {
            Self {
                state,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl Driver for MockDriver {
        type Error = Error;

        fn id(&self) -> &str {
            "mock"
        }

        fn name(&self) -> &str {
            "mock"
        }

        async fn prepare(&self, _base_image: &Path) -> Result<(), Self::Error> {
            self.calls.lock().unwrap().push("prepare");
            Ok(())
        }

        async fn boot(&self) -> Result<u32, Self::Error> {
            self.calls.lock().unwrap().push("boot");
            Ok(7)
        }

        async fn shutdown(&self) -> Result<(), Self::Error> {
            self.calls.lock().unwrap().push("shutdown");
            Ok(())
        }

        async fn destroy(&self) -> Result<(), Self::Error> {
            self.calls.lock().unwrap().push("destroy");
            Ok(())
        }
    }

    impl RecoverableDriver for MockDriver {
        fn recover(&self) -> Result<machine::instance::InstanceState, Self::Error> {
            Ok(self.state)
        }
    }

    #[async_trait]
    impl OrchestrationDriver for MockDriver {
        async fn connect_guest(&self) -> Result<(), Error> {
            self.calls.lock().unwrap().push("connect_guest");
            Ok(())
        }

        async fn provision(&self, _scripts: Vec<guest::agent::ProvisionScript>) -> Result<(), Error> {
            self.calls.lock().unwrap().push("provision");
            Ok(())
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.insert_resource(CmdQueue::test());
        app.insert_resource(MessageQueue::<OrchestratorMessage>::test());
        app.add_shared_plugin(OrchestratorPlugin::<MockDriver>::default());
        app
    }

    #[test]
    fn recovering_missing_records_recovered_state() {
        let mut app = test_app();
        let entity = app.world_mut().spawn((
            ManagedInstance(machine::instance::Instance::new_with_driver(
                MockDriver::new(machine::instance::InstanceState::Missing),
                machine::instance::BackendKind::Libvirt,
            )),
            build_instance_sm::<MockDriver>(),
            Recovering,
        )).id();

        app.update();

        assert_eq!(
            app.world().get::<RecoveredState>(entity).map(|s| s.0),
            Some(machine::instance::InstanceState::Missing)
        );
    }
}
