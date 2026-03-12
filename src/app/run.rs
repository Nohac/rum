use crate::backend::{self, Backend};
use crate::cli::{Cli, Command, ImageCommand, OutputFormat};
use crate::commands;

async fn run_pre_config_command(cli: &Cli) -> miette::Result<bool> {
    match &cli.command {
        Command::Serve => {
            let sys_config = crate::config::load_config(&cli.config)?;
            commands::serve::run_daemon(&sys_config).await?;
            Ok(true)
        }
        Command::Init { defaults } => {
            crate::init::run(*defaults)?;
            Ok(true)
        }
        Command::Skill => {
            print!("{}", crate::skill::SKILL_DOC);
            Ok(true)
        }
        Command::Image { action } => {
            let cache_dir = crate::paths::cache_dir();
            match action {
                ImageCommand::List => crate::image::list_cached(&cache_dir)?,
                ImageCommand::Delete { name } => crate::image::delete_cached(&cache_dir, name)?,
                ImageCommand::Clear => crate::image::clear_cache(&cache_dir)?,
                ImageCommand::Search { query } => {
                    crate::registry::search(query.as_deref(), &cli.config).await?;
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub async fn run(cli: Cli) -> miette::Result<()> {
    if run_pre_config_command(&cli).await? {
        return Ok(());
    }
    let output_format = super::output::resolve_output_format(&cli.output);
    let mode = super::output::resolve_output_mode(&output_format, cli.verbose, cli.quiet);
    let logging = super::tracing::init_tracing(mode);

    if let Command::Serve | Command::Init { .. } | Command::Image { .. } | Command::Skill =
        cli.command
    {
        unreachable!()
    }

    let sys_config = crate::config::load_config(&cli.config)?;
    let logs_dir = crate::paths::logs_dir(&sys_config.id, sys_config.name.as_deref());
    if matches!(cli.command, Command::Up { .. }) {
        std::fs::create_dir_all(&logs_dir).ok();
        logging.file_handle.set_file(&logs_dir.join("rum.log")).ok();
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
            commands::daemon_client::request_shutdown(&sys_config, false).await?;
            println!("Shutdown requested for '{}'.", sys_config.display_name());
        }
        Command::Destroy => {
            if crate::daemon::is_daemon_running(&sys_config) {
                let _ = commands::daemon_client::request_shutdown(&sys_config, true).await;
            }
            commands::destroy::destroy_cleanup(&sys_config).await?;
        }
        Command::Status => {
            let vm_name = sys_config.display_name();
            let info = match crate::daemon::is_daemon_running(&sys_config) {
                true => commands::daemon_client::request_status(&sys_config).await?,
                false => commands::status::offline_status(&sys_config),
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
            let config_text = commands::daemon_client::request_ssh_config(&sys_config).await?;
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
            let cid = crate::backend::libvirt::get_vsock_cid(&sys_config)?;
            let exit_code = crate::agent::run_exec(cid, args.join(" ")).await?;
            if matches!(output_format, OutputFormat::Json) {
                #[derive(facet::Facet)]
                struct ExecJson {
                    exit_code: i32,
                }
                println!(
                    "{}",
                    facet_json::to_string(&ExecJson { exit_code }).expect("JSON serialization"),
                );
            }
            std::process::exit(exit_code);
        }
        Command::Cp { src, dst } => {
            let direction = crate::agent::parse_copy_args(&src, &dst)?;
            let cid = crate::backend::libvirt::get_vsock_cid(&sys_config)?;
            match direction {
                crate::agent::CopyDirection::Upload { local, guest } => {
                    let bytes = crate::agent::copy_to_guest(cid, &local, &guest).await?;
                    println!("{} -> :{} ({bytes} bytes)", local.display(), guest);
                }
                crate::agent::CopyDirection::Download { guest, local } => {
                    let bytes = crate::agent::copy_from_guest(cid, &guest, &local).await?;
                    println!(":{} -> {} ({bytes} bytes)", guest, local.display());
                }
            }
        }
        Command::Provision { system, boot } => {
            commands::provision::run_provision(&sys_config, system, boot, mode).await?;
        }
        Command::DumpIso { dir } => {
            let mounts = sys_config.resolve_mounts()?;
            let seed_path = dir.join("seed.iso");
            let seed_config = crate::cloudinit::SeedConfig {
                hostname: sys_config.hostname(),
                user_name: &sys_config.config.user.name,
                user_groups: &sys_config.config.user.groups,
                mounts: &mounts,
                autologin: sys_config.config.advanced.autologin,
                ssh_keys: &[],
                agent_binary: None,
            };
            crate::cloudinit::generate_seed_iso(&seed_path, &seed_config).await?;
            println!("Wrote seed ISO to {}", seed_path.display());
        }
    }

    Ok(())
}
