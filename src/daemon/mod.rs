mod rpc;
mod client;
mod service;
mod serve;

pub use rpc::{StatusInfo, RumDaemon, RumDaemonClient};
pub use client::{DaemonConnector, DaemonClient, connect, is_daemon_running};
pub use serve::{run_serve, spawn_background, wait_for_daemon_ready};
pub(crate) use serve::ServiceHandles;
