use ecsdk::app::AsyncApp;
use ecsdk::network::{IsomorphicApp, IsomorphicAppExt};
use ecsdk::prelude::*;

use orchestrator::{
    ManagedInstanceSpec, OrchestratorMessage, OrchestratorPlugin, OrchestrationDriver,
    spawn_managed_instance,
};

/// Install the orchestrator plugin for one driver type and spawn a managed
/// instance entity into an existing app.
pub fn bootstrap_instance<D: OrchestrationDriver>(
    app: &mut App,
    spec: ManagedInstanceSpec<D>,
) -> Entity {
    app.add_shared_plugin(OrchestratorPlugin::<D>::default());
    spawn_managed_instance(app.world_mut(), spec)
}

/// Build the first CLI-facing async app for one managed instance.
///
/// The resulting app is still intentionally narrow: it bootstraps the
/// orchestrator plugin and inserts one managed instance entity. Argument
/// parsing, rendering, and transport can layer on top later without changing
/// the orchestrator crate.
pub fn build_instance_app<D: OrchestrationDriver>(
    spec: ManagedInstanceSpec<D>,
) -> AsyncApp<OrchestratorMessage> {
    let mut app = IsomorphicApp::<OrchestratorMessage>::new().build_server();
    bootstrap_instance(&mut app, spec);
    app
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use async_trait::async_trait;
    use machine::driver::{Driver, RecoverableDriver};
    use machine::error::Error;
    use machine::instance::{BackendKind, Instance, InstanceState};
    use orchestrator::{InstancePhase, ManagedInstance};

    use super::*;

    #[derive(Clone)]
    struct MockDriver;

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
            Ok(())
        }

        async fn boot(&self) -> Result<u32, Self::Error> {
            Ok(7)
        }

        async fn shutdown(&self) -> Result<(), Self::Error> {
            Ok(())
        }

        async fn destroy(&self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    impl RecoverableDriver for MockDriver {
        fn recover(&self) -> Result<InstanceState, Self::Error> {
            Ok(InstanceState::Missing)
        }
    }

    #[async_trait]
    impl OrchestrationDriver for MockDriver {
        async fn connect_guest(&self) -> Result<(), Error> {
            Ok(())
        }

        async fn provision(
            &self,
            _scripts: Vec<guest::agent::ProvisionScript>,
        ) -> Result<(), Error> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bootstrap_spawns_a_managed_instance_entity() {
        let spec = ManagedInstanceSpec::new(Instance::new_with_driver(
            MockDriver,
            BackendKind::Libvirt,
        ));

        let mut app = build_instance_app(spec);
        let world = app.world_mut();

        let mut query = world.query::<(&ManagedInstance<MockDriver>, &InstancePhase)>();
        let mut items = query.iter(world);
        assert!(items.next().is_some());
        assert!(items.next().is_none());
    }
}
