use std::io::IsTerminal;
use std::path::PathBuf;

use inquire::Confirm;

use crate::error::RumError;
use super::errors::map_inquire_err;
use super::flow::run_wizard;
use super::toml::{default_config, generate_toml};

pub fn run(defaults: bool) -> Result<(), RumError> {
    let output_path = PathBuf::from("rum.toml");

    if output_path.exists() && defaults {
        return Err(RumError::Validation {
            message: "rum.toml already exists (use interactive mode to overwrite)".into(),
        });
    }

    if !defaults && !std::io::stdin().is_terminal() {
        return Err(RumError::Validation {
            message: "rum init requires a terminal for the interactive wizard. Use --defaults for non-interactive mode.".into(),
        });
    }

    if output_path.exists() {
        let overwrite = Confirm::new("rum.toml already exists. Overwrite?")
            .with_default(false)
            .prompt()
            .map_err(map_inquire_err)?;
        if !overwrite {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let config = if defaults {
        default_config()
    } else {
        run_wizard()?
    };

    let toml = generate_toml(&config);
    std::fs::write(&output_path, &toml).map_err(|e| RumError::ConfigWrite {
        path: output_path.display().to_string(),
        source: e,
    })?;

    println!("Created rum.toml");
    println!("Run `rum up` to start the VM.");
    Ok(())
}
