use crate::error::RumError;

pub fn handle_log_command(
    logs_dir: &std::path::Path,
    failed: bool,
    all: bool,
    rum_log: bool,
) -> Result<(), RumError> {
    if rum_log {
        let rum_log_path = logs_dir.join("rum.log");
        if rum_log_path.exists() {
            let contents = std::fs::read_to_string(&rum_log_path).map_err(|e| RumError::Io {
                context: format!("reading {}", rum_log_path.display()),
                source: e,
            })?;
            print!("{contents}");
        } else {
            println!("No rum.log found. Run `rum up` first.");
        }
        return Ok(());
    }

    if all {
        let logs = crate::logging::list_script_logs(logs_dir);
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
    match crate::logging::latest_script_log(logs_dir, failed) {
        Some(path) => {
            let contents =
                std::fs::read_to_string(&path).map_err(|e| RumError::Io {
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
