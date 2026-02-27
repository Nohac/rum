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
    } else if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
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

    // Handle serve before normal config loading — daemon mode
    if matches!(cli.command, Command::Serve) {
        let sys_config = config::load_config(&cli.config)?;
        rum::daemon::run_serve(&sys_config).await?;
        return Ok(());
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
            let effective_detach = detach || mode == OutputMode::Plain;
            backend
                .up(&sys_config, reset, effective_detach, mode)
                .await?
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
            destroy_cleanup(&sys_config).await?;
        }
        Command::Status => {
            let vm_name = sys_config.display_name();

            match rum::daemon::connect(&sys_config) {
                Ok(client) => {
                    let info = client
                        .status()
                        .await
                        .map_err(|e| rum::error::RumError::Daemon {
                            message: e.to_string(),
                        })?;

                    println!("VM '{vm_name}': {}", info.state);
                    for ip in &info.ips {
                        println!("  IP: {ip}");
                    }
                    if info.daemon_running {
                        println!("  Daemon: running");
                    }
                }
                Err(_) => {
                    // No daemon — do an offline status check via libvirt
                    let info = offline_status(&sys_config);
                    println!("VM '{vm_name}': {}", info.state);
                    for ip in &info.ips {
                        println!("  IP: {ip}");
                    }
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
            handle_log_command(&logs_dir, failed, all, rum)?;
        }
        Command::Exec { args } => {
            if args.is_empty() {
                eprintln!("Usage: rum exec <command> [args...]");
                std::process::exit(1);
            }
            let command = args.join(" ");
            let cid = rum::backend::libvirt::get_vsock_cid(&sys_config)?;
            let exit_code = rum::agent::run_exec(cid, command).await?;
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
            let cid = rum::backend::libvirt::get_vsock_cid(&sys_config)?;

            // Build provision scripts (same pattern as libvirt.rs up())
            let config = &sys_config.config;
            let drives = sys_config.resolve_drives()?;
            let resolved_fs = sys_config.resolve_fs(&drives)?;
            let mut provision_scripts = Vec::new();

            if !resolved_fs.is_empty() {
                provision_scripts.push(rum::agent::ProvisionScript {
                    name: "rum-drives".into(),
                    title: "Setting up drives and filesystems".into(),
                    content: rum::cloudinit::build_drive_script(&resolved_fs),
                    order: 0,
                    run_on: rum::agent::RunOn::System,
                });
            }
            if let Some(ref sys) = config.provision.system {
                provision_scripts.push(rum::agent::ProvisionScript {
                    name: "rum-system".into(),
                    title: "Running system provisioning".into(),
                    content: sys.script.clone(),
                    order: 1,
                    run_on: rum::agent::RunOn::System,
                });
            }
            if let Some(ref boot_cfg) = config.provision.boot {
                provision_scripts.push(rum::agent::ProvisionScript {
                    name: "rum-boot".into(),
                    title: "Running boot provisioning".into(),
                    content: boot_cfg.script.clone(),
                    order: 2,
                    run_on: rum::agent::RunOn::Boot,
                });
            }

            // Filter by flags: --system = only System, --boot = only Boot, neither = all
            let scripts: Vec<_> = if system && !boot {
                provision_scripts
                    .into_iter()
                    .filter(|s| matches!(s.run_on, rum::agent::RunOn::System))
                    .collect()
            } else if boot && !system {
                provision_scripts
                    .into_iter()
                    .filter(|s| matches!(s.run_on, rum::agent::RunOn::Boot))
                    .collect()
            } else {
                provision_scripts
            };

            if scripts.is_empty() {
                println!("No provisioning scripts to run.");
                return Ok(());
            }

            let agent = rum::agent::wait_for_agent(cid).await?;
            let logs_dir = rum::paths::logs_dir(&sys_config.id, sys_config.name.as_deref());
            std::fs::create_dir_all(&logs_dir).ok();

            let total_steps = scripts.len();
            let mut progress = rum::progress::StepProgress::new(total_steps, mode);
            rum::agent::run_provision(&agent, scripts, &mut progress, &logs_dir).await?;
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

/// Local cleanup after force-stopping a VM via roam: undefine domain,
/// tear down auto-created networks, remove work dir.
async fn destroy_cleanup(sys_config: &rum::config::SystemConfig) -> miette::Result<()> {
    use virt::connect::Connect;
    use virt::domain::Domain;
    use virt::network::Network;

    let id = &sys_config.id;
    let name_opt = sys_config.name.as_deref();
    let vm_name = sys_config.display_name();
    let config = &sys_config.config;

    virt::error::clear_error_callback();
    let mut had_domain = false;
    let mut had_artifacts = false;

    if let Ok(mut conn) = Connect::open(Some(sys_config.libvirt_uri())) {
        if let Ok(dom) = Domain::lookup_by_name(&conn, vm_name) {
            had_domain = true;
            if dom.is_active().unwrap_or(false) {
                let _ = dom.destroy();
            }
            let _ = dom.undefine();
        }

        // Tear down auto-created networks
        for iface in &config.network.interfaces {
            let net_name = rum::network_xml::prefixed_name(id, &iface.network);
            if let Ok(net) = Network::lookup_by_name(&conn, &net_name) {
                if net.is_active().unwrap_or(false) {
                    let _ = net.destroy();
                }
                let _ = net.undefine();
            }
        }

        let _ = conn.close();
    }

    // Remove work dir
    let work = rum::paths::work_dir(id, name_opt);
    if work.exists() {
        had_artifacts = true;
        tokio::fs::remove_dir_all(&work)
            .await
            .map_err(|e| rum::error::RumError::Io {
                context: format!("removing {}", work.display()),
                source: e,
            })?;
    }

    match (had_domain, had_artifacts) {
        (true, _) => println!("VM '{vm_name}' destroyed."),
        (false, true) => println!("Removed artifacts for '{vm_name}'."),
        (false, false) => println!("VM '{vm_name}' not found — nothing to destroy."),
    }

    Ok(())
}

/// Offline status check when no daemon is running.
/// Queries libvirt directly to determine VM state.
fn offline_status(sys_config: &rum::config::SystemConfig) -> rum::daemon::StatusInfo {
    use virt::connect::Connect;
    use virt::domain::Domain;

    virt::error::clear_error_callback();
    let vm_name = sys_config.display_name();

    let Ok(mut conn) = Connect::open(Some(sys_config.libvirt_uri())) else {
        return rum::daemon::StatusInfo {
            state: "not defined".to_string(),
            ips: Vec::new(),
            daemon_running: false,
        };
    };

    let info = match Domain::lookup_by_name(&conn, vm_name) {
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

            rum::daemon::StatusInfo {
                state,
                ips,
                daemon_running: false,
            }
        }
        Err(_) => rum::daemon::StatusInfo {
            state: "not defined".to_string(),
            ips: Vec::new(),
            daemon_running: false,
        },
    };

    let _ = conn.close();
    info
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
