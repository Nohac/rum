use roam_stream::{HandshakeConfig, accept};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::paths;

use super::client::connect;
use super::rpc::RumDaemonDispatcher;
use super::service::{DaemonAction, DaemonImpl};

// ── Daemon serve loop ───────────────────────────────────────────────

pub async fn run_serve(sys_config: &SystemConfig) -> Result<(), RumError> {
    let id = &sys_config.id;
    let name_opt = sys_config.name.as_deref();
    let vm_name = sys_config.display_name();
    let uri = sys_config.libvirt_uri().to_string();
    let vm_name_owned = vm_name.to_string();

    // Start background services (log subscription, port forwards)
    let service_handles = start_services(sys_config).await?;

    // Write PID file
    let pid_file = paths::pid_path(id, name_opt);
    if let Some(parent) = pid_file.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&pid_file, std::process::id().to_string()).map_err(|e| RumError::Io {
        context: format!("writing PID file {}", pid_file.display()),
        source: e,
    })?;

    // Bind Unix socket
    let sock_path = paths::socket_path(id, name_opt);
    if sock_path.exists() {
        let _ = std::fs::remove_file(&sock_path);
    }
    let listener =
        tokio::net::UnixListener::bind(&sock_path).map_err(|e| RumError::Io {
            context: format!("binding Unix socket {}", sock_path.display()),
            source: e,
        })?;

    let (action_tx, mut action_rx) = mpsc::channel::<DaemonAction>(4);
    let handler = DaemonImpl::new(sys_config, action_tx);

    tracing::info!(vm_name, sock = %sock_path.display(), "daemon listening");

    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| RumError::Io {
                context: "registering SIGTERM handler".into(),
                source: e,
            })?;

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let dispatcher = RumDaemonDispatcher::new(handler.clone());
                        tokio::spawn(async move {
                            match accept(stream, HandshakeConfig::default(), dispatcher).await {
                                Ok((_handle, _incoming, driver)) => {
                                    let _ = driver.run().await;
                                }
                                Err(e) => tracing::error!("daemon handshake failed: {e}"),
                            }
                        });
                    }
                    Err(e) => tracing::error!("daemon accept error: {e}"),
                }
            }
            _ = action_rx.recv() => {
                tracing::info!("daemon received shutdown action");
                break;
            }
            _ = poll_domain_state(&uri, &vm_name_owned) => {
                tracing::info!("VM stopped externally, daemon exiting");
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!("daemon received SIGTERM");
                break;
            }
        }
    }

    // Cleanup
    abort_handles(&service_handles);
    let _ = std::fs::remove_file(&sock_path);
    let _ = std::fs::remove_file(&pid_file);

    tracing::info!("daemon exited");
    Ok(())
}

// ── Background service management ───────────────────────────────────

pub(crate) struct ServiceHandles {
    pub log_handle: Option<JoinHandle<()>>,
    pub forward_handles: Vec<JoinHandle<()>>,
}

fn abort_handles(handles: &ServiceHandles) {
    if let Some(ref h) = handles.log_handle {
        h.abort();
    }
    for h in &handles.forward_handles {
        h.abort();
    }
}

async fn start_services(sys_config: &SystemConfig) -> Result<ServiceHandles, RumError> {
    let config = &sys_config.config;

    // Connect to agent via vsock
    let vsock_cid = crate::backend::libvirt::get_vsock_cid(sys_config).ok();

    let agent_client = if let Some(cid) = vsock_cid {
        crate::agent::wait_for_agent(cid).await.ok()
    } else {
        None
    };

    // Log subscription
    let log_handle = agent_client
        .as_ref()
        .map(crate::agent::start_log_subscription);

    // Port forwards
    let forward_handles = if let Some(cid) = vsock_cid
        && !config.ports.is_empty()
    {
        crate::agent::start_port_forwards(cid, &config.ports).await?
    } else {
        Vec::new()
    };

    Ok(ServiceHandles {
        log_handle,
        forward_handles,
    })
}

// ── Daemon spawning ─────────────────────────────────────────────────

pub fn spawn_background(sys_config: &SystemConfig) -> Result<(), RumError> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let exe = std::env::current_exe().map_err(|e| RumError::Io {
        context: "getting current executable path".into(),
        source: e,
    })?;

    let config_path = &sys_config.config_path;

    // Ensure work dir exists for PID + socket files
    let work = paths::work_dir(&sys_config.id, sys_config.name.as_deref());
    std::fs::create_dir_all(&work).map_err(|e| RumError::Io {
        context: format!("creating work directory {}", work.display()),
        source: e,
    })?;

    Command::new(exe)
        .args(["--config", &config_path.to_string_lossy(), "serve"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| RumError::Io {
            context: "spawning daemon process".into(),
            source: e,
        })?;

    Ok(())
}

pub async fn wait_for_daemon_ready(sys_config: &SystemConfig) -> Result<(), RumError> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);

    loop {
        if let Ok(client) = connect(sys_config)
            && client.ping().await.is_ok()
        {
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(RumError::Daemon {
                message: "daemon did not become ready within 10s".into(),
            });
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

async fn poll_domain_state(uri: &str, vm_name: &str) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let still_running = (|| {
            virt_error::clear_error_callback();
            let mut conn = Connect::open(Some(uri)).ok()?;
            let dom = Domain::lookup_by_name(&conn, vm_name).ok()?;
            let active = dom.is_active().unwrap_or(false);
            conn.close().ok();
            Some(active)
        })();
        if still_running != Some(true) {
            return;
        }
    }
}
