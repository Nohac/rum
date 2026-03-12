pub mod destroy;
pub mod first_boot;
pub mod reattach;
pub mod reboot;
pub mod reprovision;
pub mod shutdown;

use std::path::PathBuf;

use bevy::app::prelude::*;
use bevy::ecs::prelude::*;
use bevy_replicon::prelude::Replicated;
use ecsdk_core::ApplyMessage;
use ecsdk_tasks::{SpawnTask, TaskQueue};
use seldom_state::prelude::*;

use crate::agent::AgentClient;
use crate::config::SystemConfig;
use crate::phase::vm_phase::*;
use crate::phase::{FlowIntent, ShutdownRequested, VmPhase};

type Tq = TaskQueue<RumMessage>;

// ── Components ──────────────────────────────────────────────────

#[derive(Component)]
pub struct VmConfig(pub SystemConfig);

#[derive(Component)]
pub struct BaseImagePath(pub PathBuf);

#[derive(Component)]
pub struct VsockCid(pub u32);

#[derive(Component)]
pub struct AgentHandle(pub AgentClient);

#[derive(Component)]
pub struct ScriptQueue {
    pub scripts: Vec<String>,
    pub current: usize,
}

impl ScriptQueue {
    pub fn new(scripts: Vec<String>) -> Self {
        Self {
            scripts,
            current: 0,
        }
    }

    pub fn current_name(&self) -> Option<&str> {
        self.scripts.get(self.current).map(|s| s.as_str())
    }

    pub fn advance(&mut self) -> bool {
        self.current += 1;
        self.current < self.scripts.len()
    }
}

#[derive(Component, serde::Serialize, serde::Deserialize)]
pub struct VmError(pub String);

// ── Messages ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum RumMessage {
    /// Spawn the VM entity with all components + state machine.
    SpawnVm(Box<SpawnVmData>),

    /// Mark a phase as done (success or failure).
    MarkDone { entity: Entity, success: bool },

    /// Request graceful shutdown (Ctrl+C).
    RequestShutdown,

    /// Request force stop (second Ctrl+C).
    RequestForceStop,
}

#[derive(Clone, Debug)]
pub struct SpawnVmData {
    pub sys_config: SystemConfig,
    pub intent: FlowIntent,
    pub initial_phase: VmPhase,
    pub scripts: Vec<String>,
    pub total_steps: usize,
}

impl ApplyMessage for RumMessage {
    fn apply(&self, world: &mut World) {
        match self {
            Self::SpawnVm(data) => {
                let SpawnVmData {
                    sys_config,
                    intent,
                    initial_phase,
                    scripts,
                    total_steps,
                } = data.as_ref();

                let initial_marker = phase_marker(*initial_phase);
                let sm = build_sm_for_intent(*intent);

                let mut entity = world.spawn((
                    VmConfig(sys_config.clone()),
                    *intent,
                    *initial_phase,
                    sm,
                    ScriptQueue::new(scripts.clone()),
                    crate::render::StepProgress {
                        current: 0,
                        total: *total_steps,
                    },
                    Replicated,
                ));

                // Insert the initial phase marker component to kick off the SM
                initial_marker(&mut entity);
            }

            Self::MarkDone { entity, success } => {
                if let Ok(mut e) = world.get_entity_mut(*entity) {
                    if *success {
                        e.insert(Done::Success);
                    } else {
                        e.insert(Done::Failure);
                    }
                }
            }

            Self::RequestShutdown => {
                world.resource_mut::<ShutdownRequested>().0 = true;
            }

            Self::RequestForceStop => {
                // Force stop: immediately exit the app
                world.resource_mut::<ShutdownRequested>().0 = true;
                world.resource_mut::<ecsdk_core::AppExit>().0 = true;
            }
        }
    }
}

/// Returns a closure that inserts the appropriate marker component for a VmPhase.
fn phase_marker(phase: VmPhase) -> fn(&mut EntityWorldMut) {
    match phase {
        VmPhase::Virgin => |e| {
            e.insert(Virgin);
        },
        VmPhase::DownloadingImage => |e| {
            e.insert(DownloadingImage);
        },
        VmPhase::Preparing => |e| {
            e.insert(Preparing);
        },
        VmPhase::Booting => |e| {
            e.insert(Booting);
        },
        VmPhase::ConnectingAgent => |e| {
            e.insert(ConnectingAgent);
        },
        VmPhase::Provisioning => |e| {
            e.insert(Provisioning);
        },
        VmPhase::StartingServices => |e| {
            e.insert(StartingServices);
        },
        VmPhase::Running => |e| {
            e.insert(Running);
        },
        VmPhase::ShuttingDown => |e| {
            e.insert(ShuttingDown);
        },
        VmPhase::Destroying => |e| {
            e.insert(Destroying);
        },
        VmPhase::Stopped => |e| {
            e.insert(Stopped);
        },
        VmPhase::Destroyed => |e| {
            e.insert(Destroyed);
        },
        VmPhase::Failed => |e| {
            e.insert(Failed);
        },
    }
}

// ── State machine routing ───────────────────────────────────────

pub fn build_sm_for_intent(intent: FlowIntent) -> StateMachine {
    match intent {
        FlowIntent::FirstBoot => first_boot::build_sm(),
        FlowIntent::Reboot => reboot::build_sm(),
        FlowIntent::Reattach => reattach::build_sm(),
        FlowIntent::Shutdown => shutdown::build_sm(),
        FlowIntent::Destroy => destroy::build_sm(),
        FlowIntent::Reprovision => reprovision::build_sm(),
    }
}

// ── Transition conditions ───────────────────────────────────────

pub(crate) fn always(In(_entity): In<Entity>) -> bool {
    true
}

pub(crate) fn shutdown_requested(In(_entity): In<Entity>, flag: Res<ShutdownRequested>) -> bool {
    flag.0
}

// ── Phase-entry observers ───────────────────────────────────────

fn on_downloading_image(
    trigger: On<Insert, DownloadingImage>,
    mut commands: Commands,
    configs: Query<&VmConfig>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let base = config.0.config.image.base.clone();

    commands
        .entity(entity)
        .spawn_task(move |cmd: Tq| async move {
            let entity = cmd.entity();
            let cache = crate::paths::cache_dir();
            match crate::workers::ensure_image(&base, &cache).await {
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
                            .insert((VmError(msg), Done::Failure));
                    })
                    .wake();
                }
            }
        });
}

fn on_preparing(
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
        commands
            .entity(entity)
            .insert((VmError("base image not available".into()), Done::Failure));
        return;
    };
    let sc = config.0.clone();
    let base_path = base_image.0.clone();

    commands
        .entity(entity)
        .spawn_task(move |cmd: Tq| async move {
            let entity = cmd.entity();
            match crate::workers::prepare_vm(&sc, &base_path).await {
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
                            .insert((VmError(msg), Done::Failure));
                    })
                    .wake();
                }
            }
        });
}

fn on_booting(trigger: On<Insert, Booting>, mut commands: Commands, configs: Query<&VmConfig>) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let sc = config.0.clone();

    commands
        .entity(entity)
        .spawn_task(move |cmd: Tq| async move {
            let entity = cmd.entity();
            match crate::workers::boot_vm(&sc).await {
                Ok(cid) => {
                    cmd.send(move |world: &mut World| {
                        world.entity_mut(entity).insert(VsockCid(cid));
                        world.entity_mut(entity).insert(Done::Success);
                    })
                    .wake();
                }
                Err(e) => {
                    let msg = e.to_string();
                    cmd.send(move |world: &mut World| {
                        world
                            .entity_mut(entity)
                            .insert((VmError(msg), Done::Failure));
                    })
                    .wake();
                }
            }
        });
}

fn on_connecting_agent(
    trigger: On<Insert, ConnectingAgent>,
    mut commands: Commands,
    cids: Query<&VsockCid>,
) {
    let entity = trigger.event_target();
    let Ok(cid) = cids.get(entity) else {
        commands
            .entity(entity)
            .insert((VmError("vsock CID not available".into()), Done::Failure));
        return;
    };
    let cid = cid.0;

    commands
        .entity(entity)
        .spawn_task(move |cmd: Tq| async move {
            let entity = cmd.entity();
            match crate::workers::connect_agent(cid).await {
                Ok(client) => {
                    cmd.send(move |world: &mut World| {
                        world.entity_mut(entity).insert(AgentHandle(client));
                        world.entity_mut(entity).insert(Done::Success);
                    })
                    .wake();
                }
                Err(e) => {
                    let msg = e.to_string();
                    cmd.send(move |world: &mut World| {
                        world
                            .entity_mut(entity)
                            .insert((VmError(msg), Done::Failure));
                    })
                    .wake();
                }
            }
        });
}

fn on_provisioning(
    trigger: On<Insert, Provisioning>,
    mut commands: Commands,
    configs: Query<&VmConfig>,
    agents: Query<&AgentHandle>,
    scripts: Query<&ScriptQueue>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let Ok(agent) = agents.get(entity) else {
        // No agent — skip provisioning (reattach case)
        commands.entity(entity).insert(Done::Success);
        return;
    };

    let Ok(script_queue) = scripts.get(entity) else {
        // No scripts to run
        commands.entity(entity).insert(Done::Success);
        return;
    };

    if script_queue.scripts.is_empty() {
        commands.entity(entity).insert(Done::Success);
        return;
    }

    let sc = config.0.clone();
    let agent_client = agent.0.clone();
    let all_scripts = script_queue.scripts.clone();

    commands
        .entity(entity)
        .spawn_task(move |cmd: Tq| async move {
            let entity = cmd.entity();

            // Build provision scripts from names
            let mut provision_scripts = Vec::new();
            for name in &all_scripts {
                if let Some(script) = build_provision_script(&sc, name) {
                    provision_scripts.push(script);
                }
            }

            if provision_scripts.is_empty() {
                cmd.send(move |world: &mut World| {
                    world.entity_mut(entity).insert(Done::Success);
                })
                .wake();
                return;
            }

            let logs_dir = crate::paths::logs_dir(&sc.id, sc.name.as_deref());
            match crate::workers::run_provision(&agent_client, provision_scripts, &logs_dir).await {
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
                            .insert((VmError(msg), Done::Failure));
                    })
                    .wake();
                }
            }
        });
}

fn on_starting_services(
    trigger: On<Insert, StartingServices>,
    mut commands: Commands,
    configs: Query<&VmConfig>,
    cids: Query<&VsockCid>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let cid = cids.get(entity).ok().map(|c| c.0);
    let sc = config.0.clone();

    commands
        .entity(entity)
        .spawn_task(move |cmd: Tq| async move {
            let entity = cmd.entity();
            let Some(cid) = cid else {
                // No vsock CID — skip services (non-fatal)
                cmd.send(move |world: &mut World| {
                    world.entity_mut(entity).insert(Done::Success);
                })
                .wake();
                return;
            };
            match crate::workers::start_services(cid, &sc).await {
                Ok(_handles) => {
                    cmd.send(move |world: &mut World| {
                        world.entity_mut(entity).insert(Done::Success);
                    })
                    .wake();
                }
                Err(e) => {
                    tracing::warn!("failed to start services: {e}");
                    // Non-fatal — still mark success
                    cmd.send(move |world: &mut World| {
                        world.entity_mut(entity).insert(Done::Success);
                    })
                    .wake();
                }
            }
        });
}

fn on_shutting_down(
    trigger: On<Insert, ShuttingDown>,
    mut commands: Commands,
    configs: Query<&VmConfig>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let sc = config.0.clone();

    commands
        .entity(entity)
        .spawn_task(move |cmd: Tq| async move {
            let entity = cmd.entity();
            if let Err(e) = crate::workers::shutdown_vm(&sc).await {
                tracing::warn!("shutdown failed: {e}");
            }
            cmd.send(move |world: &mut World| {
                world.entity_mut(entity).insert(Done::Success);
            })
            .wake();
        });
}

fn on_destroying(
    trigger: On<Insert, Destroying>,
    mut commands: Commands,
    configs: Query<&VmConfig>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let sc = config.0.clone();

    commands
        .entity(entity)
        .spawn_task(move |cmd: Tq| async move {
            let entity = cmd.entity();
            match crate::workers::destroy_vm(&sc).await {
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
                            .insert((VmError(msg), Done::Failure));
                    })
                    .wake();
                }
            }
        });
}

// ── Step progress tracking ──────────────────────────────────────

fn advance_step_progress(mut query: Query<&mut crate::render::StepProgress, Changed<VmPhase>>) {
    for mut progress in &mut query {
        progress.current += 1;
    }
}

// ── Terminal state observers ────────────────────────────────────

fn on_stopped(_trigger: On<Insert, Stopped>, mut exit: ResMut<ecsdk_core::AppExit>) {
    exit.0 = true;
}

fn on_destroyed(_trigger: On<Insert, Destroyed>, mut exit: ResMut<ecsdk_core::AppExit>) {
    exit.0 = true;
}

fn on_failed(
    trigger: On<Insert, Failed>,
    errors: Query<&VmError>,
    mut exit: ResMut<ecsdk_core::AppExit>,
) {
    let entity = trigger.event_target();
    if let Ok(err) = errors.get(entity) {
        tracing::error!("VM failed: {}", err.0);
    }
    exit.0 = true;
}

// ── Build provision script helper ───────────────────────────────

fn build_provision_script(
    sys_config: &crate::config::SystemConfig,
    name: &str,
) -> Option<rum_agent::ProvisionScript> {
    let config = &sys_config.config;
    match name {
        "rum-drives" => {
            let drives = sys_config.resolve_drives().ok()?;
            let resolved_fs = sys_config.resolve_fs(&drives).ok()?;
            if resolved_fs.is_empty() {
                return None;
            }
            Some(rum_agent::ProvisionScript {
                name: "rum-drives".into(),
                title: "Setting up drives and filesystems".into(),
                content: crate::cloudinit::build_drive_script(&resolved_fs),
                order: 0,
                run_on: rum_agent::RunOn::System,
            })
        }
        "rum-system" => {
            let system = config.provision.system.as_ref()?;
            Some(rum_agent::ProvisionScript {
                name: "rum-system".into(),
                title: "Running system provisioning".into(),
                content: system.script.clone(),
                order: 1,
                run_on: rum_agent::RunOn::System,
            })
        }
        "rum-boot" => {
            let boot = config.provision.boot.as_ref()?;
            Some(rum_agent::ProvisionScript {
                name: "rum-boot".into(),
                title: "Running boot provisioning".into(),
                content: boot.script.clone(),
                order: 2,
                run_on: rum_agent::RunOn::Boot,
            })
        }
        _ => None,
    }
}

// ── Plugins ─────────────────────────────────────────────────────

/// State machines only, no observers. For pure state machine tests.
pub struct LifecycleTestPlugin;

impl Plugin for LifecycleTestPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(StateMachinePlugin::default().schedule(PreUpdate));
        app.init_resource::<ShutdownRequested>();
    }
}

/// Full lifecycle plugin: state machines + phase-entry observers.
pub struct LifecyclePlugin;

impl Plugin for LifecyclePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(StateMachinePlugin::default().schedule(PreUpdate));
        app.init_resource::<ShutdownRequested>();

        // Step progress tracking (runs between SM transitions and render)
        app.add_systems(Update, advance_step_progress);

        // Phase entry observers
        app.add_observer(on_downloading_image);
        app.add_observer(on_preparing);
        app.add_observer(on_booting);
        app.add_observer(on_connecting_agent);
        app.add_observer(on_provisioning);
        app.add_observer(on_starting_services);
        app.add_observer(on_shutting_down);
        app.add_observer(on_destroying);

        // Terminal state observers
        app.add_observer(on_stopped);
        app.add_observer(on_destroyed);
        app.add_observer(on_failed);
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phase::VmPhase;
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
        let entity = spawn_vm(&mut app, first_boot::build_sm(), Virgin);

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
        let entity = spawn_vm(&mut app, first_boot::build_sm(), Virgin);

        app.update(); // Virgin → DownloadingImage
        assert!(app.world().get::<DownloadingImage>(entity).is_some());

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // DownloadingImage → Preparing
        assert!(app.world().get::<Preparing>(entity).is_some());

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // Preparing → Booting
        assert!(app.world().get::<Booting>(entity).is_some());

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // Booting → ConnectingAgent
        assert!(app.world().get::<ConnectingAgent>(entity).is_some());

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // ConnectingAgent → Provisioning
        assert!(app.world().get::<Provisioning>(entity).is_some());

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // Provisioning → StartingServices
        assert!(app.world().get::<StartingServices>(entity).is_some());

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // StartingServices → Running
        assert!(app.world().get::<Running>(entity).is_some());
        assert_eq!(app.world().get::<VmPhase>(entity), Some(&VmPhase::Running),);
    }

    #[test]
    fn first_boot_failure_goes_to_failed() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, first_boot::build_sm(), Virgin);

        app.update(); // Virgin → DownloadingImage

        app.world_mut().entity_mut(entity).insert(Done::Failure);
        app.update(); // DownloadingImage → Failed

        assert!(app.world().get::<Failed>(entity).is_some());
        assert_eq!(app.world().get::<VmPhase>(entity), Some(&VmPhase::Failed),);
    }

    #[test]
    fn shutdown_from_running() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, first_boot::build_sm(), Virgin);

        // Drive to Running
        app.update(); // → DownloadingImage
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // → Preparing
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // → Booting
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // → ConnectingAgent
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // → Provisioning
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // → StartingServices
        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // → Running

        assert!(app.world().get::<Running>(entity).is_some());

        // Trigger shutdown
        app.world_mut().resource_mut::<ShutdownRequested>().0 = true;
        app.update(); // Running → ShuttingDown

        assert!(app.world().get::<ShuttingDown>(entity).is_some());

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // ShuttingDown → Stopped

        assert!(app.world().get::<Stopped>(entity).is_some());
        assert_eq!(app.world().get::<VmPhase>(entity), Some(&VmPhase::Stopped),);
    }

    #[test]
    fn reboot_happy_path() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, reboot::build_sm(), Booting);

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // Booting → ConnectingAgent
        assert!(app.world().get::<ConnectingAgent>(entity).is_some());

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // → Provisioning
        assert!(app.world().get::<Provisioning>(entity).is_some());

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // → StartingServices

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // → Running
        assert!(app.world().get::<Running>(entity).is_some());
    }

    #[test]
    fn destroy_happy_path() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, destroy::build_sm(), Destroying);

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // Destroying → Destroyed

        assert!(app.world().get::<Destroyed>(entity).is_some());
        assert_eq!(
            app.world().get::<VmPhase>(entity),
            Some(&VmPhase::Destroyed),
        );
    }

    #[test]
    fn reattach_happy_path() {
        let mut app = test_app();
        let entity = spawn_vm(&mut app, reattach::build_sm(), StartingServices);

        app.world_mut().entity_mut(entity).insert(Done::Success);
        app.update(); // StartingServices → Running

        assert!(app.world().get::<Running>(entity).is_some());
    }
}
