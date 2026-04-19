use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::Context;
use clap::{Parser, Subcommand};
use cli::render::{RenderMode, RumRenderPlugin};
use machine::config::{SystemConfig, load_config};
use machine::driver::{Driver, LibvirtDriver};
use machine::instance::Instance;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

const INTERNAL_DAEMON_CONFIG: &str = "RUM_INTERNAL_DAEMON_CONFIG";

#[derive(Parser)]
#[command(name = "rum")]
#[command(about = "Bootstraps rum orchestration flows")]
struct Cli {
    /// Path to the rum config file.
    #[arg(short, long, default_value = "rum.toml")]
    config: PathBuf,

    /// Output mode for the attached client.
    #[arg(long, value_enum, default_value_t = RenderMode::Plain)]
    output: RenderMode,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(flatten)]
    Direct(DirectCmd),
    #[command(flatten)]
    Starts(StartsDaemonCmd),
    #[command(flatten)]
    Requires(RequiresDaemonCmd),
    #[command(flatten)]
    Maybe(MaybeDaemonCmd),
}

#[derive(Subcommand)]
enum StartsDaemonCmd {
    /// Start or attach to the current machine.
    Up,
}

#[derive(Subcommand)]
enum DirectCmd {
    /// Show provisioning logs from the local instance work directory.
    Log {
        /// Show only the newest failed provisioning log.
        #[arg(long)]
        failed: bool,

        /// List available provisioning logs newest first.
        #[arg(long)]
        list: bool,
    },
}

#[derive(Subcommand)]
enum RequiresDaemonCmd {
    /// Ask the daemon to shut down the current machine.
    Down,
    /// Copy files to or from the managed guest.
    Cp {
        /// Source path. Prefix the guest path with `:`.
        src: String,
        /// Destination path. Prefix the guest path with `:`.
        dst: String,
    },
    /// Query the daemon for the current machine status.
    Status {
        /// Keep the status client attached and render live updates.
        #[arg(long)]
        watch: bool,

        /// Stay attached until the instance reaches running or a terminal state.
        #[arg(long)]
        wait_ready: bool,
    },
}

#[derive(Subcommand)]
enum MaybeDaemonCmd {
    /// Destroy the managed machine and purge its persisted state.
    Destroy,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    if let Some(config) = std::env::var_os(INTERNAL_DAEMON_CONFIG) {
        return run_daemon(&PathBuf::from_str(
            &config
                .into_string()
                .expect("failed to convert config path to string"),
        )?)
        .await;
    }

    let cli = Cli::parse();

    let system = load_config(&cli.config).context("failed to load machine config")?;

    if let Command::Direct(cmd) = &cli.command {
        return match cmd {
            DirectCmd::Log { failed, list } => {
                let selection = match (*failed, *list) {
                    (true, true) => anyhow::bail!("--failed and --list are mutually exclusive"),
                    (true, false) => cli::log::LogSelection::LatestFailed,
                    (false, true) => cli::log::LogSelection::List,
                    (false, false) => cli::log::LogSelection::Latest,
                };
                cli::log::run(&system, selection)
            }
        };
    }

    let socket_path = cli::ipc::socket_path(&system);
    let restart_requested = Arc::new(AtomicBool::new(false));
    let iso = cli::app::create_isomorphic_app(socket_path, restart_requested.clone());

    let mut app = iso.build_client();
    let config_path = cli.config.canonicalize()?;

    match cli.command {
        Command::Direct(_) => unreachable!("direct commands return before daemon setup"),
        Command::Starts(cmd) => match cmd {
            StartsDaemonCmd::Up => {
                app.add_plugins(RumRenderPlugin::new(cli.output));
                run_up(&config_path, &system, app)
                    .await
                    .context("failed to run up command")?;
            }
        },
        Command::Requires(cmd) => {
            ensure_connected(&cli.config, &system).await?;

            match cmd {
                RequiresDaemonCmd::Down => {
                    run_down(app).await?;
                }
                RequiresDaemonCmd::Cp { src, dst } => {
                    run_cp(app, &src, &dst).await?;
                }
                RequiresDaemonCmd::Status { watch, wait_ready } => {
                    let render_enabled = watch || wait_ready;
                    if render_enabled {
                        app.add_plugins(RumRenderPlugin::new(cli.output));
                    }
                    run_status(app, watch, wait_ready).await?;
                }
            }
        }
        Command::Maybe(cmd) => match cmd {
            MaybeDaemonCmd::Destroy => {
                let app = cli::app::build_client_app(app, cli.output, true);
                run_destroy(system.clone(), app).await?;
            }
        },
    };

    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false),
        )
        .try_init();
}

async fn run_up(
    config_path: &Path,
    system: &SystemConfig,
    app: ecsdk::app::AsyncApp<orchestrator::OrchestratorMessage>,
) -> anyhow::Result<()> {
    let socket_path = cli::ipc::socket_path(system);
    ensure_daemon(config_path, &socket_path)
        .await
        .context("Failed to ensure daemon")?;

    let app = cli::client::build_up_client(app);
    app.run().await;
    Ok(())
}

async fn run_daemon(config_path: &Path) -> anyhow::Result<()> {
    let spec = cli::server::load_server_spec(config_path).await?;
    let socket_path = spec.socket_path.clone();
    let control_socket_path = cli::ipc::control_socket_path(&spec.system);
    tokio::spawn(async move {
        if let Err(error) = cli::control::run_control_server(control_socket_path, socket_path).await
        {
            tracing::error!(error = %error, "control sidechannel failed");
        }
    });

    let socket_path = spec.socket_path.clone();
    let iso =
        cli::app::create_isomorphic_app(spec.socket_path.clone(), Arc::new(AtomicBool::new(false)));
    let app = cli::server::build_up_server(iso, spec);
    app.run().await;
    let _ = std::fs::remove_file(socket_path);
    Ok(())
}

async fn run_down(
    app: ecsdk::app::AsyncApp<orchestrator::OrchestratorMessage>,
) -> anyhow::Result<()> {
    let app = cli::down::build_down_client(app);
    app.run().await;
    Ok(())
}

async fn run_status(
    app: ecsdk::app::AsyncApp<orchestrator::OrchestratorMessage>,
    watch: bool,
    wait_ready: bool,
) -> anyhow::Result<()> {
    let mode = match (watch, wait_ready) {
        (true, true) => anyhow::bail!("--watch and --wait-ready are mutually exclusive"),
        (true, false) => cli::status::StatusMode::Watch,
        (false, true) => cli::status::StatusMode::WaitReady,
        (false, false) => cli::status::StatusMode::Snapshot,
    };

    let app = cli::status::build_status_client(app, mode);
    app.run().await;
    Ok(())
}

async fn run_cp(
    app: ecsdk::app::AsyncApp<orchestrator::OrchestratorMessage>,
    src: &str,
    dst: &str,
) -> anyhow::Result<()> {
    let request = cli::cp::prepare_request(src, dst)?;
    let app = cli::cp::build_cp_client(app, request);
    app.run().await;
    Ok(())
}

async fn run_destroy(
    system: SystemConfig,
    app: ecsdk::app::AsyncApp<orchestrator::OrchestratorMessage>,
) -> anyhow::Result<()> {
    let socket_path = cli::ipc::socket_path(&system);

    if cli::ipc::connect(&socket_path).await.is_err() {
        let instance = Instance::<LibvirtDriver>::new(system.clone());
        instance.driver().destroy().await?;
        println!("destroyed local rum state");
        return Ok(());
    }

    let app = cli::destroy::build_destroy_client(app);
    app.run().await;
    Ok(())
}

async fn ensure_daemon(config_path: &Path, socket_path: &Path) -> anyhow::Result<()> {
    if cli::ipc::connect(socket_path).await.is_ok() {
        return Ok(());
    }
    spawn_daemon(config_path)?;

    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if cli::ipc::connect(socket_path).await.is_ok() {
            return Ok(());
        }
    }

    anyhow::bail!(format!(
        "timed out waiting for rum daemon at {}",
        socket_path.display()
    ))
}

async fn ensure_connected(config: &Path, system: &SystemConfig) -> anyhow::Result<()> {
    let socket_path = cli::ipc::socket_path(system);
    return match cli::ipc::connect(&socket_path).await {
        Ok(_) => Ok(()),
        Err(_) => maybe_restart_daemon(config, system).await,
    };
}

fn spawn_daemon(config_path: &Path) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let config_dir = config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .context("what is hanging")?;

    let config_name = config_path
        .file_name()
        .context(format!("invalid config path: {}", &config_path.display()))?;
    std::process::Command::new(&exe)
        .current_dir(config_dir)
        .env(INTERNAL_DAEMON_CONFIG, config_name)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context(format!("Failed to run command: {}", exe.display()))?;
    Ok(())
}

async fn maybe_restart_daemon(config_path: &Path, system: &SystemConfig) -> anyhow::Result<()> {
    let control_socket_path = cli::ipc::control_socket_path(system);
    let main_socket_path = cli::ipc::socket_path(system);
    let pid = cli::control::shutdown_daemon(&control_socket_path)
        .await
        .context("Failed to shut down daemon")?;

    wait_for_pid_exit(pid).await?;
    spawn_daemon(config_path)?;

    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if cli::ipc::connect(&main_socket_path).await.is_ok() {
            break;
        }
    }

    Ok(())
}

async fn wait_for_pid_exit(pid: u32) -> anyhow::Result<()> {
    let proc_path = PathBuf::from(format!("/proc/{pid}"));
    for _ in 0..50 {
        if !proc_path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    anyhow::bail!("timed out waiting for daemon process {pid} to exit")
}
