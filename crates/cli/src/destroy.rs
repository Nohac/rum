use ecsdk::app::AsyncApp;
use ecsdk::prelude::*;
use ecsdk::tasks::SpawnTask;
use machine::driver::Driver;
use machine::driver::LibvirtDriver;
use orchestrator::instance::instance_phase::Failed;
use orchestrator::{EntityError, InstancePhase, OrchestratorMessage};
use crate::render::{RenderMode, RumRenderPlugin};
use orchestrator::instance::ManagedInstance;

use crate::protocol::{DestroyRequest, DestroyResponse};

/// Isomorphic request feature that lets a client ask the daemon to destroy the
/// managed machine and purge its persisted state.
pub struct DestroyFeature;

impl RequestPlugin for DestroyFeature {
    type Request = DestroyRequest;
    type Trigger = ecsdk::network::InitialConnection;

    fn auto_register_client() -> bool {
        false
    }

    fn build_server(app: &mut App) {
        app.add_observer(handle_destroy_request);
    }

    fn build_client(app: &mut App) {
        app.add_observer(handle_destroy_response);
    }
}

/// Build the client app used by `rum destroy`.
pub fn build_destroy_client(
    socket_path: std::path::PathBuf,
    render_mode: RenderMode,
) -> AsyncApp<OrchestratorMessage> {
    let iso = crate::app::create_isomorphic_app(socket_path);
    let mut app = iso.build_client();
    DestroyFeature::register_client(&mut app);
    app.add_plugins((RumRenderPlugin::new(render_mode), RumDestroyClientPlugin));
    app
}

struct RumDestroyClientPlugin;

impl Plugin for RumDestroyClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_failed);
        app.add_systems(Update, on_server_disconnect);
    }
}

fn handle_destroy_request(
    trigger: On<FromClient<DestroyRequest>>,
    instances: Query<(Entity, &ManagedInstance<LibvirtDriver>)>,
    phases: Query<&InstancePhase>,
    mut commands: Commands,
) {
    let Some((entity, instance)) = instances.iter().next() else {
        DestroyRequest::reply(
            &mut commands,
            trigger.event().client_id,
            DestroyResponse { accepted: false },
        );
        return;
    };
    let phase = phases.get(entity).ok().copied();

    DestroyRequest::reply(
        &mut commands,
        trigger.event().client_id,
        DestroyResponse { accepted: true },
    );

    match phase {
        Some(InstancePhase::Running) => {
            commands.insert_resource(crate::server::DestroyRequested(true));
            commands.send_msg(OrchestratorMessage::RequestShutdown);
        }
        _ => {
            let driver = instance.driver();
            commands.spawn_empty().spawn_task(move |task| async move {
                match driver.destroy().await {
                    Ok(()) => {
                        task.queue_cmd_wake(|world: &mut World| {
                            tracing::info!("managed instance destroyed; exiting daemon");
                            world.write_message(AppExit::Success);
                        });
                    }
                    Err(error) => {
                        task.queue_cmd_wake(move |world: &mut World| {
                            tracing::error!(error = %error, "failed to destroy managed instance");
                            world.write_message(AppExit::Success);
                        });
                    }
                }
            });
        }
    }
}

fn handle_destroy_response(trigger: On<DestroyResponse>) {
    if trigger.event().accepted {
        tracing::info!("destroy request accepted");
    } else {
        tracing::warn!("destroy request rejected because no managed instance was found");
    }
}

fn on_server_disconnect(
    mut disconnects: MessageReader<ServerDisconnected>,
    mut exit: MessageWriter<AppExit>,
) {
    if disconnects.read().next().is_some() {
        tracing::info!("rum daemon disconnected");
        exit.write(AppExit::Success);
    }
}

fn on_failed(
    trigger: On<Add, Failed>,
    errors: Query<&EntityError>,
    mut exit: MessageWriter<AppExit>,
) {
    let entity = trigger.event_target();
    if let Ok(error) = errors.get(entity) {
        tracing::error!(entity = entity.index().index(), error = %error.0, "managed instance failed");
    } else {
        tracing::error!(entity = entity.index().index(), "managed instance failed");
    }
    exit.write(AppExit::Success);
}
