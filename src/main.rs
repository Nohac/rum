use clap::Parser;
use rum::cli::Cli;

#[tokio::main]
async fn main() -> miette::Result<()> {
    rum::app::run(Cli::parse()).await
}
