use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

pub use agent::{ProvisionScript, RunOn};
use agent::ProvisionEvent;

use crate::error::RumError;

pub async fn run_provision(
    agent: &super::AgentClient,
    scripts: Vec<agent::ProvisionScript>,
    logs_dir: &Path,
) -> Result<(), RumError> {
    let script_names: Vec<String> = scripts.iter().map(|s| s.name.clone()).collect();

    let (tx, rx) = roam::channel::<ProvisionEvent>();
    let agent = agent.clone();
    let task = tokio::spawn(async move { agent.provision(scripts, tx).await });

    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let mut failed = false;

    for script_name in &script_names {
        let rx = rx.clone();
        let mut logger = ScriptLogger::new(logs_dir, script_name).ok();
        let success = async move {
            let mut rx = rx.lock().await;
            while let Ok(Some(event)) = rx.recv().await {
                match event {
                    ProvisionEvent::Done(code) => {
                        if let Some(lg) = logger.take() {
                            lg.finish(code == 0);
                        }
                        return code == 0;
                    }
                    ProvisionEvent::Stdout(ref line) | ProvisionEvent::Stderr(ref line) => {
                        if let Some(ref mut lg) = logger {
                            lg.write_line(line);
                        }
                    }
                }
            }
            if let Some(lg) = logger.take() {
                lg.finish(false);
            }
            false
        }
        .await;

        if !success {
            failed = true;
            break;
        }
    }

    for name in &script_names {
        rotate_logs(logs_dir, name, 10);
    }

    let result = task
        .await
        .map_err(|e| RumError::Io {
            context: format!("provision task panicked: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?
        .map_err(|e| RumError::Io {
            context: format!("provision RPC failed: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?;

    if failed || !result.success {
        return Err(RumError::ProvisionFailed {
            script: result.failed_script,
        });
    }

    Ok(())
}

struct ScriptLogger {
    file: std::fs::File,
    path: std::path::PathBuf,
}

impl ScriptLogger {
    fn new(logs_dir: &Path, script_name: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(logs_dir)?;
        let filename = format!("{}_{}_running.log", utc_timestamp(), script_name);
        let path = logs_dir.join(filename);
        let file = std::fs::File::create(&path)?;
        Ok(Self { file, path })
    }

    fn write_line(&mut self, line: &str) {
        use std::io::Write;

        let _ = writeln!(self.file, "{line}");
    }

    fn finish(self, success: bool) {
        let suffix = if success { "ok" } else { "failed" };
        let Some(name) = self.path.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        let new_path = self
            .path
            .with_file_name(name.replace("_running.log", &format!("_{suffix}.log")));
        let _ = std::fs::rename(&self.path, new_path);
    }
}

fn rotate_logs(logs_dir: &Path, script_name: &str, keep: usize) {
    let Ok(entries) = std::fs::read_dir(logs_dir) else {
        return;
    };

    let mut matching: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
                name.contains(&format!("_{script_name}_"))
                    && (name.ends_with("_ok.log") || name.ends_with("_failed.log"))
            })
        })
        .collect();
    matching.sort();

    if matching.len() > keep {
        for old in &matching[..matching.len() - keep] {
            let _ = std::fs::remove_file(old);
        }
    }
}

fn utc_timestamp() -> String {
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    let days = (secs / 86_400) as i64;
    let time_of_day = secs % 86_400;
    let hours = time_of_day / 3_600;
    let minutes = (time_of_day % 3_600) / 60;
    let seconds = time_of_day % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{day:02}T{hours:02}-{minutes:02}-{seconds:02}")
}
