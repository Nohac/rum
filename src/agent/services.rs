use rum_agent::{LogEvent, LogLevel, LogStream};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_vsock::{VsockAddr, VsockStream};

use crate::config::PortForward;
use crate::error::RumError;

const FORWARD_PORT: u32 = 2223;

pub(crate) fn start_log_subscription(agent: &super::AgentClient) -> JoinHandle<()> {
    let (tx, mut rx) = roam::channel::<LogEvent>();
    let agent = agent.clone();

    tokio::spawn(async move {
        let subscribe_fut = agent.subscribe_logs(tx);
        tokio::spawn(async move {
            let _ = subscribe_fut.await;
        });

        while let Ok(Some(event)) = rx.recv().await {
            let stream = match event.stream {
                LogStream::Log => "log",
                LogStream::Stdout => "stdout",
                LogStream::Stderr => "stderr",
            };
            match event.level {
                LogLevel::Trace => tracing::trace!(target: "guest", stream, "{}", event.message),
                LogLevel::Debug => tracing::debug!(target: "guest", stream, "{}", event.message),
                LogLevel::Info => tracing::info!(target: "guest", stream, "{}", event.message),
                LogLevel::Warn => tracing::warn!(target: "guest", stream, "{}", event.message),
                LogLevel::Error => tracing::error!(target: "guest", stream, "{}", event.message),
            }
        }
    })
}

pub async fn start_port_forwards(
    cid: u32,
    ports: &[PortForward],
) -> Result<Vec<JoinHandle<()>>, RumError> {
    let mut handles = Vec::new();

    for pf in ports {
        let bind_addr = format!("{}:{}", pf.bind_addr(), pf.host);
        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| RumError::Io {
            context: format!("binding port forward on {bind_addr}"),
            source: e,
        })?;

        let guest_port = pf.guest;
        let host_port = pf.host;

        let handle = tokio::spawn(async move {
            loop {
                let (tcp_stream, _addr) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!(port = host_port, "forward accept error: {e}");
                        continue;
                    }
                };

                tokio::spawn(async move {
                    if let Err(e) = proxy_connection(cid, guest_port, tcp_stream).await {
                        tracing::error!(host_port, guest_port, "forward proxy error: {e}");
                    }
                });
            }
        });

        handles.push(handle);
    }

    Ok(handles)
}

async fn proxy_connection(
    cid: u32,
    guest_port: u16,
    mut tcp: tokio::net::TcpStream,
) -> Result<(), std::io::Error> {
    let mut vsock = VsockStream::connect(VsockAddr::new(cid, FORWARD_PORT)).await?;
    vsock.write_u16(guest_port).await?;
    tokio::io::copy_bidirectional(&mut tcp, &mut vsock).await?;
    Ok(())
}
