use bevy::ecs::prelude::*;
use ecsdk_tasks::{SpawnTask, TaskQueue};
use seldom_state::prelude::*;

use crate::phase::vm_phase::*;

type Tq = TaskQueue<super::RumMessage>;

#[derive(Component)]
pub struct ScriptQueue {
    pub scripts: Vec<String>,
    pub current: usize,
}

impl ScriptQueue {
    pub fn new(scripts: Vec<String>) -> Self {
        Self { scripts, current: 0 }
    }

    pub fn current_name(&self) -> Option<&str> {
        self.scripts.get(self.current).map(|s| s.as_str())
    }

    pub fn advance(&mut self) -> bool {
        self.current += 1;
        self.current < self.scripts.len()
    }
}

pub fn on_provisioning(
    trigger: On<Insert, Provisioning>,
    mut commands: Commands,
    configs: Query<&super::prepare::VmConfig>,
    agents: Query<&super::agent::AgentHandle>,
    scripts: Query<&ScriptQueue>,
) {
    let entity = trigger.event_target();
    let Ok(config) = configs.get(entity) else {
        return;
    };
    let Ok(agent) = agents.get(entity) else {
        commands.entity(entity).insert(Done::Success);
        return;
    };
    let Ok(script_queue) = scripts.get(entity) else {
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

    commands.entity(entity).spawn_task(move |cmd: Tq| async move {
        let entity = cmd.entity();

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
        match crate::vm::services::run_provision(&agent_client, provision_scripts, &logs_dir).await
        {
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
