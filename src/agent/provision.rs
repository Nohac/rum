use std::path::Path;
use std::sync::Arc;

pub use rum_agent::{ProvisionScript, RunOn};
use rum_agent::ProvisionEvent;

use crate::error::RumError;
use crate::logging::ScriptLogger;
use crate::progress::StepProgress;

pub async fn run_provision(
    agent: &super::AgentClient,
    scripts: Vec<rum_agent::ProvisionScript>,
    progress: &mut StepProgress,
    logs_dir: &Path,
) -> Result<(), RumError> {
    let script_names: Vec<String> = scripts.iter().map(|s| s.name.clone()).collect();
    let titles: Vec<String> = scripts.iter().map(|s| s.title.clone()).collect();

    let (tx, rx) = roam::channel::<ProvisionEvent>();
    let agent = agent.clone();
    let task = tokio::spawn(async move { agent.provision(scripts, tx).await });

    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let mut failed = false;

    for (i, title) in titles.iter().enumerate() {
        let rx = rx.clone();
        let title_owned = title.clone();
        let mut logger = ScriptLogger::new(logs_dir, &script_names[i]).ok();
        let success = progress
            .run(title, |step| async move {
                let mut rx = rx.lock().await;
                while let Ok(Some(event)) = rx.recv().await {
                    match event {
                        ProvisionEvent::Done(code) => {
                            if let Some(lg) = logger.take() {
                                lg.finish(code == 0);
                            }
                            if code != 0 {
                                step.set_failed();
                                step.set_done_label(format!("{title_owned} (exit code {code})"));
                                return false;
                            }
                            return true;
                        }
                        ProvisionEvent::Stdout(ref line)
                        | ProvisionEvent::Stderr(ref line) => {
                            if let Some(ref mut lg) = logger {
                                lg.write_line(line);
                            }
                            step.log(line);
                        }
                    }
                }
                if let Some(lg) = logger.take() {
                    lg.finish(false);
                }
                step.set_failed();
                step.set_done_label(format!("{title_owned} (connection lost)"));
                false
            })
            .await;

        if !success {
            for remaining in &titles[i + 1..] {
                progress.skip(&format!("{remaining} (skipped)"));
            }
            failed = true;
            break;
        }
    }

    for name in &script_names {
        crate::logging::rotate_logs(logs_dir, name, 10);
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
