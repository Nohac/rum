use clap::Parser;
use tracing_subscriber::EnvFilter;

use rum::backend::{self, Backend};
use rum::cli::{Cli, Command};
use rum::config;

#[tokio::main]
async fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::from_default_env().add_directive("rum=info".parse().expect("valid log directive"))
    };

    tracing_subscriber::fmt().with_env_filter(filter).init();

    let config = config::load_config(&cli.config)?;
    let backend = backend::create_backend();

    match cli.command {
        Command::Up { reset } => backend.up(&config, &cli.config, reset).await?,
        Command::Down => backend.down(&config).await?,
        Command::Destroy { purge } => backend.destroy(&config, purge).await?,
        Command::Status => backend.status(&config).await?,
        Command::DumpIso { dir } => {
            use rum::cloudinit;
            let mounts = config.resolve_mounts(&cli.config)?;
            let seed_path = dir.join("seed.iso");
            cloudinit::generate_seed_iso(
                &seed_path,
                config.hostname(),
                &config.provision.script,
                &config.provision.packages,
                &mounts,
            )
            .await?;
            println!("Wrote seed ISO to {}", seed_path.display());
        }
    }

    Ok(())
}
