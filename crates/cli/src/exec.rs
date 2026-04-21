use ecsdk::app::AsyncApp;
use ecsdk::network::{InitialConnection, IsomorphicPlugin};
use ecsdk::prelude::*;
use ecsdk::tasks::SpawnTask;
use machine::driver::LibvirtDriver;
use machine::guest::VsockConnector;
use orchestrator::{
    LogBuffer, ManagedInstance, OrchestratorMessage, ProvisionLogView,
};

use crate::protocol::{ExecRequest, ExecResponse};

/// Shared request feature for daemon-backed guest command execution.
pub struct ExecFeature;

impl IsomorphicPlugin for ExecFeature {
    fn build_shared(&self, app: &mut App) {
        ExecRequest::register(app);
    }

    fn build_server(&self, app: &mut App) {
        app.add_observer(handle_exec_request);
    }

    fn build_client(&self, app: &mut App) {
        app.add_observer(handle_exec_response);
        app.add_systems(Update, crate::exit::on_server_disconnect);
    }
}

/// Client request state used to send one concrete exec request on the initial
/// daemon connection.
#[derive(Resource, Clone)]
struct PendingExecRequest(ExecRequest);

pub fn prepare_request(command: &[String]) -> anyhow::Result<ExecRequest> {
    if command.is_empty() {
        anyhow::bail!("missing command")
    }

    Ok(ExecRequest {
        command: Some(command.join(" ")),
    })
}

/// Build the client app used by `rum exec`.
pub fn build_exec_client(
    mut app: AsyncApp<OrchestratorMessage>,
    request: ExecRequest,
) -> AsyncApp<OrchestratorMessage> {
    app.insert_resource(PendingExecRequest(request));
    app.add_observer(send_exec_request_on_connect);
    app
}

fn send_exec_request_on_connect(
    _trigger: On<Add, InitialConnection>,
    request: Res<PendingExecRequest>,
    mut commands: Commands,
) {
    commands.client_trigger(request.0.clone());
}

fn handle_exec_request(
    trigger: On<FromClient<ExecRequest>>,
    instances: Query<(Entity, &ManagedInstance<LibvirtDriver>)>,
    views: Query<&ProvisionLogView>,
    mut buffers: Query<&mut LogBuffer>,
    mut commands: Commands,
) {
    let Some((instance_entity, instance)) = instances.iter().next() else {
        ExecRequest::reply(
            &mut commands,
            trigger.event().client_id,
            ExecResponse {
                success: false,
                exit_code: 1,
                message: Some("no managed instance was found".into()),
            },
        );
        return;
    };

    let Some(command) = trigger.event().message.command.clone() else {
        ExecRequest::reply(
            &mut commands,
            trigger.event().client_id,
            ExecResponse {
                success: false,
                exit_code: 1,
                message: Some("missing exec request payload".into()),
            },
        );
        return;
    };

    if let Ok(mut buffer) = buffers.get_mut(instance_entity) {
        buffer.lines.clear();
    }
    if let Ok(entries) = views.get(instance_entity) {
        for entry in entries.iter() {
            commands.entity(entry).despawn();
        }
    }

    let driver = instance.driver();
    let client_id = trigger.event().client_id;
    commands.spawn_empty().spawn_task(move |task| async move {
        let log_task = task.clone();
        let on_output = move |line: String| {
            log_task.queue_cmd_tick(move |world: &mut World| {
                if let Some(mut buffer) = world.get_mut::<LogBuffer>(instance_entity) {
                    buffer.push(line);
                }
            });
        };

        let response = match run_exec(driver, command, on_output).await {
            Ok(exit_code) => ExecResponse {
                success: exit_code == 0,
                exit_code,
                message: None,
            },
            Err(message) => ExecResponse {
                success: false,
                exit_code: 1,
                message: Some(message),
            },
        };

        task.queue_cmd_wake(move |world: &mut World| {
            let mut commands = world.commands();
            ExecRequest::reply(&mut commands, client_id, response);
        });
    });
}

async fn run_exec<F>(
    driver: LibvirtDriver,
    command: String,
    on_output: F,
) -> Result<i32, String>
where
    F: Fn(String) + Send + Sync,
{
    let cid = driver
        .get_vsock_cid()
        .map_err(|error| format!("guest connection is not ready: {error}"))?;
    let client = guest::client::wait_for_agent(VsockConnector::new(cid))
        .await
        .map_err(|error| format!("failed to connect to guest agent: {error}"))?;

    client
        .exec_with_output(command, move |event| on_output(event.message))
        .await
        .map_err(|error| error.to_string())
}

fn handle_exec_response(trigger: On<ExecResponse>, mut exit: MessageWriter<AppExit>) {
    let response = trigger.event();
    if let Some(message) = response.message.as_deref() {
        eprintln!("{message}");
    }

    if response.success {
        exit.write(AppExit::Success);
    } else {
        if response.message.is_none() {
            eprintln!("command exited with status {}", response.exit_code);
        }
        exit.write(AppExit::from_code(1));
    }
}
