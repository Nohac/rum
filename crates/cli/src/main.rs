use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use machine::config::load_config;
use cli::render::RenderMode;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

#[derive(Parser)]
#[command(name = "rum")]
#[command(about = "Bootstraps rum orchestration flows")]
struct Cli {
    /// Run in daemon mode instead of attaching as a client.
    #[arg(short, long)]
    daemon: bool,

    /// Output mode for the attached client.
    #[arg(long, value_enum, default_value_t = RenderMode::Plain)]
    output: RenderMode,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start or attach to the current machine.
    Up {
        /// Path to the rum config file.
        #[arg(default_value = "rum.toml")]
        config: PathBuf,
    },
    /// Ask the daemon to shut down the current machine.
    Down {
        /// Path to the rum config file.
        #[arg(default_value = "rum.toml")]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Command::Up { config } if cli.daemon => run_daemon(&config).await?,
        Command::Up { config } => run_up(&config, cli.output).await?,
        Command::Down { .. } if cli.daemon => {
            return Err("--daemon only supports `rum up`".into());
        }
        Command::Down { config } => run_down(&config, cli.output).await?,
    }

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
    render_mode: RenderMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let system = load_config(config_path)?;
    let socket_path = cli::ipc::socket_path(&system);

    ensure_daemon(config_path, &socket_path).await?;

    let app = cli::client::build_up_client(socket_path, render_mode);
    app.run().await;
    Ok(())
}

async fn run_daemon(config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let spec = cli::server::load_server_spec(config_path).await?;
    let socket_path = spec.socket_path.clone();
    let app = cli::server::build_up_server(spec);
    app.run().await;
    let _ = std::fs::remove_file(socket_path);
    Ok(())
}

async fn run_down(
    config_path: &Path,
    render_mode: RenderMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let system = load_config(config_path)?;
    let socket_path = cli::ipc::socket_path(&system);

    if cli::ipc::connect(&socket_path).await.is_err() {
        tracing::info!(socket = %socket_path.display(), "no rum daemon is running");
        return Ok(());
    }

    let app = cli::down::build_down_client(socket_path, render_mode);
    app.run().await;
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

fn spawn_daemon(config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .arg("--daemon")
        .arg("up")
        .arg(config_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;
    Ok(())
}
