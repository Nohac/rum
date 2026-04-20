use std::path::{Path, PathBuf};
use ecsdk::app::AsyncApp;
use ecsdk::network::IsomorphicAppExt;
use ecsdk::prelude::*;
use ecsdk::tasks::SpawnTask;
use machine::config::{SystemConfig, load_config};
use machine::driver::Driver;
use machine::driver::LibvirtDriver;
use machine::image::ensure_base_image;
use machine::instance::Instance;
use machine::{error::Error, paths};
use orchestrator::instance::instance_phase::{Failed, Stopped};
use orchestrator::{
    ManagedInstanceSpec, OrchestratorMessage, OrchestratorPlugin, ShutdownRequested,
    spawn_managed_instance,
};

/// Server bootstrap inputs resolved before the daemon starts.
///
/// The daemon process receives fully resolved startup inputs so the running ECS
/// app can stay focused on orchestration instead of config parsing and image
/// lookup.
pub struct ServerSpec {
    pub system: SystemConfig,
    pub socket_path: PathBuf,
    pub managed_instance: ManagedInstanceSpec<LibvirtDriver>,
}

/// Resolve config and startup inputs for a single `rum up` daemon.
pub async fn load_server_spec(config_path: &Path) -> Result<ServerSpec, Error> {
    let system = load_config(config_path)?;
    let display_name = system.display_name().to_string();
    let instance = Instance::new(system.clone());
    let base_image = ensure_base_image(&system.config.image.base, &paths::cache_dir()).await?;
    let socket_path = crate::ipc::socket_path(&system);
    let provision_plan = build_provision_plan(&system);

    Ok(ServerSpec {
        system,
        socket_path,
        managed_instance: ManagedInstanceSpec::new(instance)
            .with_label(display_name)
            .with_resolved_base_image(base_image)
            .with_provision_plan(provision_plan),
    })
}

/// Build the first server-side daemon app for `rum up`.
pub fn build_up_server(
    iso: ecsdk::network::IsomorphicApp<OrchestratorMessage>,
    spec: ServerSpec,
) -> AsyncApp<OrchestratorMessage> {
    let mut app = iso.build_server();
    app.add_isomorphic_plugin(
        ecsdk::network::AppRole::Server,
        OrchestratorPlugin::<LibvirtDriver>::default(),
    );
    app.add_plugins(RumServerPlugin);
    spawn_managed_instance(app.world_mut(), spec.managed_instance);
    app
}

/// Server-side observers for daemon lifecycle behavior.
struct RumServerPlugin;

impl Plugin for RumServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DestroyRequested>();
        app.add_observer(exit_on_stopped_after_shutdown);
        app.add_observer(destroy_after_stop);
        app.add_observer(exit_on_failed);
    }
}

/// Resource toggled when the daemon should purge instance state after the
/// managed runtime has stopped.
#[derive(Resource, Default)]
pub struct DestroyRequested(pub bool);

fn exit_on_stopped_after_shutdown(
    _trigger: On<Add, Stopped>,
    shutdown: Res<ShutdownRequested>,
    destroy: Res<DestroyRequested>,
    mut exit: MessageWriter<AppExit>,
) {
    if shutdown.0 && !destroy.0 {
        tracing::info!("managed instance stopped after shutdown request; exiting daemon");
        exit.write(AppExit::Success);
    }
}

fn destroy_after_stop(
    _trigger: On<Add, Stopped>,
    destroy: Res<DestroyRequested>,
    instances: Query<&orchestrator::ManagedInstance<LibvirtDriver>>,
    mut commands: Commands,
) {
    if !destroy.0 {
        return;
    }

    let Some(instance) = instances.iter().next() else {
        tracing::warn!("destroy was requested after stop, but no managed instance was found");
        commands.insert_resource(DestroyRequested(false));
        return;
    };

    let driver = instance.driver();
    commands.insert_resource(DestroyRequested(false));
    commands.spawn_empty().spawn_task(move |task| async move {
        match driver.destroy().await {
            Ok(()) => {
                task.queue_cmd_wake(|world: &mut World| {
                    tracing::info!("managed instance destroyed after shutdown; exiting daemon");
                    world.write_message(AppExit::Success);
                });
            }
            Err(error) => {
                task.queue_cmd_wake(move |world: &mut World| {
                    tracing::error!(error = %error, "failed to destroy managed instance after shutdown");
                    world.write_message(AppExit::Success);
                });
            }
        }
    });
}

fn exit_on_failed(_trigger: On<Add, Failed>, mut exit: MessageWriter<AppExit>) {
    tracing::error!("managed instance failed; exiting daemon");
    exit.write(AppExit::Success);
}

fn build_provision_plan(system: &SystemConfig) -> Vec<guest::agent::ProvisionScript> {
    let mut scripts = Vec::new();

    if let Some(provision) = &system.config.provision.system {
        scripts.push(guest::agent::ProvisionScript {
            name: "system".into(),
            title: "System provisioning".into(),
            content: provision.script.clone(),
            order: 0,
            run_on: guest::agent::RunOn::System,
        });
    }

    if let Some(provision) = &system.config.provision.boot {
        scripts.push(guest::agent::ProvisionScript {
            name: "boot".into(),
            title: "Boot provisioning".into(),
            content: provision.script.clone(),
            order: 100,
            run_on: guest::agent::RunOn::Boot,
        });
    }

    scripts
}
