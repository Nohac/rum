use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "rum", about = "Lightweight VM provisioning via libvirt")]
pub struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "rum.toml")]
    pub config: PathBuf,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create and start the VM
    Up {
        /// Wipe overlay + seed to force fresh first boot
        #[arg(long)]
        reset: bool,
    },

    /// Stop the VM
    Down,

    /// Undefine VM and remove artifacts
    Destroy {
        /// Also remove cached base image
        #[arg(long)]
        purge: bool,
    },

    /// Show VM state
    Status,

    /// Dump the generated cloud-init seed ISO to a directory (for debugging)
    DumpIso {
        /// Output directory
        dir: PathBuf,
    },
}
