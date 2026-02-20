use facet::Facet;

#[derive(Debug, Clone, Facet)]
pub struct ReadyResponse {
    pub version: String,
    pub hostname: String,
}

#[roam::service]
pub trait RumAgent {
    async fn ping(&self) -> Result<ReadyResponse, String>;
}
