use std::io::IsTerminal;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use rum::backend::{self, Backend};
use rum::cli::{Cli, Command};
use rum::config;
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

    // Suppress tracing when the progress UI manages the terminal (Normal/Quiet).
    // Tracing output to stderr corrupts indicatif's terminal line tracking,
    // causing redraws to clear completed steps.
    let filter = match mode {
        OutputMode::Verbose => EnvFilter::new("debug"),
        OutputMode::Normal | OutputMode::Quiet => EnvFilter::new("off"),
        OutputMode::Plain => EnvFilter::from_default_env()
            .add_directive("rum=info".parse().expect("valid log directive")),
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Handle init before loading config — it creates the config
    if let Command::Init { defaults } = cli.command {
        return rum::init::run(defaults).map_err(Into::into);
    }

    let sys_config = config::load_config(&cli.config)?;
    let backend = backend::create_backend();

    match cli.command {
        Command::Init { .. } => unreachable!(),
        Command::Up { reset } => backend.up(&sys_config, reset, mode).await?,
        Command::Down => backend.down(&sys_config).await?,
        Command::Destroy { purge } => backend.destroy(&sys_config, purge).await?,
        Command::Status => backend.status(&sys_config).await?,
        Command::Ssh { args } => backend.ssh(&sys_config, &args).await?,
        Command::SshConfig => backend.ssh_config(&sys_config).await?,
        Command::DumpIso { dir } => {
            use rum::cloudinit;
            let mounts = sys_config.resolve_mounts()?;
            let seed_path = dir.join("seed.iso");
            cloudinit::generate_seed_iso(
                &seed_path,
                sys_config.hostname(),
                &mounts,
                sys_config.config.advanced.autologin,
                &[],
                None,
            )
            .await?;
            println!("Wrote seed ISO to {}", seed_path.display());
        }
    }

    Ok(())
}
