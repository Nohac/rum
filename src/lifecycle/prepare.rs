use std::path::PathBuf;

use bevy::ecs::prelude::*;
use ecsdk_tasks::{SpawnTask, TaskQueue};
use seldom_state::prelude::*;

use crate::phase::vm_phase::*;

type Tq = TaskQueue<super::RumMessage>;

#[derive(Component)]
pub struct VmConfig(pub crate::config::SystemConfig);

#[derive(Component)]
pub struct BaseImagePath(pub PathBuf);

pub fn on_downloading_image(
    trigger: On<Insert, DownloadingImage>,
    mut commands: Commands,
    configs: Query<&VmConfig>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let base = config.0.config.image.base.clone();

    commands.entity(entity).spawn_task(move |cmd: Tq| async move {
        let entity = cmd.entity();
        let cache = crate::paths::cache_dir();
        match crate::vm::prepare::ensure_image(&base, &cache).await {
            Ok(path) => {
                cmd.send(move |world: &mut World| {
                    world.entity_mut(entity).insert(BaseImagePath(path));
                    world.entity_mut(entity).insert(Done::Success);
                })
                .wake();
            }
            Err(e) => {
                let msg = e.to_string();
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

pub fn on_preparing(
    trigger: On<Insert, Preparing>,
    mut commands: Commands,
    configs: Query<&VmConfig>,
    images: Query<&BaseImagePath>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let Ok(base_image) = images.get(entity) else {
        commands.entity(entity).insert((
            super::terminal::VmError("base image not available".into()),
            Done::Failure,
        ));
        return;
    };
    let sc = config.0.clone();
    let base_path = base_image.0.clone();

    commands.entity(entity).spawn_task(move |cmd: Tq| async move {
        let entity = cmd.entity();
        match crate::vm::prepare::prepare_vm(&sc, &base_path).await {
            Ok(()) => {
                cmd.send(move |world: &mut World| {
                    world.entity_mut(entity).insert(Done::Success);
                })
                .wake();
            }
            Err(e) => {
                let msg = e.to_string();
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

pub fn on_booting(
    trigger: On<Insert, Booting>,
    mut commands: Commands,
    configs: Query<&VmConfig>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let sc = config.0.clone();

    commands.entity(entity).spawn_task(move |cmd: Tq| async move {
        let entity = cmd.entity();
        match crate::vm::boot::boot_vm(&sc).await {
            Ok(cid) => {
                cmd.send(move |world: &mut World| {
                    world.entity_mut(entity).insert(super::agent::VsockCid(cid));
                    world.entity_mut(entity).insert(Done::Success);
                })
                .wake();
            }
            Err(e) => {
                let msg = e.to_string();
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
