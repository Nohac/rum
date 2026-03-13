use tokio::task::JoinHandle;

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::paths;

// ── Background service management ───────────────────────────────────

pub(crate) struct ServiceHandles {
    pub log_handle: Option<JoinHandle<()>>,
    pub forward_handles: Vec<JoinHandle<()>>,
}

pub(crate) fn abort_handles(handles: &ServiceHandles) {
    if let Some(ref h) = handles.log_handle {
        h.abort();
    }
    for h in &handles.forward_handles {
        h.abort();
    }
}

pub(crate) async fn start_services(sys_config: &SystemConfig) -> Result<ServiceHandles, RumError> {
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

    let workspace_dir = std::env::current_dir().map_err(|e| RumError::Io {
        context: "getting current working directory".into(),
        source: e,
    })?;
    let daemon_log = workspace_dir.join("rum-daemon.log");
    let stdout_log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&daemon_log)
        .map_err(|e| RumError::Io {
            context: format!("opening daemon stdio log {}", daemon_log.display()),
            source: e,
        })?;
    let stderr_log = stdout_log.try_clone().map_err(|e| RumError::Io {
        context: format!("cloning daemon stdio log handle {}", daemon_log.display()),
        source: e,
    })?;

    let child = Command::new(exe)
        .args(["--config", &config_path.to_string_lossy(), "serve"])
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(stderr_log))
        .process_group(0)
        .spawn()
        .map_err(|e| RumError::Io {
            context: "spawning daemon process".into(),
            source: e,
        })
        .inspect_err(|e| tracing::debug!(?e))?;

    log_daemon_child(child, workspace_dir);

    Ok(())
}

fn log_daemon_child(mut child: std::process::Child, workspace_dir: std::path::PathBuf) {
    std::thread::spawn(move || {
        let status = child.wait();
        let message = match status {
            Ok(status) => format!(
                "daemon process exited: pid={} status={status}\n",
                child.id()
            ),
            Err(error) => format!(
                "failed waiting for daemon process pid={}: {error}\n",
                child.id()
            ),
        };

        for file_name in ["rum-client.log", "rum-daemon.log"] {
            let path = workspace_dir.join(file_name);
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = std::io::Write::write_all(&mut file, message.as_bytes());
            }
        }
    });
}

pub fn is_daemon_running(sys_config: &SystemConfig) -> bool {
    let pid_file = paths::pid_path(&sys_config.id, sys_config.name.as_deref());
    let Ok(contents) = std::fs::read_to_string(&pid_file) else {
        return false;
    };
    let Ok(pid) = contents.trim().parse::<i32>() else {
        return false;
    };
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}
