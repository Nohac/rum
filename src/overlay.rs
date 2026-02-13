use std::path::Path;

use crate::error::RumError;

/// Create a qcow2 overlay backed by the given base image.
pub async fn create_overlay(base_image: &Path, overlay_path: &Path) -> Result<(), RumError> {
    if let Some(parent) = overlay_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| RumError::Io {
                context: format!("creating directory {}", parent.display()),
                source: e,
            })?;
    }

    let output = tokio::process::Command::new("qemu-img")
        .args(["create", "-f", "qcow2", "-b"])
        .arg(base_image)
        .args(["-F", "qcow2"])
        .arg(overlay_path)
        .output()
        .await
        .map_err(|e| RumError::Io {
            context: "running qemu-img".into(),
            source: e,
        })?;

    if !output.status.success() {
        return Err(RumError::ExternalCommand {
            command: "qemu-img".into(),
            message: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    tracing::info!(path = %overlay_path.display(), "created qcow2 overlay");
    Ok(())
}
