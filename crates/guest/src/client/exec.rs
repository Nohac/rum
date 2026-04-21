use crate::agent::{LogEvent, LogStream};

use super::{Client, ClientError};

impl<C> Client<C>
where
    C: roam_stream::Connector,
{
    pub async fn exec(&self, command: String) -> Result<i32, ClientError> {
        use std::io::Write;

        self.exec_with_output(command, |event| match event.stream {
            LogStream::Stdout => {
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(stdout, "{}", event.message);
                let _ = stdout.flush();
            }
            LogStream::Stderr | LogStream::Log => {
                let mut stderr = std::io::stderr().lock();
                let _ = writeln!(stderr, "{}", event.message);
                let _ = stderr.flush();
            }
        })
        .await
    }

    pub async fn exec_with_output<F>(
        &self,
        command: String,
        on_output: F,
    ) -> Result<i32, ClientError>
    where
        F: Fn(LogEvent) + Send + Sync,
    {
        let (tx, mut rx) = roam::channel::<LogEvent>();
        let agent = self.rpc().clone();
        let exec_task = tokio::spawn(async move { agent.exec(command, tx).await });

        while let Ok(Some(event)) = rx.recv().await {
            on_output(event);
        }

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
        Ok(result.exit_code.unwrap_or(1))
    }
}
