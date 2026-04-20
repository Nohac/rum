use std::collections::HashMap;

use bevy::ecs::prelude::*;
use orchestrator::{
    EntityError, InstanceLabel, InstancePhase, ProvisionLogEntry, ProvisionLogView, RecoveredState,
};

#[derive(Default)]
pub(super) struct PlainRenderState {
    last_phase: HashMap<Entity, InstancePhase>,
    last_log_count: HashMap<Entity, usize>,
    last_recovered: HashMap<Entity, machine::instance::InstanceState>,
    printed_failure: HashMap<Entity, String>,
}

#[allow(clippy::type_complexity)]
pub(super) fn render_plain(
    query: Query<
        (
            Entity,
            Option<&InstanceLabel>,
            Option<&RecoveredState>,
            Option<&ProvisionLogView>,
            &InstancePhase,
            Option<&EntityError>,
        ),
        Without<ecsdk::network::InitialConnection>,
    >,
    log_entries: Query<&ProvisionLogEntry>,
    mut state: Local<PlainRenderState>,
) {
    let mut entities: Vec<_> = query.iter().collect();
    entities.sort_by(|a, b| {
        let label_a = a.1.map(|label| label.0.as_str()).unwrap_or("instance");
        let label_b = b.1.map(|label| label.0.as_str()).unwrap_or("instance");
        label_a.cmp(label_b).then_with(|| a.0.index().cmp(&b.0.index()))
    });

    for (entity, label, recovered, log_view, phase, error) in entities {
        let label = label.map(|label| label.0.as_str()).unwrap_or("instance");

        if let Some(recovered) = recovered {
            let recovered_state = **recovered;
            if state.last_recovered.get(&entity) != Some(&recovered_state) {
                println!("{label}: recovered state = {recovered_state}");
                state.last_recovered.insert(entity, recovered_state);
            }
        }

        let phase = *phase;
        if state.last_phase.get(&entity) != Some(&phase) {
            println!("{label}: {}", phase.label());
            state.last_phase.insert(entity, phase);
        }

        if phase == InstancePhase::Failed
            && let Some(error) = error
            && state.printed_failure.get(&entity) != Some(&error.0)
        {
            eprintln!("{label}: {}", error.0);
            state.printed_failure.insert(entity, error.0.clone());
        }

        if let Some(log_view) = log_view {
            let seen = state.last_log_count.get(&entity).copied().unwrap_or_default();
            for entry_entity in log_view.iter().skip(seen) {
                if let Ok(entry) = log_entries.get(entry_entity) {
                    println!("  {} | {}", entry.label, entry.message);
                }
            }
            state.last_log_count.insert(entity, log_view.iter().len());
        }
    }
}
