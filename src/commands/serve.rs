use ecsdk_core::{MessageQueue, WakeSignal};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::lifecycle::{LifecyclePlugin, RumMessage, SpawnVmData};
use crate::phase::{FlowIntent, VmPhase};
use crate::replicon::RumServerPlugin;
use crate::vm_state::VmState;

/// Run daemon mode: ECS app with lifecycle + replicon server.
pub async fn run_daemon(sys_config: &SystemConfig) -> Result<(), RumError> {
    let id = &sys_config.id;
    let name_opt = sys_config.name.as_deref();

    // Write PID file
    let pid_file = crate::paths::pid_path(id, name_opt);
    if let Some(parent) = pid_file.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&pid_file, std::process::id().to_string()).map_err(|e| RumError::Io {
        context: format!("writing PID file {}", pid_file.display()),
        source: e,
    })?;

    let socket_path = crate::paths::socket_path(id, name_opt);

    // Detect current VM state
    let initial_state = {
        virt::error::clear_error_callback();
        match virt::connect::Connect::open(Some(sys_config.libvirt_uri())) {
            Ok(mut conn) => {
                let state = crate::vm_state::detect_state(sys_config, &conn);
                conn.close().ok();
                state
            }
            Err(_) => VmState::Virgin,
        }
    };

    // Build script names and select intent
    let (system_scripts, boot_scripts) = build_script_names(sys_config);
    let (intent, initial_phase, scripts, total_steps) =
        select_intent(&initial_state, system_scripts, boot_scripts)?;

    // Set up ECS app with lifecycle + server (no render)
    let (mut app, rx) = ecsdk_app::setup::<RumMessage>();

    // Set up tracing: ecsdk layer (routes tracing events into ECS) + file layer
    let wake = app.world().resource::<WakeSignal>().clone();
    let (tracing_layer, tracing_receiver) = ecsdk_tracing::setup(wake);

    let logs_dir = crate::paths::logs_dir(id, name_opt);
    std::fs::create_dir_all(&logs_dir).ok();
    let (file_writer, file_handle) = crate::logging::DeferredFileWriter::new();
    file_handle.set_file(&logs_dir.join("rum.log")).ok();
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_writer)
        .with_filter(tracing_subscriber::EnvFilter::new("rum=debug"));

    tracing_subscriber::registry()
        .with(
            tracing_layer.with_filter(
                tracing_subscriber::filter::Targets::new()
                    .with_target("rum", tracing::Level::DEBUG),
            ),
        )
        .with(file_layer)
        .init();

    app.add_plugins(ecsdk_tracing::TracingPlugin::new(tracing_receiver));
    app.add_plugins(LifecyclePlugin);
    app.add_plugins(RumServerPlugin {
        socket_path: socket_path.clone(),
    });
    app.insert_resource(crate::replicon::DaemonConfig(sys_config.clone()));
    let service_handles = crate::daemon::start_services(sys_config).await?;

    // Send the SpawnVm message to kick off the state machine
    let state_queue = app.world().resource::<MessageQueue<RumMessage>>().clone();
    state_queue.send(RumMessage::SpawnVm(Box::new(SpawnVmData {
        sys_config: sys_config.clone(),
        intent,
        initial_phase,
        scripts,
        total_steps,
    })));

    // Handle Ctrl+C / SIGTERM: send shutdown request
    let shutdown_queue = state_queue.clone();
    tokio::spawn(async move {
        let mut first = true;
        loop {
            tokio::signal::ctrl_c().await.ok();
            if first {
                shutdown_queue.send(RumMessage::RequestShutdown);
                first = false;
            } else {
                shutdown_queue.send(RumMessage::RequestForceStop);
            }
        }
    });

    // Run the ECS select loop until AppExit is set
    ecsdk_app::run_async(&mut app, rx).await;

    // Write provisioned marker if we completed a first boot successfully
    if matches!(intent, FlowIntent::FirstBoot | FlowIntent::Reboot) {
        let marker = crate::paths::provisioned_marker(id, name_opt);
        if !marker.exists() {
            let _ = tokio::fs::write(&marker, b"").await;
        }
    }

    // Cleanup
    crate::daemon::abort_handles(&service_handles);
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&pid_file);

    Ok(())
}

/// Build provision script names from config (matching names used by flows).
fn build_script_names(sys_config: &SystemConfig) -> (Vec<String>, Vec<String>) {
    let config = &sys_config.config;
    let drives = sys_config.resolve_drives().unwrap_or_default();
    let resolved_fs = sys_config.resolve_fs(&drives).unwrap_or_default();

    let mut system_scripts = Vec::new();
    if !resolved_fs.is_empty() {
        system_scripts.push("rum-drives".to_string());
    }
    if config.provision.system.is_some() {
        system_scripts.push("rum-system".to_string());
    }

    let mut boot_scripts = Vec::new();
    if config.provision.boot.is_some() {
        boot_scripts.push("rum-boot".to_string());
    }

    (system_scripts, boot_scripts)
}

/// Select the ECS flow intent and initial phase from detected VM state.
fn select_intent(
    state: &VmState,
    system_scripts: Vec<String>,
    boot_scripts: Vec<String>,
) -> Result<(FlowIntent, VmPhase, Vec<String>, usize), RumError> {
    match state {
        VmState::Virgin | VmState::ImageCached | VmState::Prepared | VmState::PartialBoot => {
            let mut scripts = system_scripts;
            scripts.extend(boot_scripts);
            let total = match state {
                VmState::Virgin | VmState::ImageCached => 4 + scripts.len(),
                VmState::Prepared | VmState::PartialBoot => 2 + scripts.len(),
                _ => 1,
            };
            let initial_phase = VmPhase::from_vm_state(*state, FlowIntent::FirstBoot);
            Ok((FlowIntent::FirstBoot, initial_phase, scripts, total))
        }
        VmState::Provisioned => {
            let total = 2 + boot_scripts.len();
            Ok((FlowIntent::Reboot, VmPhase::Booting, boot_scripts, total))
        }
        VmState::Running => Ok((FlowIntent::Reattach, VmPhase::StartingServices, vec![], 1)),
        VmState::RunningStale => Err(RumError::RequiresRestart {
            name: "VM".into(),
        }),
    }
}
