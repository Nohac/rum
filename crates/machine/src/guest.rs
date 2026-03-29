use roam_stream::Connector;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_vsock::{VsockAddr, VsockStream};

use crate::config::PortForward;
use crate::error::Error;

pub const AGENT_BINARY: &[u8] = include_bytes!(env!("CARGO_BIN_FILE_GUEST"));

pub const AGENT_SERVICE: &str = "\
[Unit]
Description=rum guest agent
After=local-fs.target

[Service]
Type=simple
ExecStart=/usr/local/bin/rum-agent
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
";

pub const RPC_PORT: u32 = 2222;
const FORWARD_PORT: u32 = 2223;

#[derive(Clone)]
pub struct VsockConnector {
    cid: u32,
}

impl VsockConnector {
    pub fn new(cid: u32) -> Self {
        Self { cid }
    }
}

impl Connector for VsockConnector {
    type Transport = VsockStream;

    async fn connect(&self) -> std::io::Result<VsockStream> {
        VsockStream::connect(VsockAddr::new(self.cid, RPC_PORT)).await
    }
}

pub async fn start_port_forwards(
    cid: u32,
    ports: &[PortForward],
) -> Result<Vec<JoinHandle<()>>, Error> {
    let mut handles = Vec::new();

    for pf in ports {
        let bind_addr = format!("{}:{}", pf.bind_addr(), pf.host);
        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| Error::Io {
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
