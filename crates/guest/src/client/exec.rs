use crate::agent::{LogEvent, LogStream};

use super::{Client, ClientError};

impl<C> Client<C>
where
    C: roam_stream::Connector,
{
    pub async fn exec(&self, command: String) -> Result<i32, ClientError> {
        use tokio::io::AsyncWriteExt;

        let (tx, mut rx) = roam::channel::<LogEvent>();
        let agent = self.rpc().clone();
        let exec_task = tokio::spawn(async move { agent.exec(command, tx).await });

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
            .map_err(|e| ClientError::Io {
                context: format!("exec task panicked: {e}"),
                source: std::io::Error::other(e.to_string()),
            })?
            .map_err(|message| ClientError::Rpc {
                context: "exec RPC failed".into(),
                message: message.to_string(),
            })?;

        let _ = stream_task.await;
        Ok(result.exit_code.unwrap_or(1))
    }
}
