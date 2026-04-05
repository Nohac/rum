use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use machine::config::load_config;

#[derive(Parser)]
#[command(name = "rum")]
#[command(about = "Bootstraps rum orchestration flows")]
struct Cli {
    /// Run in daemon mode instead of attaching as a client.
    #[arg(short, long)]
    daemon: bool,

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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Up { config } if cli.daemon => run_daemon(&config).await?,
        Command::Up { config } => run_up(&config).await?,
    }

    Ok(())
}

async fn run_up(config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let system = load_config(config_path)?;
    let socket_path = cli::ipc::socket_path(&system);

    ensure_daemon(config_path, &socket_path).await?;

    let app = cli::client::build_up_client(socket_path);
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
