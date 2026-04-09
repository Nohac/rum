use ecsdk::network::IsomorphicApp;
use orchestrator::OrchestratorMessage;

/// Create the shared isomorphic CLI app used by both the daemon and clients.
///
/// This keeps shared plugin registration order identical across server/client
/// builds, which is especially important for request features and replication
/// channel registration.
pub fn create_isomorphic_app(socket_path: std::path::PathBuf) -> IsomorphicApp<OrchestratorMessage> {
    let mut iso = IsomorphicApp::new();
    iso.add_plugin(crate::network::SharedNetworkPlugin::new(socket_path));
    iso.add_plugin(crate::down::DownFeature);
    iso.add_plugin(crate::status::StatusFeature);
    iso
}
