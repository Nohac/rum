use ecsdk::app::AsyncApp;
use ecsdk::network::IsomorphicApp;
use orchestrator::OrchestratorMessage;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::render::{RenderMode, RumRenderPlugin};

/// Create the shared isomorphic CLI app used by both the daemon and clients.
///
/// This keeps shared plugin registration order identical across server/client
/// builds, which is especially important for request features and replication
/// channel registration.
pub fn create_isomorphic_app(
    socket_path: std::path::PathBuf,
    restart_requested: Arc<AtomicBool>,
) -> IsomorphicApp<OrchestratorMessage> {
    let mut iso = IsomorphicApp::new();
    iso.add_plugin(crate::network::SharedNetworkPlugin::new(socket_path));
    iso.add_plugin(crate::cp::CopyFeature);
    iso.add_plugin(crate::down::DownFeature);
    iso.add_plugin(crate::destroy::DestroyFeature);
    iso.add_plugin(crate::exec::ExecFeature);
    iso.add_plugin(crate::status::StatusFeature);
    iso.add_plugin(crate::restart::ProtocolRestartPlugin::new(
        restart_requested,
    ));
    iso
}

/// Build a client app from the shared isomorphic app.
///
/// Rendering is configured once here so command-specific client builders only
/// need to add their own request/exit behavior.
pub fn build_client_app(
    mut app: AsyncApp<OrchestratorMessage>,
    render_mode: RenderMode,
    render_enabled: bool,
) -> AsyncApp<OrchestratorMessage> {
    if render_enabled {
        app.add_plugins(RumRenderPlugin::new(render_mode));
    }
    app
}
