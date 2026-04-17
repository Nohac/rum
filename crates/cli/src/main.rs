use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use cli::render::RenderMode;
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
    /// Internal daemon entrypoint.
    Daemon,

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
enum RequiresDaemonCmd {
    /// Ask the daemon to shut down the current machine.
    Down,
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let mut cli = Cli::parse();

    if matches!(cli.command, Command::Daemon)
        && let Some(config) = std::env::var_os(INTERNAL_DAEMON_CONFIG)
    {
        cli.config = PathBuf::from(config);
    }

    if matches!(cli.command, Command::Daemon) {
        return run_daemon(&cli.config).await;
    }

    let system = load_config(&cli.config)?;
    let socket_path = cli::ipc::socket_path(&system);
    let restart_requested = Arc::new(AtomicBool::new(false));
    let iso = cli::app::create_isomorphic_app(socket_path, restart_requested.clone());

    let restart_args = match cli.command {
        Command::Daemon => unreachable!("daemon command returns before client setup"),
        Command::Starts(cmd) => match cmd {
            StartsDaemonCmd::Up => {
                let app = cli::app::build_client_app(iso, cli.output, true);
                run_up(&cli.config, &system, app).await?;
                up_args(&cli.config, cli.output)
            }
        },
        Command::Requires(cmd) => {
            ensure_connected(&system).await?;

            match cmd {
                RequiresDaemonCmd::Down => {
                    let app = cli::app::build_client_app(iso, cli.output, true);
                    run_down(app).await?;
                    down_args(&cli.config, cli.output)
                }
                RequiresDaemonCmd::Status { watch, wait_ready } => {
                    let render_enabled = watch || wait_ready;
                    let app = cli::app::build_client_app(iso, cli.output, render_enabled);
                    run_status(app, watch, wait_ready).await?;
                    status_args(&cli.config, cli.output, watch, wait_ready)
                }
            }
        }
        Command::Maybe(cmd) => match cmd {
            MaybeDaemonCmd::Destroy => {
                let app = cli::app::build_client_app(iso, cli.output, true);
                run_destroy(
                    &cli.config,
                    system.clone(),
                    app,
                    restart_requested.clone(),
                    cli.output,
                )
                .await?;
                destroy_args(&cli.config, cli.output)
            }
        },
    };

    maybe_restart_daemon(&cli.config, &system, restart_requested, restart_args).await?;

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
) -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = cli::ipc::socket_path(system);
    ensure_daemon(config_path, &socket_path).await?;

    let app = cli::client::build_up_client(app);
    app.run().await;
    Ok(())
}

async fn run_daemon(config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
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
) -> Result<(), Box<dyn std::error::Error>> {
    let app = cli::down::build_down_client(app);
    app.run().await;
    Ok(())
}

async fn run_status(
    app: ecsdk::app::AsyncApp<orchestrator::OrchestratorMessage>,
    watch: bool,
    wait_ready: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mode = match (watch, wait_ready) {
        (true, true) => return Err("--watch and --wait-ready are mutually exclusive".into()),
        (true, false) => cli::status::StatusMode::Watch,
        (false, true) => cli::status::StatusMode::WaitReady,
        (false, false) => cli::status::StatusMode::Snapshot,
    };

    let app = cli::status::build_status_client(app, mode);
    app.run().await;
    Ok(())
}

async fn run_destroy(
    config_path: &Path,
    system: SystemConfig,
    app: ecsdk::app::AsyncApp<orchestrator::OrchestratorMessage>,
    restart_requested: Arc<AtomicBool>,
    render_mode: RenderMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = cli::ipc::socket_path(&system);

    if cli::ipc::connect(&socket_path).await.is_err() {
        let instance = Instance::<LibvirtDriver>::new(system.clone());
        instance.driver().destroy().await?;
        println!("destroyed local rum state");
        return Ok(());
    }

    let app = cli::destroy::build_destroy_client(app);
    app.run().await;
    maybe_restart_daemon(
        config_path,
        &system,
        restart_requested,
        destroy_args(config_path, render_mode),
    )
    .await?;
    Ok(())
}

async fn ensure_daemon(
    config_path: &Path,
    socket_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
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

    Err(format!(
        "timed out waiting for rum daemon at {}",
        socket_path.display()
    )
    .into())
}

async fn ensure_connected(system: &SystemConfig) -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = cli::ipc::socket_path(system);
    if cli::ipc::connect(&socket_path).await.is_ok() {
        return Ok(());
    }
    Err(format!("no rum daemon is running at {}", socket_path.display()).into())
}

fn spawn_daemon(config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let config_name = config_path
        .file_name()
        .ok_or_else(|| format!("invalid config path: {}", config_path.display()))?;

    std::process::Command::new(exe)
        .current_dir(config_dir)
        .env(INTERNAL_DAEMON_CONFIG, config_name)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;
    Ok(())
}

async fn maybe_restart_daemon(
    config_path: &Path,
    system: &SystemConfig,
    restart_requested: Arc<AtomicBool>,
    args: Vec<OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !restart_requested.swap(false, Ordering::SeqCst) {
        return Ok(());
    }

    let control_socket_path = cli::ipc::control_socket_path(system);
    let main_socket_path = cli::ipc::socket_path(system);
    let pid = cli::control::shutdown_daemon(&control_socket_path)
        .await
        .map_err(|error| format!("failed to shut down stale daemon: {error}"))?;

    wait_for_pid_exit(pid).await?;
    spawn_daemon(config_path)?;

    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if cli::ipc::connect(&main_socket_path).await.is_ok() {
            break;
        }
    }

    let exe = std::env::current_exe()?;
    let status = std::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    if !status.success() {
        return Err(format!("replacement client exited with {status}").into());
    }

    Ok(())
}

async fn wait_for_pid_exit(pid: u32) -> Result<(), Box<dyn std::error::Error>> {
    let proc_path = PathBuf::from(format!("/proc/{pid}"));
    for _ in 0..50 {
        if !proc_path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!("timed out waiting for daemon process {pid} to exit").into())
}

fn render_mode_arg(render_mode: RenderMode) -> OsString {
    let name = render_mode
        .to_possible_value()
        .map(|value| value.get_name().to_string())
        .unwrap_or_else(|| "plain".into());
    OsString::from(name)
}

fn up_args(config_path: &Path, render_mode: RenderMode) -> Vec<OsString> {
    vec![
        OsString::from("--config"),
        config_path.as_os_str().to_os_string(),
        OsString::from("--output"),
        render_mode_arg(render_mode),
        OsString::from("up"),
    ]
}

fn down_args(config_path: &Path, render_mode: RenderMode) -> Vec<OsString> {
    vec![
        OsString::from("--config"),
        config_path.as_os_str().to_os_string(),
        OsString::from("--output"),
        render_mode_arg(render_mode),
        OsString::from("down"),
    ]
}

fn destroy_args(config_path: &Path, render_mode: RenderMode) -> Vec<OsString> {
    vec![
        OsString::from("--config"),
        config_path.as_os_str().to_os_string(),
        OsString::from("--output"),
        render_mode_arg(render_mode),
        OsString::from("destroy"),
    ]
}

fn status_args(
    config_path: &Path,
    render_mode: RenderMode,
    watch: bool,
    wait_ready: bool,
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("--config"),
        config_path.as_os_str().to_os_string(),
        OsString::from("--output"),
        render_mode_arg(render_mode),
        OsString::from("status"),
    ];
    if watch {
        args.push(OsString::from("--watch"));
    }
    if wait_ready {
        args.push(OsString::from("--wait-ready"));
    }
    args
}
