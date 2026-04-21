use std::path::PathBuf;

use ecsdk::prelude::*;
use machine::instance::InstanceState;
use orchestrator::InstancePhase;
use serde::{Deserialize, Serialize};

/// Client requests that the daemon shut down the managed machine.
#[derive(Default, Event, ClientRequest, Serialize, Deserialize)]
#[request(response = "DownResponse")]
pub struct DownRequest;

/// Server acknowledges a shutdown request.
#[derive(Event, Serialize, Deserialize)]
pub struct DownResponse {
    pub accepted: bool,
}

/// Client requests that the daemon destroy the managed machine and purge its
/// persisted state directory.
#[derive(Default, Event, ClientRequest, Serialize, Deserialize)]
#[request(response = "DestroyResponse")]
pub struct DestroyRequest;

/// Server acknowledges a destroy request.
#[derive(Event, Serialize, Deserialize)]
pub struct DestroyResponse {
    pub accepted: bool,
}

/// Direction and resolved paths for a guest file copy request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CopySpec {
    Upload { local: PathBuf, guest: String },
    Download { guest: String, local: PathBuf },
}

/// Client requests that the daemon copy files to or from the managed guest.
#[derive(Default, Clone, Event, ClientRequest, Serialize, Deserialize)]
#[request(response = "CopyResponse")]
pub struct CopyRequest {
    pub spec: Option<CopySpec>,
}

/// Result of a file-copy request handled by the daemon.
#[derive(Event, Serialize, Deserialize)]
pub struct CopyResponse {
    pub success: bool,
    pub message: String,
}

/// Client requests that the daemon execute a shell command in the managed
/// guest and stream its output through the replicated log pipeline.
#[derive(Default, Clone, Event, ClientRequest, Serialize, Deserialize)]
#[request(response = "ExecResponse")]
pub struct ExecRequest {
    pub command: Option<String>,
}

/// Final result of a guest exec request handled by the daemon.
#[derive(Event, Serialize, Deserialize)]
pub struct ExecResponse {
    pub success: bool,
    pub exit_code: i32,
    pub message: Option<String>,
}

/// Client requests a one-shot status snapshot from the daemon.
#[derive(Default, Event, ClientRequest, Serialize, Deserialize)]
#[request(response = "StatusResponse")]
pub struct StatusRequest;

/// Snapshot of the currently managed instance known by the daemon.
#[derive(Event, Serialize, Deserialize)]
pub struct StatusResponse {
    pub found: bool,
    pub label: Option<String>,
    pub recovered_state: Option<InstanceState>,
    pub phase: Option<InstancePhase>,
    pub error: Option<String>,
}
