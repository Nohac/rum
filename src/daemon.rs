use std::io;
use std::path::PathBuf;

use facet::Facet;
use roam_stream::{Client, Connector, HandshakeConfig, NoDispatcher, accept};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use virt::connect::Connect;
use virt::domain::Domain;
use virt::error as virt_error;

use crate::config::SystemConfig;
use crate::error::RumError;
use crate::paths;

// ── Roam service definition ─────────────────────────────────────────

#[derive(Debug, Clone, Facet)]
pub struct StatusInfo {
    pub state: String,
    pub ips: Vec<String>,
    pub daemon_running: bool,
}

#[roam::service]
pub trait RumDaemon {
    async fn ping(&self) -> Result<String, String>;
    async fn shutdown(&self) -> Result<String, String>;
    async fn force_stop(&self) -> Result<String, String>;
    async fn status(&self) -> Result<StatusInfo, String>;
    async fn ssh_config(&self) -> Result<String, String>;
}

// ── Connector: always Unix socket ───────────────────────────────────

pub struct DaemonConnector {
    path: PathBuf,
}

impl Connector for DaemonConnector {
    type Transport = UnixStream;

    async fn connect(&self) -> io::Result<UnixStream> {
        UnixStream::connect(&self.path).await
    }
}

// ── Client type alias ───────────────────────────────────────────────

pub type DaemonClient = RumDaemonClient<Client<DaemonConnector, NoDispatcher>>;

// ── DaemonImpl: daemon-mode implementation ──────────────────────────

enum DaemonAction {
    Shutdown,
    ForceStop,
}

#[derive(Clone)]
pub struct DaemonImpl {
    uri: String,
    vm_name: String,
    ssh_user: String,
    ssh_key_path: PathBuf,
    action_tx: mpsc::Sender<DaemonAction>,
}

impl DaemonImpl {
    fn new(sys_config: &SystemConfig, action_tx: mpsc::Sender<DaemonAction>) -> Self {
        Self {
            uri: sys_config.libvirt_uri().to_string(),
            vm_name: sys_config.display_name().to_string(),
            ssh_user: sys_config.config.ssh.user.clone(),
            ssh_key_path: paths::ssh_key_path(&sys_config.id, sys_config.name.as_deref()),
            action_tx,
        }
    }

    /// Signal the daemon loop to exit after a short delay, giving the
    /// RPC response time to flush back to the client before teardown.
    fn signal_exit_deferred(&self, action: DaemonAction) {
        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let _ = tx.send(action).await;
        });
    }

    fn connect_libvirt(&self) -> Result<Connect, String> {
        virt_error::clear_error_callback();
        Connect::open(Some(&self.uri)).map_err(|e| format!("libvirt connect failed: {e}"))
    }

    fn lookup_domain(&self, conn: &Connect) -> Result<Domain, String> {
        Domain::lookup_by_name(conn, &self.vm_name)
            .map_err(|_| format!("VM '{}' is not defined", self.vm_name))
    }
}

impl RumDaemon for DaemonImpl {
    async fn ping(&self, _cx: &roam::Context) -> Result<String, String> {
        Ok("daemon".into())
    }

    async fn shutdown(&self, _cx: &roam::Context) -> Result<String, String> {
        let conn = self.connect_libvirt()?;
        let dom = self.lookup_domain(&conn)?;

        if !dom.is_active().unwrap_or(false) {
            return Ok(format!("VM '{}' is not running.", self.vm_name));
        }

        // ACPI shutdown
        dom.shutdown()
            .map_err(|e| format!("shutdown failed: {e}"))?;

        // Wait up to 30s
        for _ in 0..30 {
            if !dom.is_active().unwrap_or(false) {
                self.signal_exit_deferred(DaemonAction::Shutdown);
                return Ok(format!("VM '{}' stopped.", self.vm_name));
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        // Force stop
        let _ = dom.destroy();
        self.signal_exit_deferred(DaemonAction::Shutdown);
        Ok(format!("VM '{}' force stopped.", self.vm_name))
    }

    async fn force_stop(&self, _cx: &roam::Context) -> Result<String, String> {
        let conn = self.connect_libvirt()?;

        if let Ok(dom) = Domain::lookup_by_name(&conn, &self.vm_name)
            && dom.is_active().unwrap_or(false)
        {
            let _ = dom.destroy();
        }

        self.signal_exit_deferred(DaemonAction::ForceStop);
        Ok(format!("VM '{}' force stopped.", self.vm_name))
    }

    async fn status(&self, _cx: &roam::Context) -> Result<StatusInfo, String> {
        let conn = self.connect_libvirt()?;

        match Domain::lookup_by_name(&conn, &self.vm_name) {
            Ok(dom) => {
                let state = if dom.is_active().unwrap_or(false) {
                    "running".to_string()
                } else {
                    "stopped".to_string()
                };

                let mut ips = Vec::new();
                if dom.is_active().unwrap_or(false)
                    && let Ok(ifaces) = dom.interface_addresses(
                        virt::sys::VIR_DOMAIN_INTERFACE_ADDRESSES_SRC_LEASE,
                        0,
                    )
                {
                    for iface in &ifaces {
                        for addr in &iface.addrs {
                            ips.push(addr.addr.clone());
                        }
                    }
                }

                Ok(StatusInfo {
                    state,
                    ips,
                    daemon_running: true,
                })
            }
            Err(_) => Ok(StatusInfo {
                state: "not defined".to_string(),
                ips: Vec::new(),
                daemon_running: true,
            }),
        }
    }

    async fn ssh_config(&self, _cx: &roam::Context) -> Result<String, String> {
        let conn = self.connect_libvirt()?;
        let dom = self.lookup_domain(&conn)?;

        if !dom.is_active().unwrap_or(false) {
            return Err(format!("VM '{}' is not running", self.vm_name));
        }

        let ip = get_first_ip(&dom)
            .ok_or_else(|| format!("no IP found for VM '{}'", self.vm_name))?;

        Ok(format!(
            "Host {vm}\n  \
             HostName {ip}\n  \
             User {user}\n  \
             IdentityFile {key}\n  \
             StrictHostKeyChecking no\n  \
             UserKnownHostsFile /dev/null\n  \
             LogLevel ERROR",
            vm = self.vm_name,
            user = self.ssh_user,
            key = self.ssh_key_path.display(),
        ))
    }
}

fn get_first_ip(dom: &Domain) -> Option<String> {
    let ifaces = dom
        .interface_addresses(virt::sys::VIR_DOMAIN_INTERFACE_ADDRESSES_SRC_LEASE, 0)
        .ok()?;

    for iface in &ifaces {
        for addr in &iface.addrs {
            if addr.typed == 0 {
                return Some(addr.addr.clone());
            }
        }
    }
    None
}

// ── connect(): create a client connected to the daemon ──────────────

pub fn connect(sys_config: &SystemConfig) -> Result<DaemonClient, RumError> {
    if !is_daemon_running(sys_config) {
        return Err(RumError::Daemon {
            message: format!(
                "no daemon running for '{}'. Run `rum up` first.",
                sys_config.display_name()
            ),
        });
    }
    let sock = paths::socket_path(&sys_config.id, sys_config.name.as_deref());
    let connector = DaemonConnector { path: sock };
    let client = roam_stream::connect(connector, HandshakeConfig::default(), NoDispatcher);
    Ok(RumDaemonClient::new(client))
}

pub fn is_daemon_running(sys_config: &SystemConfig) -> bool {
    let pid_file = paths::pid_path(&sys_config.id, sys_config.name.as_deref());
    let Ok(contents) = std::fs::read_to_string(&pid_file) else {
        return false;
    };
    let Ok(pid) = contents.trim().parse::<i32>() else {
        return false;
    };
    // Check if process is alive via /proc
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

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
