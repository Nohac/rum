use rum_agent::{LogEvent, LogStream};
use tokio::io::AsyncWriteExt;

use crate::error::RumError;

pub async fn run_exec(cid: u32, command: String) -> Result<i32, RumError> {
    let agent = super::wait_for_agent(cid).await?;
    let (tx, mut rx) = roam::channel::<LogEvent>();
    let exec_task = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.exec(command, tx).await })
    };

    let stream_task = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        let mut stderr = tokio::io::stderr();
        while let Ok(Some(event)) = rx.recv().await {
            match event.stream {
                LogStream::Stdout => {
                    let _ = stdout.write_all(event.message.as_bytes()).await;
                    let _ = stdout.write_all(b"\n").await;
                    let _ = stdout.flush().await;
                }
                LogStream::Stderr | LogStream::Log => {
                    let _ = stderr.write_all(event.message.as_bytes()).await;
                    let _ = stderr.write_all(b"\n").await;
                    let _ = stderr.flush().await;
                }
            }
        }
    });

    let result = exec_task
        .await
        .map_err(|e| RumError::Io {
            context: format!("exec task panicked: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?
        .map_err(|e| RumError::Io {
            context: format!("exec RPC failed: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?;

    let _ = stream_task.await;
    Ok(result.exit_code.unwrap_or(1))
}
