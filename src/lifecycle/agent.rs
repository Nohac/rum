use std::time::Duration;

use bevy::ecs::prelude::*;
use ecsdk_tasks::{SpawnTask, TaskQueue};
use seldom_state::prelude::*;

use crate::agent::AgentClient;
use crate::phase::vm_phase::*;

type Tq = TaskQueue<super::RumMessage>;

#[derive(Component)]
pub struct VsockCid(pub u32);

#[derive(Component)]
pub struct AgentHandle(pub AgentClient);

pub fn on_connecting_agent(
    trigger: On<Insert, ConnectingAgent>,
    mut commands: Commands,
    cids: Query<&VsockCid>,
) {
    let entity = trigger.event_target();
    let Ok(cid) = cids.get(entity) else {
        commands.entity(entity).insert((
            super::terminal::VmError("vsock CID not available".into()),
            Done::Failure,
        ));
        return;
    };
    let cid = cid.0;
    tracing::debug!(entity = ?entity, vsock_cid = cid, "entered ConnectingAgent");

    commands
        .entity(entity)
        .spawn_task(move |cmd: Tq| async move {
            let entity = cmd.entity();
            tracing::debug!(entity = ?entity, vsock_cid = cid, "starting and waiting for agent");
            match crate::vm::connect_agent(cid).await {
                Ok(client) => {
                    tracing::debug!(entity = ?entity, "agent connection completed");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    cmd.send(move |world: &mut World| {
                        world.entity_mut(entity).insert(AgentHandle(client));
                        world.entity_mut(entity).insert(Done::Success);
                    })
                    .wake();
                }
                Err(e) => {
                    let msg = e.to_string();
                    tracing::debug!(entity = ?entity, error = %msg, "agent connection failed");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    cmd.send(move |world: &mut World| {
                        world
                            .entity_mut(entity)
                            .insert((super::terminal::VmError(msg), Done::Failure));
                    })
                    .wake();
                }
            }
        });
}

pub fn on_starting_services(
    trigger: On<Insert, StartingServices>,
    mut commands: Commands,
    configs: Query<&super::prepare::VmConfig>,
    cids: Query<&VsockCid>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let cid = cids.get(entity).ok().map(|c| c.0);
    let sc = config.0.clone();
    tracing::debug!(
        entity = ?entity,
        vm = sc.display_name(),
        vsock_cid = ?cid,
        "entered StartingServices"
    );

    commands.entity(entity).spawn_task(move |cmd: Tq| async move {
        let entity = cmd.entity();
        let Some(cid) = cid else {
            tracing::debug!(entity = ?entity, "skipping services start because vsock CID is absent");
            cmd.send(move |world: &mut World| {
                world.entity_mut(entity).insert(Done::Success);
            })
            .wake();
            return;
        };
        match crate::vm::services::start_services(cid, &sc).await {
            Ok(_handles) => {
                tracing::debug!(entity = ?entity, "services started");
                cmd.send(move |world: &mut World| {
                    world.entity_mut(entity).insert(Done::Success);
                })
                .wake();
            }
            Err(e) => {
                tracing::warn!("failed to start services: {e}");
                tracing::debug!(entity = ?entity, error = %e, "services failed to start; continuing");
                cmd.send(move |world: &mut World| {
                    world.entity_mut(entity).insert(Done::Success);
                })
                .wake();
            }
        }
    });
}
