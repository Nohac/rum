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

    /// Suppress log lines, show only step completion
    #[arg(short, long)]
    pub quiet: bool,

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

    /// Connect to the VM via SSH
    Ssh {
        /// Extra arguments passed to ssh
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Print OpenSSH config for the VM
    SshConfig,

    /// Initialize a new rum.toml in the current directory
    Init {
        /// Skip all prompts and use sensible defaults
        #[arg(long)]
        defaults: bool,
    },

    /// Show provisioning and runtime logs
    Log {
        /// Show only the most recent failed script log
        #[arg(long)]
        failed: bool,
        /// List all available script logs
        #[arg(long)]
        all: bool,
        /// Show rum's own internal log
        #[arg(long)]
        rum: bool,
    },

    /// Manage cached base images
    Image {
        #[command(subcommand)]
        action: ImageCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ImageCommand {
    /// List cached images with size and modification time
    List,
    /// Delete a specific cached image by filename
    Delete {
        /// Image filename to delete
        name: String,
    },
    /// Delete all cached images
    Clear,
    /// Search cloud image registry and update rum.toml
    Search {
        /// Filter images by name (e.g. "ubuntu", "fedora")
        query: Option<String>,
    },
}
