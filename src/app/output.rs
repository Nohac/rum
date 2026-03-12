use std::io::IsTerminal;

use crate::cli::OutputFormat;
use crate::progress::OutputMode;

pub fn resolve_output_format(format: &OutputFormat) -> OutputFormat {
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

pub fn resolve_output_mode(format: &OutputFormat, verbose: bool, quiet: bool) -> OutputMode {
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
        OutputFormat::Auto => OutputMode::Normal,
    }
}
