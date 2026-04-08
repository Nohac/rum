use ecsdk::prelude::*;
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
