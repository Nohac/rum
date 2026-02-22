use std::io::IsTerminal;

use clap::Parser;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use rum::backend::{self, Backend};
use rum::cli::{Cli, Command, ImageCommand};
use rum::config;
use rum::logging;
use rum::progress::OutputMode;

#[tokio::main]
async fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    let mode = if cli.quiet {
        OutputMode::Quiet
    } else if cli.verbose {
        OutputMode::Verbose
    } else if !std::io::stdout().is_terminal() {
        OutputMode::Plain
    } else {
        OutputMode::Normal
    };

    // Terminal layer: suppress tracing when the progress UI manages the terminal
    // (Normal/Quiet). Tracing output to stderr corrupts indicatif's terminal
    // line tracking, causing redraws to clear completed steps.
    let terminal_filter = match mode {
        OutputMode::Verbose => EnvFilter::new("debug"),
        OutputMode::Normal | OutputMode::Quiet => EnvFilter::new("off"),
        OutputMode::Plain => EnvFilter::from_default_env()
            .add_directive("rum=info".parse().expect("valid log directive")),
    };

    let terminal_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(terminal_filter);

    // File layer: always captures rum=debug, initially discards until activated
    let (file_writer, file_handle) = logging::DeferredFileWriter::new();
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_writer)
        .with_filter(EnvFilter::new("rum=debug"));

    tracing_subscriber::registry()
        .with(terminal_layer)
        .with(file_layer)
        .init();

    // Handle init before loading config — it creates the config
    if let Command::Init { defaults } = cli.command {
        return rum::init::run(defaults).map_err(Into::into);
    }

    // Clone config path before moving cli.command — Search needs it
    let config_path = cli.config.clone();

    // Handle image commands before loading config — they don't need a rum.toml
    if let Command::Image { action } = cli.command {
        let cache_dir = rum::paths::cache_dir();
        return match action {
            ImageCommand::List => rum::image::list_cached(&cache_dir).map_err(Into::into),
            ImageCommand::Delete { name } => {
                rum::image::delete_cached(&cache_dir, &name).map_err(Into::into)
            }
            ImageCommand::Clear => rum::image::clear_cache(&cache_dir).map_err(Into::into),
            ImageCommand::Search { query } => {
                rum::registry::search(query.as_deref(), &config_path)
                    .await
                    .map_err(Into::into)
            }
        };
    }

    let sys_config = config::load_config(&cli.config)?;

    // Activate file logging for commands that run the VM
    let logs_dir = rum::paths::logs_dir(&sys_config.id, sys_config.name.as_deref());
    if matches!(cli.command, Command::Up { .. }) {
        std::fs::create_dir_all(&logs_dir).ok();
        file_handle.set_file(&logs_dir.join("rum.log")).ok();
    }

    let backend = backend::create_backend();

    match cli.command {
        Command::Init { .. } | Command::Image { .. } => unreachable!(),
        Command::Up { reset } => backend.up(&sys_config, reset, mode).await?,
        Command::Down => backend.down(&sys_config).await?,
        Command::Destroy { purge } => backend.destroy(&sys_config, purge).await?,
        Command::Status => backend.status(&sys_config).await?,
        Command::Ssh { args } => backend.ssh(&sys_config, &args).await?,
        Command::SshConfig => backend.ssh_config(&sys_config).await?,
        Command::Log { failed, all, rum } => {
            handle_log_command(&logs_dir, failed, all, rum)?;
        }
        Command::DumpIso { dir } => {
            use rum::cloudinit;
            let mounts = sys_config.resolve_mounts()?;
            let seed_path = dir.join("seed.iso");
            let seed_config = cloudinit::SeedConfig {
                hostname: sys_config.hostname(),
                user_name: &sys_config.config.user.name,
                user_groups: &sys_config.config.user.groups,
                mounts: &mounts,
                autologin: sys_config.config.advanced.autologin,
                ssh_keys: &[],
                agent_binary: None,
            };
            cloudinit::generate_seed_iso(&seed_path, &seed_config).await?;
            println!("Wrote seed ISO to {}", seed_path.display());
        }
    }

    Ok(())
}

fn handle_log_command(
    logs_dir: &std::path::Path,
    failed: bool,
    all: bool,
    rum_log: bool,
) -> miette::Result<()> {
    if rum_log {
        let rum_log_path = logs_dir.join("rum.log");
        if rum_log_path.exists() {
            let contents = std::fs::read_to_string(&rum_log_path).map_err(|e| {
                rum::error::RumError::Io {
                    context: format!("reading {}", rum_log_path.display()),
                    source: e,
                }
            })?;
            print!("{contents}");
        } else {
            println!("No rum.log found. Run `rum up` first.");
        }
        return Ok(());
    }

    if all {
        let logs = logging::list_script_logs(logs_dir);
        if logs.is_empty() {
            println!("No script logs found.");
        } else {
            for entry in &logs {
                let status_indicator = if entry.status == "failed" {
                    "FAIL"
                } else {
                    " OK "
                };
                println!(
                    "[{status_indicator}] {} {} ({})",
                    entry.timestamp,
                    entry.script_name,
                    entry.path.display()
                );
            }
        }
        return Ok(());
    }

    // Default / --failed: show the latest script log (optionally failed-only)
    match logging::latest_script_log(logs_dir, failed) {
        Some(path) => {
            let contents =
                std::fs::read_to_string(&path).map_err(|e| rum::error::RumError::Io {
                    context: format!("reading {}", path.display()),
                    source: e,
                })?;
            let fname = path.file_name().and_then(|f| f.to_str()).unwrap_or("?");
            println!("--- {fname} ---");
            print!("{contents}");
        }
        None => {
            if failed {
                println!("No failed script logs found.");
            } else {
                println!("No script logs found. Run `rum up` first.");
            }
        }
    }

    Ok(())
}
