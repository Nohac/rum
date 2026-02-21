use std::path::Path;

use crate::error::RumError;
use crate::qcow2;

/// Create a qcow2 overlay backed by the given base image.
pub async fn create_overlay(base_image: &Path, overlay_path: &Path) -> Result<(), RumError> {
    qcow2::create_qcow2_overlay(overlay_path, base_image)
}
