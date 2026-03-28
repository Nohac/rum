use std::path::Path;

use crate::error::RumError;
use crate::qcow2;

/// Create a qcow2 overlay backed by the given base image.
///
/// If `virtual_size` is provided, the overlay's virtual disk size is set to
/// `max(backing_size, virtual_size)`, allowing the root partition to grow.
pub async fn create_overlay(
    base_image: &Path,
    overlay_path: &Path,
    virtual_size: Option<u64>,
) -> Result<(), RumError> {
    qcow2::create_qcow2_overlay(overlay_path, base_image, virtual_size)
}
