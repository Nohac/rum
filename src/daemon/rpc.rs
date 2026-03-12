use facet::Facet;

// ── Roam service definition ─────────────────────────────────────────

#[derive(Debug, Clone, Facet)]
pub struct StatusInfo {
    pub state: String,
    pub ips: Vec<String>,
    pub daemon_running: bool,
}

#[roam::service]
pub trait RumDaemon {
    async fn ping(&self) -> Result<String, String>;
    async fn shutdown(&self) -> Result<String, String>;
    async fn force_stop(&self) -> Result<String, String>;
    async fn status(&self) -> Result<StatusInfo, String>;
    async fn ssh_config(&self) -> Result<String, String>;
}
