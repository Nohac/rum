use bevy::app::prelude::*;
use bevy::ecs::prelude::*;
use seldom_state::prelude::*;

use crate::phase::{FlowIntent, ShutdownRequested};

pub fn build_sm_for_intent(intent: FlowIntent) -> StateMachine {
    match intent {
        FlowIntent::FirstBoot => super::first_boot::build_sm(),
        FlowIntent::Reboot => super::reboot::build_sm(),
        FlowIntent::Reattach => super::reattach::build_sm(),
        FlowIntent::Shutdown => super::shutdown::build_sm(),
        FlowIntent::Destroy => super::destroy::build_sm(),
        FlowIntent::Reprovision => super::reprovision::build_sm(),
    }
}

pub(crate) fn always(In(_entity): In<Entity>) -> bool {
    true
}

pub(crate) fn shutdown_requested(In(_entity): In<Entity>, flag: Res<ShutdownRequested>) -> bool {
    flag.0
}

pub struct LifecycleTestPlugin;

impl Plugin for LifecycleTestPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(StateMachinePlugin::default().schedule(PreUpdate));
        app.init_resource::<ShutdownRequested>();
    }
}

pub struct LifecyclePlugin;

impl Plugin for LifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(StateMachinePlugin::default().schedule(PreUpdate));
        app.init_resource::<ShutdownRequested>();
        app.add_systems(Update, super::terminal::advance_step_progress);
        app.add_observer(super::prepare::on_downloading_image);
        app.add_observer(super::prepare::on_preparing);
        app.add_observer(super::prepare::on_booting);
        app.add_observer(super::agent::on_connecting_agent);
        app.add_observer(super::provision::on_provisioning);
        app.add_observer(super::agent::on_starting_services);
        app.add_observer(super::stop::on_shutting_down);
        app.add_observer(super::destroy::on_destroying);
        app.add_observer(super::terminal::on_running);
        app.add_observer(super::terminal::on_stopped);
        app.add_observer(super::terminal::on_destroyed);
        app.add_observer(super::terminal::on_failed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase::VmPhase;
    use crate::phase::vm_phase::{
        Booting, ConnectingAgent, Destroyed, Destroying, DownloadingImage, Failed, Preparing,
        Provisioning, Running, ShuttingDown, StartingServices, Stopped, Virgin,
    };
    use bevy::app::App;
    use ecsdk_core::CmdQueue;

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(LifecycleTestPlugin);
        app.insert_resource(CmdQueue::test());
        app.init_resource::<ecsdk_core::AppExit>();
        app
    }

    fn spawn_vm(app: &mut App, sm: StateMachine, initial: impl Component + Clone) -> Entity {
        app.world_mut().spawn((initial, sm)).id()
    }

    #[test]
    fn first_boot_virgin_to_downloading() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, super::super::first_boot::build_sm(), Virgin);
        app.update();
        assert!(app.world().get::<DownloadingImage>(entity).is_some());
        assert_eq!(
            app.world().get::<VmPhase>(entity),
            Some(&VmPhase::DownloadingImage),
        );
    }

    #[test]
    fn first_boot_full_happy_path() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, super::super::first_boot::build_sm(), Virgin);
        app.update();
        assert!(app.world().get::<DownloadingImage>(entity).is_some());
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<Preparing>(entity).is_some());
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<Booting>(entity).is_some());
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<ConnectingAgent>(entity).is_some());
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<Provisioning>(entity).is_some());
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<StartingServices>(entity).is_some());
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<Running>(entity).is_some());
        assert_eq!(app.world().get::<VmPhase>(entity), Some(&VmPhase::Running));
    }

    #[test]
    fn first_boot_failure_goes_to_failed() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, super::super::first_boot::build_sm(), Virgin);
        app.update();
        app.world_mut().entity_mut(entity).insert(Done::Failure);
        app.update();
        assert!(app.world().get::<Failed>(entity).is_some());
        assert_eq!(app.world().get::<VmPhase>(entity), Some(&VmPhase::Failed));
    }

    #[test]
    fn shutdown_from_running() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, super::super::first_boot::build_sm(), Virgin);
        app.update();
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<Running>(entity).is_some());
        app.world_mut().resource_mut::<ShutdownRequested>().0 = true;
        app.update();
        assert!(app.world().get::<ShuttingDown>(entity).is_some());
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<Stopped>(entity).is_some());
        assert_eq!(app.world().get::<VmPhase>(entity), Some(&VmPhase::Stopped));
    }

    #[test]
    fn first_boot_shutdown_before_running_destroys() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, super::super::first_boot::build_sm(), Virgin);
        app.world_mut().resource_mut::<ShutdownRequested>().0 = true;
        app.update();
        assert!(app.world().get::<Destroying>(entity).is_some());
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<Destroyed>(entity).is_some());
        assert_eq!(
            app.world().get::<VmPhase>(entity),
            Some(&VmPhase::Destroyed)
        );
    }

    #[test]
    fn reboot_shutdown_before_running_destroys() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, super::super::reboot::build_sm(), Booting);
        app.world_mut().resource_mut::<ShutdownRequested>().0 = true;
        app.update();
        assert!(app.world().get::<Destroying>(entity).is_some());
    }

    #[test]
    fn reattach_shutdown_before_running_destroys() {
        let mut app = test_app();
        let entity = spawn_vm(
            &mut app,
            super::super::reattach::build_sm(),
            StartingServices,
        );
        app.world_mut().resource_mut::<ShutdownRequested>().0 = true;
        app.update();
        assert!(app.world().get::<Destroying>(entity).is_some());
    }

    #[test]
    fn reprovision_shutdown_before_running_destroys() {
        let mut app = test_app();
        let entity = spawn_vm(
            &mut app,
            super::super::reprovision::build_sm(),
            Provisioning,
        );
        app.world_mut().resource_mut::<ShutdownRequested>().0 = true;
        app.update();
        assert!(app.world().get::<Destroying>(entity).is_some());
    }

    #[test]
    fn reboot_happy_path() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, super::super::reboot::build_sm(), Booting);
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<ConnectingAgent>(entity).is_some());
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<Provisioning>(entity).is_some());
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<Running>(entity).is_some());
    }

    #[test]
    fn destroy_happy_path() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, super::super::destroy::build_sm(), Destroying);
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update();
        assert!(app.world().get::<Destroyed>(entity).is_some());
    }
}
