use std::path::PathBuf;

use ecsdk::app::AsyncApp;
use ecsdk::network::{InitialConnection, IsomorphicPlugin};
use ecsdk::prelude::*;
use ecsdk::tasks::SpawnTask;
use guest::client::CopyDirection;
use machine::driver::LibvirtDriver;
use machine::guest::VsockConnector;
use orchestrator::ManagedInstance;
use orchestrator::OrchestratorMessage;

use crate::protocol::{CopyRequest, CopyResponse, CopySpec};

/// Shared request feature for daemon-backed guest file copies.
pub struct CopyFeature;

impl IsomorphicPlugin for CopyFeature {
    fn build_shared(&self, app: &mut App) {
        CopyRequest::register(app);
    }

    fn build_server(&self, app: &mut App) {
        app.add_observer(handle_copy_request);
    }

    fn build_client(&self, app: &mut App) {
        app.add_observer(handle_copy_response);
        app.add_systems(Update, crate::exit::on_server_disconnect);
    }
}

/// Client request state used to send one concrete copy request on the initial
/// daemon connection.
#[derive(Resource, Clone)]
struct PendingCopyRequest(CopyRequest);

/// Parse the user-facing `rum cp` arguments and resolve the host-side path to
/// an absolute path before handing control to the daemon.
pub fn prepare_request(src: &str, dst: &str) -> anyhow::Result<CopyRequest> {
    let direction = guest::client::parse_copy_args(src, dst)?;
    let spec = match direction {
        CopyDirection::Upload { local, guest } => CopySpec::Upload {
            local: absolutize_local(local)?,
            guest,
        },
        CopyDirection::Download { guest, local } => CopySpec::Download {
            guest,
            local: absolutize_local(local)?,
        },
    };

    Ok(CopyRequest { spec: Some(spec) })
}

/// Build the client app used by `rum cp`.
pub fn build_cp_client(
    mut app: AsyncApp<OrchestratorMessage>,
    request: CopyRequest,
) -> AsyncApp<OrchestratorMessage> {
    app.insert_resource(PendingCopyRequest(request));
    app.add_observer(send_copy_request_on_connect);
    app
}

fn send_copy_request_on_connect(
    _trigger: On<Add, InitialConnection>,
    request: Res<PendingCopyRequest>,
    mut commands: Commands,
) {
    commands.client_trigger(request.0.clone());
}

fn handle_copy_request(
    trigger: On<FromClient<CopyRequest>>,
    instances: Query<&ManagedInstance<LibvirtDriver>>,
    mut commands: Commands,
) {
    let Some(instance) = instances.iter().next() else {
        CopyRequest::reply(
            &mut commands,
            trigger.event().client_id,
            CopyResponse {
                success: false,
                message: "no managed instance was found".into(),
            },
        );
        return;
    };

    let Some(spec) = trigger.event().message.spec.clone() else {
        CopyRequest::reply(
            &mut commands,
            trigger.event().client_id,
            CopyResponse {
                success: false,
                message: "missing copy request payload".into(),
            },
        );
        return;
    };

    let driver = instance.driver();
    let client_id = trigger.event().client_id;
    commands.spawn_empty().spawn_task(move |task| async move {
        let response = match run_copy(driver, spec).await {
            Ok(message) => CopyResponse {
                success: true,
                message,
            },
            Err(message) => CopyResponse {
                success: false,
                message,
            },
        };

        task.queue_cmd_wake(move |world: &mut World| {
            let mut commands = world.commands();
            CopyRequest::reply(&mut commands, client_id, response);
        });
    });
}

async fn run_copy(driver: LibvirtDriver, spec: CopySpec) -> Result<String, String> {
    let cid = driver
        .get_vsock_cid()
        .map_err(|error| format!("guest connection is not ready: {error}"))?;
    let client = guest::client::wait_for_agent(VsockConnector::new(cid))
        .await
        .map_err(|error| format!("failed to connect to guest agent: {error}"))?;

    match spec {
        CopySpec::Upload { local, guest } => {
            let bytes = guest::client::copy_to_guest(&client, &local, &guest)
                .await
                .map_err(|error| error.to_string())?;
            Ok(format!(
                "copied {} bytes to guest:{} from {}",
                bytes,
                guest,
                local.display()
            ))
        }
        CopySpec::Download { guest, local } => {
            let bytes = guest::client::copy_from_guest(&client, &guest, &local)
                .await
                .map_err(|error| error.to_string())?;
            Ok(format!(
                "copied {} bytes from guest:{} to {}",
                bytes,
                guest,
                local.display()
            ))
        }
    }
}

fn handle_copy_response(trigger: On<CopyResponse>, mut exit: MessageWriter<AppExit>) {
    let response = trigger.event();
    if response.success {
        println!("{}", response.message);
        exit.write(AppExit::Success);
    } else {
        eprintln!("{}", response.message);
        exit.write(AppExit::from_code(1));
    }
}

fn absolutize_local(path: PathBuf) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }

    Ok(std::env::current_dir()?.join(path))
}
