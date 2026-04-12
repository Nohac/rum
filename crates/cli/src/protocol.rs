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
