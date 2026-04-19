use std::path::{Path, PathBuf};

use anyhow::Context;
use machine::config::SystemConfig;
use machine::driver::LibvirtDriver;

/// Filter mode for provisioning logs stored in the instance work directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogSelection {
    Latest,
    LatestFailed,
    List,
}

/// Run the local `rum log` command against the current instance work directory.
pub fn run(system: &SystemConfig, selection: LogSelection) -> anyhow::Result<()> {
    let logs_dir = LibvirtDriver::new(system.clone()).layout().logs_dir.clone();

    match selection {
        LogSelection::List => list_logs(&logs_dir),
        LogSelection::Latest => print_latest_log(&logs_dir, false),
        LogSelection::LatestFailed => print_latest_log(&logs_dir, true),
    }
}

fn list_logs(logs_dir: &Path) -> anyhow::Result<()> {
    let entries = sorted_logs(logs_dir, None)?;
    if entries.is_empty() {
        anyhow::bail!("no provisioning logs found in {}", logs_dir.display());
    }

    for path in entries {
        println!(
            "{}",
            path.file_name().and_then(|name| name.to_str()).unwrap_or_default()
        );
    }

    Ok(())
}

fn print_latest_log(logs_dir: &Path, failed_only: bool) -> anyhow::Result<()> {
    let suffix = if failed_only {
        Some("_failed.log")
    } else {
        Some(".log")
    };
    let mut entries = sorted_logs(logs_dir, suffix)?;

    if failed_only {
        entries.retain(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_failed.log"))
        });
    }

    let Some(path) = entries.into_iter().next() else {
        if failed_only {
            anyhow::bail!("no failed provisioning logs found in {}", logs_dir.display());
        }
        anyhow::bail!("no provisioning logs found in {}", logs_dir.display());
    };

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read log file {}", path.display()))?;
    print!("{content}");
    Ok(())
}

fn sorted_logs(logs_dir: &Path, suffix: Option<&str>) -> anyhow::Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(logs_dir)
        .with_context(|| format!("failed to read logs directory {}", logs_dir.display()))?;

    let mut paths: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return false;
            };

            suffix.is_none_or(|suffix| name.ends_with(suffix))
        })
        .collect();

    paths.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    Ok(paths)
}
