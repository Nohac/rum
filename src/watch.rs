use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use russh::client;
use russh::keys::{self, PrivateKeyWithHashAlg};
use tokio::sync::mpsc;
use virt::connect::Connect;
use virt::domain::Domain;

use crate::config::ResolvedMount;

/// Start an inotify bridge that watches host-side mount sources and triggers
/// guest-side inotify events via SSH `touch` commands.
///
/// The returned task waits for the VM IP in the background, then starts
/// watching. It runs until aborted. Only watches mounts with `inotify = true`
/// and `readonly = false`.
pub fn start_inotify_bridge(
    mounts: &[ResolvedMount],
    libvirt_uri: String,
    vm_name: String,
    ssh_user: String,
    ssh_key_path: PathBuf,
) -> tokio::task::JoinHandle<()> {
    // Collect watchable mounts (owned, for move into task)
    let watch_mounts: Vec<(PathBuf, String)> = mounts
        .iter()
        .filter(|m| m.inotify && !m.readonly)
        .map(|m| (m.source.clone(), m.target.clone()))
        .collect();

    tokio::spawn(async move {
        if watch_mounts.is_empty() {
            return;
        }

        // Wait for VM IP in the background using its own libvirt connection
        let ip = match wait_for_ip(&libvirt_uri, &vm_name).await {
            Some(ip) => ip,
            None => {
                tracing::debug!("inotify bridge: gave up waiting for VM IP");
                return;
            }
        };
        tracing::info!(ip, "inotify bridge: VM IP acquired, starting watchers");

        // Establish persistent SSH connection via russh (retry until sshd is up)
        let mut handle = loop {
            if let Some(h) = ssh_connect(&ip, &ssh_user, &ssh_key_path).await {
                break h;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        };

        // Build path translation map, sorted longest-prefix-first
        let mut path_map: Vec<(PathBuf, String)> = watch_mounts.clone();
        path_map.sort_by(|a, b| b.0.as_os_str().len().cmp(&a.0.as_os_str().len()));

        // Set up file watcher with tokio channel
        let (tx, rx) = mpsc::unbounded_channel();

        let mut watcher = match RecommendedWatcher::new(
            move |event| {
                let _ = tx.send(event);
            },
            notify::Config::default(),
        ) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("inotify bridge: failed to create watcher: {e}");
                return;
            }
        };

        for (source, target) in &watch_mounts {
            tracing::info!(
                source = %source.display(),
                target,
                "watching mount for inotify bridge"
            );
            if let Err(e) = watcher.watch(source, RecursiveMode::Recursive) {
                tracing::warn!("inotify bridge: failed to watch {}: {e}", source.display());
                return;
            }
        }

        // Keep watcher alive for the duration of the task
        let _watcher = watcher;
        bridge_loop(rx, &path_map, &mut handle, &ip, &ssh_user, &ssh_key_path).await;
    })
}

/// Minimal russh client handler — accepts all server keys (like StrictHostKeyChecking=no).
struct SshHandler;

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Establish a persistent SSH connection to the guest and authenticate with the given key.
async fn ssh_connect(
    ip: &str,
    ssh_user: &str,
    ssh_key_path: &Path,
) -> Option<client::Handle<SshHandler>> {
    let config = Arc::new(client::Config::default());
    let addr = format!("{ip}:22");

    tracing::info!(addr, user = ssh_user, key = %ssh_key_path.display(), "inotify bridge: connecting SSH");

    let mut handle = match client::connect(config, &*addr, SshHandler).await {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("inotify bridge: SSH connect to {addr} failed: {e}");
            return None;
        }
    };

    tracing::info!("inotify bridge: TCP+SSH handshake OK, authenticating");

    let key_data = match std::fs::read_to_string(ssh_key_path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("inotify bridge: failed to read SSH key {}: {e}", ssh_key_path.display());
            return None;
        }
    };

    let key = match keys::decode_secret_key(&key_data, None) {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!("inotify bridge: failed to decode SSH key: {e}");
            return None;
        }
    };

    let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), None);

    match handle.authenticate_publickey(ssh_user, key_with_alg).await {
        Ok(auth) => {
            if !auth.success() {
                tracing::warn!("inotify bridge: SSH auth rejected for user {ssh_user}");
                return None;
            }
        }
        Err(e) => {
            tracing::warn!("inotify bridge: SSH auth failed: {e}");
            return None;
        }
    }

    tracing::info!("inotify bridge: SSH authenticated");
    Some(handle)
}

/// Check whether a filesystem event should be forwarded to the guest.
///
/// Only forward content-level changes: creation, data writes, and renames.
/// `touch` on the guest only produces `Modify(Metadata)` feedback through
/// virtiofs, so excluding metadata events breaks the feedback loop while
/// still catching all real edits.
fn should_forward(kind: &notify::EventKind) -> bool {
    use notify::event::*;
    matches!(
        kind,
        EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Name(_))
    )
}

/// Poll libvirt DHCP leases for the VM's first IPv4 address.
/// Opens its own connection so it doesn't interfere with the main one.
/// Returns None if the VM disappears or after ~120s timeout.
async fn wait_for_ip(libvirt_uri: &str, vm_name: &str) -> Option<String> {
    virt::error::clear_error_callback();
    let mut conn = Connect::open(Some(libvirt_uri)).ok()?;

    for _ in 0..60 {
        let dom = match Domain::lookup_by_name(&conn, vm_name) {
            Ok(d) => d,
            Err(_) => {
                let _ = conn.close();
                return None; // domain gone
            }
        };

        if !dom.is_active().unwrap_or(false) {
            let _ = conn.close();
            return None; // VM stopped
        }

        if let Ok(ifaces) =
            dom.interface_addresses(virt::sys::VIR_DOMAIN_INTERFACE_ADDRESSES_SRC_LEASE, 0)
        {
            for iface in &ifaces {
                for addr in &iface.addrs {
                    if addr.typed == 0 {
                        let _ = conn.close();
                        return Some(addr.addr.clone());
                    }
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    let _ = conn.close();
    None
}

/// Get the mtime of a host file as a Unix epoch string (seconds.nanoseconds).
/// Falls back to current time if stat fails.
fn host_mtime_epoch(path: &Path) -> String {
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| std::time::SystemTime::now());
    let dur = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:09}", dur.as_secs(), dur.subsec_nanos())
}

/// Translate a host path to a guest path using the prefix map.
fn translate_path(host_path: &Path, path_map: &[(PathBuf, String)]) -> Option<String> {
    for (source_prefix, target_prefix) in path_map {
        if let Ok(relative) = host_path.strip_prefix(source_prefix) {
            if relative.as_os_str().is_empty() {
                return Some(target_prefix.clone());
            }
            let guest = Path::new(target_prefix).join(relative);
            return Some(guest.to_string_lossy().into_owned());
        }
    }
    None
}

/// Execute a command on the guest over a persistent SSH connection.
/// Opens a new channel for each command; the connection persists across calls.
async fn ssh_exec(
    handle: &client::Handle<SshHandler>,
    command: &str,
) -> Result<(), russh::Error> {
    let mut channel = handle.channel_open_session().await?;
    channel.exec(true, command).await?;
    // Wait for the channel to close (command completes)
    while channel.wait().await.is_some() {}
    Ok(())
}

/// Main event loop: receive fs events, debounce, batch, send SSH touch commands.
async fn bridge_loop(
    mut rx: mpsc::UnboundedReceiver<Result<notify::Event, notify::Error>>,
    path_map: &[(PathBuf, String)],
    handle: &mut client::Handle<SshHandler>,
    ip: &str,
    ssh_user: &str,
    ssh_key_path: &Path,
) {
    loop {
        // Step 1: Await the first event (non-blocking for tokio)
        let first = match rx.recv().await {
            Some(event) => event,
            None => {
                tracing::debug!("watcher channel closed, stopping inotify bridge");
                return;
            }
        };

        // Step 2: Debounce — let more events accumulate
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Step 3: Drain all pending events
        let mut events = Vec::new();
        if let Ok(first) = first {
            events.push(first);
        }
        while let Ok(event) = rx.try_recv() {
            if let Ok(event) = event {
                events.push(event);
            }
        }

        for event in &events {
            tracing::debug!(
                kind = ?event.kind,
                paths = ?event.paths,
                forwarded = should_forward(&event.kind),
                "inotify bridge: event"
            );
        }

        // Step 4: Collect unique (guest_path, mtime) pairs, filtering by event kind.
        // Only Create, Modify(Data), Modify(Name) pass through — this
        // excludes Modify(Metadata) feedback from our own touch -d commands.
        let mut touches: Vec<(String, String)> = Vec::new(); // (guest_path, epoch)
        let mut seen = std::collections::HashSet::new();

        for event in &events {
            if !should_forward(&event.kind) {
                continue;
            }
            for path in &event.paths {
                if let Some(guest_path) = translate_path(path, path_map)
                    && seen.insert(guest_path.clone())
                {
                    let epoch = host_mtime_epoch(path);
                    touches.push((guest_path, epoch));
                }
            }
        }

        if touches.is_empty() {
            tracing::debug!("inotify bridge: all events filtered, nothing to forward");
            continue;
        }

        // Step 5: Build per-file touch -d commands to preserve original mtime
        let command: String = touches
            .iter()
            .map(|(gp, epoch)| format!("touch -d @{epoch} {}", shell_escape(gp)))
            .collect::<Vec<_>>()
            .join(";");

        tracing::info!(
            count = touches.len(),
            command,
            "inotify bridge: sending touch batch"
        );

        if let Err(e) = ssh_exec(handle, &command).await {
            tracing::info!("inotify bridge: ssh exec failed, reconnecting: {e}");
            // Connection may have dropped — try to reconnect
            if let Some(new_handle) = ssh_connect(ip, ssh_user, ssh_key_path).await {
                *handle = new_handle;
                // Retry the command on the fresh connection
                if let Err(e) = ssh_exec(handle, &command).await {
                    tracing::info!("inotify bridge: ssh retry failed: {e}");
                }
            } else {
                tracing::info!("inotify bridge: reconnect failed, will retry next batch");
            }
        }
    }
}

/// Minimal shell escaping: wrap in single quotes, escape embedded single quotes.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_path_basic() {
        let map = vec![
            (PathBuf::from("/home/user/project"), "/workspace".into()),
        ];
        let result = translate_path(Path::new("/home/user/project/src/main.rs"), &map);
        assert_eq!(result, Some("/workspace/src/main.rs".into()));
    }

    #[test]
    fn translate_path_root() {
        let map = vec![
            (PathBuf::from("/home/user/project"), "/workspace".into()),
        ];
        let result = translate_path(Path::new("/home/user/project"), &map);
        assert_eq!(result, Some("/workspace".into()));
    }

    #[test]
    fn translate_path_no_match() {
        let map = vec![
            (PathBuf::from("/home/user/project"), "/workspace".into()),
        ];
        let result = translate_path(Path::new("/other/path/file.rs"), &map);
        assert_eq!(result, None);
    }

    #[test]
    fn translate_path_longest_prefix_wins() {
        let map = vec![
            (PathBuf::from("/home/user/project/sub"), "/sub-mount".into()),
            (PathBuf::from("/home/user/project"), "/workspace".into()),
        ];
        let result = translate_path(Path::new("/home/user/project/sub/file.rs"), &map);
        assert_eq!(result, Some("/sub-mount/file.rs".into()));
    }

    #[test]
    fn shell_escape_basic() {
        assert_eq!(shell_escape("/workspace/src/main.rs"), "'/workspace/src/main.rs'");
    }

    #[test]
    fn shell_escape_with_quotes() {
        assert_eq!(shell_escape("it's a file"), "'it'\\''s a file'");
    }

    #[test]
    fn should_forward_content_changes_only() {
        use notify::event::*;

        // Content changes — forwarded
        assert!(should_forward(&EventKind::Create(CreateKind::File)));
        assert!(should_forward(&EventKind::Modify(ModifyKind::Data(DataChange::Any))));
        assert!(should_forward(&EventKind::Modify(ModifyKind::Name(RenameMode::Any))));

        // Metadata/access/remove — NOT forwarded (metadata = touch feedback)
        assert!(!should_forward(&EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any))));
        assert!(!should_forward(&EventKind::Access(AccessKind::Close(AccessMode::Write))));
        assert!(!should_forward(&EventKind::Access(AccessKind::Read)));
        assert!(!should_forward(&EventKind::Remove(RemoveKind::File)));
    }
}
