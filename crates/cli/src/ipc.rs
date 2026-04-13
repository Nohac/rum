use std::io;
use std::path::{Path, PathBuf};

use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, Name,
    tokio::{Listener, Stream},
};
use interprocess::local_socket::ToFsName as _;
use interprocess::local_socket::traits::tokio::Stream as _;

/// Convert a filesystem socket path into an interprocess socket name.
pub fn socket_name(path: &Path) -> Name<'static> {
    path.to_string_lossy().into_owned().to_fs_name::<GenericFilePath>().unwrap()
}

/// Create the local-socket listener used by the rum daemon.
pub fn create_listener(path: &Path) -> io::Result<Listener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(path);
    ListenerOptions::new().name(socket_name(path)).create_tokio()
}

/// Connect a client to the daemon socket.
pub async fn connect(path: &Path) -> io::Result<Stream> {
    Stream::connect(socket_name(path)).await
}

/// Derive the daemon socket path for one system config.
pub fn socket_path(system: &machine::config::SystemConfig) -> PathBuf {
    machine::paths::socket_path(&system.id, system.name.as_deref())
}

/// Derive the control sidechannel socket path for one system config.
pub fn control_socket_path(system: &machine::config::SystemConfig) -> PathBuf {
    let socket_path = socket_path(system);
    let stem = socket_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("rum");
    socket_path.with_file_name(format!("{stem}.control.sock"))
}
