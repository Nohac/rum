use std::io::IsTerminal;

use clap::Parser;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use rum::backend::{self, Backend};
use rum::cli::{Cli, Command, ImageCommand, OutputFormat};
use rum::commands;
use rum::config;
use rum::logging;
use rum::progress::OutputMode;

#[tokio::main]
async fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    // Handle serve before tracing init — daemon sets up its own subscriber
    if matches!(cli.command, Command::Serve) {
        let sys_config = config::load_config(&cli.config)?;
        commands::serve::run_daemon(&sys_config).await?;
        return Ok(());
    }

    let output_format = resolve_output_format(&cli.output);
    let mode = resolve_output_mode(&output_format, cli.verbose, cli.quiet);

    // Terminal layer: suppress tracing when the progress UI manages the terminal
    // (Normal/Quiet). Tracing output to stderr corrupts indicatif's terminal
    // line tracking, causing redraws to clear completed steps.
    let terminal_filter = match mode {
        OutputMode::Verbose => EnvFilter::new("debug"),
        OutputMode::Normal | OutputMode::Quiet | OutputMode::Silent => EnvFilter::new("off"),
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

    // Handle skill before loading config — it doesn't need rum.toml
    if matches!(cli.command, Command::Skill) {
        print!("{}", rum::skill::SKILL_DOC);
        return Ok(());
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
        Command::Init { .. } | Command::Image { .. } | Command::Skill | Command::Serve => {
            unreachable!()
        }
        Command::Up { reset, detach } => {
            commands::client::run_up(&sys_config, reset, detach, &output_format).await?
        }
        Command::Down => {
            let client = rum::daemon::connect(&sys_config)?;
            let msg = client
                .shutdown()
                .await
                .map_err(|e| rum::error::RumError::Daemon {
                    message: e.to_string(),
                })?;
            println!("{msg}");
        }
        Command::Destroy => {
            // Stop VM + daemon via roam (if daemon is running)
            if let Ok(client) = rum::daemon::connect(&sys_config) {
                let _ = client.force_stop().await;
            }

            // Local cleanup: undefine, network teardown, work dir removal
            commands::destroy::destroy_cleanup(&sys_config).await?;
        }
        Command::Status => {
            let vm_name = sys_config.display_name();

            let info = match rum::daemon::connect(&sys_config) {
                Ok(client) => {
                    client
                        .status()
                        .await
                        .map_err(|e| rum::error::RumError::Daemon {
                            message: e.to_string(),
                        })?
                }
                Err(_) => commands::status::offline_status(&sys_config),
            };

            if matches!(output_format, OutputFormat::Json) {
                println!(
                    "{}",
                    facet_json::to_string(&commands::status::StatusJson {
                        name: vm_name.to_string(),
                        state: info.state,
                        ips: info.ips,
                        daemon: info.daemon_running,
                    })
                    .expect("JSON serialization"),
                );
            } else {
                println!("VM '{vm_name}': {}", info.state);
                for ip in &info.ips {
                    println!("  IP: {ip}");
                }
                if info.daemon_running {
                    println!("  Daemon: running");
                }
            }
        }
        Command::Ssh { args } => backend.ssh(&sys_config, &args).await?,
        Command::SshConfig => {
            let client = rum::daemon::connect(&sys_config)?;
            let config_text = client
                .ssh_config()
                .await
                .map_err(|e| rum::error::RumError::Daemon {
                    message: e.to_string(),
                })?;
            print!("{config_text}");
        }
        Command::Log { failed, all, rum } => {
            commands::log::handle_log_command(&logs_dir, failed, all, rum)?;
        }
        Command::Exec { args } => {
            if args.is_empty() {
                eprintln!("Usage: rum exec <command> [args...]");
                std::process::exit(1);
            }
            let command = args.join(" ");
            let cid = rum::backend::libvirt::get_vsock_cid(&sys_config)?;
            let exit_code = rum::agent::run_exec(cid, command).await?;
            if matches!(output_format, OutputFormat::Json) {
                #[derive(facet::Facet)]
                struct ExecJson { exit_code: i32 }
                println!(
                    "{}",
                    facet_json::to_string(&ExecJson { exit_code })
                    .expect("JSON serialization"),
                );
            }
            std::process::exit(exit_code);
        }
        Command::Cp { src, dst } => {
            let direction = rum::agent::parse_copy_args(&src, &dst)?;
            let cid = rum::backend::libvirt::get_vsock_cid(&sys_config)?;
            match direction {
                rum::agent::CopyDirection::Upload { local, guest } => {
                    let bytes = rum::agent::copy_to_guest(cid, &local, &guest).await?;
                    println!(
                        "{} -> :{} ({bytes} bytes)",
                        local.display(),
                        guest,
                    );
                }
                rum::agent::CopyDirection::Download { guest, local } => {
                    let bytes = rum::agent::copy_from_guest(cid, &guest, &local).await?;
                    println!(
                        ":{} -> {} ({bytes} bytes)",
                        guest,
                        local.display(),
                    );
                }
            }
        }
        Command::Provision { system, boot } => {
            commands::provision::run_provision(&sys_config, system, boot, mode).await?;
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

/// Resolve `Auto` to a concrete format based on terminal detection.
fn resolve_output_format(format: &OutputFormat) -> OutputFormat {
    match format {
        OutputFormat::Auto => {
            if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
                OutputFormat::Plain
            } else {
                OutputFormat::Interactive
            }
        }
        other => other.clone(),
    }
}

/// Map the resolved output format (plus `--verbose`/`--quiet` modifiers)
/// into the internal `OutputMode` used by `StepProgress`.
fn resolve_output_mode(format: &OutputFormat, verbose: bool, quiet: bool) -> OutputMode {
    match format {
        OutputFormat::Json => {
            if verbose || quiet {
                eprintln!("warning: --verbose/--quiet ignored in JSON output mode");
            }
            OutputMode::Plain
        }
        OutputFormat::Plain => {
            if quiet {
                OutputMode::Quiet
            } else if verbose {
                OutputMode::Verbose
            } else {
                OutputMode::Plain
            }
        }
        OutputFormat::Interactive => {
            if quiet {
                OutputMode::Quiet
            } else if verbose {
                OutputMode::Verbose
            } else {
                OutputMode::Normal
            }
        }
        OutputFormat::Auto => {
            // Already resolved by resolve_output_format, but handle defensively
            OutputMode::Normal
        }
    }
}
