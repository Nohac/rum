use std::path::{Path, PathBuf};

use ecsdk::app::AsyncApp;
use ecsdk::network::IsomorphicAppExt;
use ecsdk::prelude::*;
use machine::config::{SystemConfig, load_config};
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
pub fn build_up_server(spec: ServerSpec) -> AsyncApp<OrchestratorMessage> {
    let iso = crate::app::create_isomorphic_app(spec.socket_path);
    let mut app = iso.build_server();
    app.add_shared_plugin(OrchestratorPlugin::<LibvirtDriver>::default());
    app.add_plugins(RumServerPlugin);
    spawn_managed_instance(app.world_mut(), spec.managed_instance);
    app
}

/// Server-side observers for daemon lifecycle behavior.
struct RumServerPlugin;

impl Plugin for RumServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(exit_on_stopped_after_shutdown);
        app.add_observer(exit_on_failed);
    }
}

fn exit_on_stopped_after_shutdown(
    _trigger: On<Add, Stopped>,
    shutdown: Res<ShutdownRequested>,
    mut exit: MessageWriter<AppExit>,
) {
    if shutdown.0 {
        tracing::info!("managed instance stopped after shutdown request; exiting daemon");
        exit.write(AppExit::Success);
    }
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
