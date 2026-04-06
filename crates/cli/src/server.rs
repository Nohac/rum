use std::path::{Path, PathBuf};

use ecsdk::app::AsyncApp;
use ecsdk::network::IsomorphicApp;
use machine::config::{SystemConfig, load_config};
use machine::driver::LibvirtDriver;
use machine::image::ensure_base_image;
use machine::instance::Instance;
use machine::{error::Error, paths};
use orchestrator::{ManagedInstanceSpec, OrchestratorMessage};

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
    let mut iso = IsomorphicApp::<OrchestratorMessage>::new();
    iso.add_plugin(crate::network::SharedNetworkPlugin::new(spec.socket_path));
    let mut app = iso.build_server();
    crate::bootstrap::bootstrap_instance(&mut app, spec.managed_instance);
    app
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
