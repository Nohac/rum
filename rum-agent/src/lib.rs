use facet::Facet;
use roam::Tx;

#[derive(Debug, Clone, Facet)]
pub struct ReadyResponse {
    pub version: String,
    pub hostname: String,
}

#[derive(Debug, Clone, Facet)]
#[repr(u8)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Facet)]
#[repr(u8)]
pub enum LogStream {
    Log,
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Facet)]
pub struct LogEvent {
    pub timestamp_us: u64,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
    pub stream: LogStream,
}

#[derive(Debug, Clone, Facet)]
pub struct ExecResult {
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Facet)]
#[repr(u8)]
pub enum RunOn {
    System,
    Boot,
}

#[derive(Debug, Clone, Facet)]
pub struct ProvisionScript {
    pub name: String,
    pub content: String,
    pub order: u32,
    pub run_on: RunOn,
}

#[derive(Debug, Clone, Facet)]
pub struct ProvisionResult {
    pub success: bool,
    pub failed_script: String,
}

#[roam::service]
pub trait RumAgent {
    async fn ping(&self) -> Result<ReadyResponse, String>;
    async fn subscribe_logs(&self, output: Tx<LogEvent>);
    async fn exec(&self, command: String, output: Tx<LogEvent>) -> ExecResult;
    async fn provision(&self, scripts: Vec<ProvisionScript>, output: Tx<LogEvent>) -> ProvisionResult;
}
