//! Minimal QCOW2 image generator for virtual disks.
//!
//! # Background
//!
//! QCOW2 (QEMU Copy-On-Write version 2) is the native disk image format for
//! QEMU/KVM virtual machines.  Unlike raw images, QCOW2 supports thin
//! provisioning (sparse allocation), snapshots, backing files, and compression.
//! A "20 GB" QCOW2 file with no data written only occupies ~256 KB on disk —
//! the space is allocated on demand as the guest writes to it.
//!
//! # Why we need this
//!
//! rum creates additional virtual disks for user-defined `[[drives]]` entries
//! and qcow2 overlay images backed by cloud base images.  Rather than shelling
//! out to `qemu-img create`, we generate valid QCOW2 images directly in Rust.
//! This keeps rum free of external tool dependencies for disk creation (the
//! same approach as our ISO 9660 generator).
//!
//! # Scope
//!
//! This module creates **empty** QCOW2 v2 images and **overlay** images with
//! a backing file.  No encryption, no compression, no snapshots.  It is not a
//! general-purpose QCOW2 library — it does exactly what rum needs.
//!
//! # Format overview
//!
//! A QCOW2 file is organized into **clusters** (default 64 KB each).  Data
//! lookup uses a two-level table:
//!
//! ```text
//!   Guest offset → L1 table → L2 table → data cluster on disk
//! ```
//!
//! For an empty image, all L1 entries are zero (no data allocated), so we only
//! need the metadata structures:
//!
//! ```text
//! ┌───────────┬──────────────────────────────────────────────────┐
//! │  Cluster  │ Contents                                         │
//! ├───────────┼──────────────────────────────────────────────────┤
//! │     0     │ Header (72 bytes) + padding                      │
//! │     1     │ L1 table (all zeros — no data allocated)         │
//! │     2     │ Refcount table (one entry → cluster 3)           │
//! │     3     │ Refcount block (marks clusters 0–3 as used)      │
//! └───────────┴──────────────────────────────────────────────────┘
//! ```
//!
//! Total file size: 4 clusters = 256 KB (with 64 KB clusters).
//!
//! For overlay images with a backing file, the layout is the same except
//! the header stores the backing file path right after byte 72, still
//! within cluster 0.  The backing file offset and length header fields
//! are set accordingly.
//!
//! # References
//!
//! - QEMU QCOW2 spec: <https://github.com/qemu/qemu/blob/master/docs/interop/qcow2.txt>
//! - Format overview: <https://people.gnome.org/~markmc/qcow-image-format.html>

use std::io::Write;
use std::path::Path;

use crate::error::RumError;
use crate::util::parse_size;

/// Cluster size: 64 KB (2^16 bytes).  This is the standard default used by
/// `qemu-img create` and provides a good balance between metadata overhead
/// and allocation granularity.
const CLUSTER_BITS: u32 = 16;
const CLUSTER_SIZE: usize = 1 << CLUSTER_BITS; // 65536

/// QCOW2 magic number: the ASCII bytes `QFI` followed by `0xFB`.
const QCOW2_MAGIC: u32 = 0x514649FB;

/// QCOW2 format version.  Version 2 is the most widely compatible and
/// sufficient for empty images (version 3 adds features like lazy refcounts
/// and extended L2 entries that we don't need).
const QCOW2_VERSION: u32 = 2;

/// Create an empty QCOW2 disk image at `path` with the given virtual `size`.
///
/// The size string supports common suffixes:
/// - `G` or `GB` — gibibytes (× 1024³)
/// - `M` or `MB` — mebibytes (× 1024²)
/// - `K` or `KB` — kibibytes (× 1024)
/// - No suffix — bytes
///
/// The resulting file is sparse: a "20G" image occupies only ~256 KB on disk.
///
/// # Examples
///
/// ```ignore
/// create_qcow2(Path::new("/tmp/disk.qcow2"), "20G")?;
/// create_qcow2(Path::new("/tmp/disk.qcow2"), "512M")?;
/// ```
pub fn create_qcow2(path: &Path, size: &str) -> Result<(), RumError> {
    let virtual_size = parse_size(size)?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RumError::Io {
            context: format!("creating directory {}", parent.display()),
            source: e,
        })?;
    }

    let image = build_qcow2(virtual_size);

    let mut file = std::fs::File::create(path).map_err(|e| RumError::Io {
        context: format!("creating qcow2 image {}", path.display()),
        source: e,
    })?;
    file.write_all(&image).map_err(|e| RumError::Io {
        context: format!("writing qcow2 image {}", path.display()),
        source: e,
    })?;

    tracing::info!(path = %path.display(), size, "created qcow2 image");
    Ok(())
}

/// Create a QCOW2 overlay image at `overlay_path` backed by `backing_file`.
///
/// The backing file must be an existing QCOW2 image.  Its virtual size is
/// read from the header and copied into the overlay.  The backing file path
/// is stored as its absolute canonical form.
pub fn create_qcow2_overlay(overlay_path: &Path, backing_file: &Path) -> Result<(), RumError> {
    if let Some(parent) = overlay_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RumError::Io {
            context: format!("creating directory {}", parent.display()),
            source: e,
        })?;
    }

    let canonical = std::fs::canonicalize(backing_file).map_err(|e| RumError::Io {
        context: format!("resolving backing file path {}", backing_file.display()),
        source: e,
    })?;
    let backing_path_str = canonical.to_string_lossy();

    // Read the backing file's virtual size from its QCOW2 header (bytes 24..32).
    let backing_header = {
        use std::io::Read;
        let mut f = std::fs::File::open(&canonical).map_err(|e| RumError::Io {
            context: format!("opening backing file {}", canonical.display()),
            source: e,
        })?;
        let mut buf = [0u8; 32];
        f.read_exact(&mut buf).map_err(|e| RumError::Io {
            context: format!("reading backing file header {}", canonical.display()),
            source: e,
        })?;
        buf
    };
    let virtual_size = u64::from_be_bytes(backing_header[24..32].try_into().unwrap());

    let image = build_qcow2_with_backing(virtual_size, &backing_path_str);

    let mut file = std::fs::File::create(overlay_path).map_err(|e| RumError::Io {
        context: format!("creating qcow2 overlay {}", overlay_path.display()),
        source: e,
    })?;
    file.write_all(&image).map_err(|e| RumError::Io {
        context: format!("writing qcow2 overlay {}", overlay_path.display()),
        source: e,
    })?;

    tracing::info!(path = %overlay_path.display(), backing = %canonical.display(), "created qcow2 overlay");
    Ok(())
}

/// Build a complete QCOW2 v2 image as a byte vector.
///
/// The image is structured as 4 clusters:
///
/// ```text
///   Cluster 0:  Header (72 bytes, rest zero-padded)
///   Cluster 1:  L1 table (all zeros — empty disk)
///   Cluster 2:  Refcount table (one 8-byte entry pointing to cluster 3)
///   Cluster 3:  Refcount block (4 entries marking clusters 0–3 as used)
/// ```
fn build_qcow2(virtual_size: u64) -> Vec<u8> {
    let mut image = vec![0u8; CLUSTER_SIZE * 4];

    // ── Cluster 0: Header ───────────────────────────────────────────
    //
    // The QCOW2 header is 72 bytes for version 2.  All multi-byte fields
    // are stored in big-endian byte order.
    //
    //   Offset  Size  Field
    //   ──────  ────  ─────
    //     0       4   Magic number (0x514649FB)
    //     4       4   Version (2)
    //     8       8   Backing file offset (0 = none)
    //    16       4   Backing file name length (0)
    //    20       4   Cluster bits (16 → 64 KB clusters)
    //    24       8   Virtual size in bytes
    //    32       4   Encryption method (0 = none)
    //    36       4   L1 table entry count
    //    40       8   L1 table offset (cluster 1 = 65536)
    //    48       8   Refcount table offset (cluster 2 = 131072)
    //    56       4   Refcount table clusters (1)
    //    60       4   Number of snapshots (0)
    //    64       8   Snapshots offset (0)

    let l1_entries = l1_table_entries(virtual_size);
    let l1_offset: u64 = CLUSTER_SIZE as u64; // cluster 1
    let refcount_table_offset: u64 = (CLUSTER_SIZE * 2) as u64; // cluster 2

    write_be32(&mut image, 0, QCOW2_MAGIC);
    write_be32(&mut image, 4, QCOW2_VERSION);
    // bytes 8..16: backing file offset = 0 (already zero)
    // bytes 16..20: backing file size = 0 (already zero)
    write_be32(&mut image, 20, CLUSTER_BITS);
    write_be64(&mut image, 24, virtual_size);
    // bytes 32..36: crypt method = 0 (already zero)
    write_be32(&mut image, 36, l1_entries);
    write_be64(&mut image, 40, l1_offset);
    write_be64(&mut image, 48, refcount_table_offset);
    write_be32(&mut image, 56, 1); // refcount table clusters
    // bytes 60..72: snapshots = 0 (already zero)

    // ── Cluster 1: L1 table ─────────────────────────────────────────
    //
    // The L1 table maps large chunks of the virtual disk to L2 tables.
    // Each L1 entry covers (cluster_size / 8) × cluster_size = 512 MB
    // with 64 KB clusters.  For an empty disk, all entries are zero
    // (meaning "not yet allocated"), so this cluster stays all-zeros.

    // ── Cluster 2: Refcount table ───────────────────────────────────
    //
    // The refcount table is an array of 8-byte offsets pointing to
    // refcount blocks.  Each refcount block tracks reference counts for
    // a range of clusters.  We only need one entry, pointing to the
    // refcount block in cluster 3.

    let refcount_block_offset: u64 = (CLUSTER_SIZE * 3) as u64; // cluster 3
    let rt_start = CLUSTER_SIZE * 2;
    write_be64(&mut image, rt_start, refcount_block_offset);

    // ── Cluster 3: Refcount block ───────────────────────────────────
    //
    // A refcount block is an array of 16-bit reference counts, one per
    // cluster (in QCOW2 v2, refcount_bits = 16).  We mark clusters 0–3
    // as having refcount = 1 (allocated).  Everything else stays zero.

    let rb_start = CLUSTER_SIZE * 3;
    for i in 0..4u16 {
        write_be16(&mut image, rb_start + (i as usize) * 2, 1);
    }

    image
}

/// Build a QCOW2 v2 overlay image with a backing file.
///
/// The structure is identical to [`build_qcow2`] — 4 clusters with the same
/// layout — except the header includes:
/// - Backing file offset (byte 8) pointing to offset 72 (right after header)
/// - Backing file name length (byte 16)
/// - The backing file path string written at byte 72 within cluster 0
fn build_qcow2_with_backing(virtual_size: u64, backing_path: &str) -> Vec<u8> {
    let mut image = build_qcow2(virtual_size);

    let backing_bytes = backing_path.as_bytes();
    let backing_offset: u64 = 72; // right after the 72-byte header

    // Set backing file offset (bytes 8..16)
    write_be64(&mut image, 8, backing_offset);

    // Set backing file name length (bytes 16..20)
    write_be32(&mut image, 16, backing_bytes.len() as u32);

    // Write the backing file path string at offset 72 (still within cluster 0)
    image[72..72 + backing_bytes.len()].copy_from_slice(backing_bytes);

    image
}

/// Calculate the number of L1 table entries needed for a given virtual size.
///
/// Each L1 entry covers one L2 table's worth of data.  With 64 KB clusters,
/// an L2 table has 8192 entries (64 KB / 8 bytes), each pointing to a 64 KB
/// data cluster, so one L1 entry covers 8192 × 64 KB = 512 MB.
fn l1_table_entries(virtual_size: u64) -> u32 {
    let l2_entries = CLUSTER_SIZE as u64 / 8; // entries per L2 table
    let bytes_per_l1 = l2_entries * CLUSTER_SIZE as u64; // bytes covered per L1 entry
    virtual_size.div_ceil(bytes_per_l1) as u32
}

// ── Big-endian write helpers ────────────────────────────────────────
//
// QCOW2 uses big-endian for all multi-byte fields, regardless of the
// host architecture.

fn write_be16(buf: &mut [u8], offset: usize, val: u16) {
    buf[offset..offset + 2].copy_from_slice(&val.to_be_bytes());
}

fn write_be32(buf: &mut [u8], offset: usize, val: u32) {
    buf[offset..offset + 4].copy_from_slice(&val.to_be_bytes());
}

fn write_be64(buf: &mut [u8], offset: usize, val: u64) {
    buf[offset..offset + 8].copy_from_slice(&val.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qcow2_has_magic() {
        let image = build_qcow2(1024 * 1024 * 1024); // 1 GB
        assert_eq!(&image[0..4], &[0x51, 0x46, 0x49, 0xFB]);
    }

    #[test]
    fn qcow2_has_correct_virtual_size() {
        let size: u64 = 20 * 1024 * 1024 * 1024; // 20 GB
        let image = build_qcow2(size);
        let stored = u64::from_be_bytes(image[24..32].try_into().unwrap());
        assert_eq!(stored, size);
    }

    #[test]
    fn qcow2_is_four_clusters() {
        let image = build_qcow2(1024 * 1024 * 1024);
        assert_eq!(image.len(), CLUSTER_SIZE * 4);
    }

    #[test]
    fn qcow2_l1_entries_small_disk() {
        // 1 GB needs ceil(1 GB / 512 MB) = 2 L1 entries
        assert_eq!(l1_table_entries(1024 * 1024 * 1024), 2);
    }

    #[test]
    fn qcow2_l1_entries_large_disk() {
        // 100 GB needs ceil(100 GB / 512 MB) = 200 L1 entries
        assert_eq!(l1_table_entries(100 * 1024 * 1024 * 1024), 200);
    }

    #[test]
    fn create_qcow2_writes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.qcow2");
        create_qcow2(&path, "1G").unwrap();
        assert!(path.exists());

        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[0..4], &[0x51, 0x46, 0x49, 0xFB]);
        assert_eq!(data.len(), CLUSTER_SIZE * 4);
    }

    #[test]
    fn overlay_has_magic() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.qcow2");
        create_qcow2(&base, "1G").unwrap();

        let overlay = dir.path().join("overlay.qcow2");
        create_qcow2_overlay(&overlay, &base).unwrap();

        let data = std::fs::read(&overlay).unwrap();
        assert_eq!(&data[0..4], &[0x51, 0x46, 0x49, 0xFB]);
    }

    #[test]
    fn overlay_backing_file_offset() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.qcow2");
        create_qcow2(&base, "1G").unwrap();

        let overlay = dir.path().join("overlay.qcow2");
        create_qcow2_overlay(&overlay, &base).unwrap();

        let data = std::fs::read(&overlay).unwrap();
        let offset = u64::from_be_bytes(data[8..16].try_into().unwrap());
        assert_eq!(offset, 72);
    }

    #[test]
    fn overlay_backing_file_name_length() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.qcow2");
        create_qcow2(&base, "1G").unwrap();

        let canonical = std::fs::canonicalize(&base).unwrap();
        let expected_len = canonical.to_string_lossy().len() as u32;

        let overlay = dir.path().join("overlay.qcow2");
        create_qcow2_overlay(&overlay, &base).unwrap();

        let data = std::fs::read(&overlay).unwrap();
        let name_len = u32::from_be_bytes(data[16..20].try_into().unwrap());
        assert_eq!(name_len, expected_len);
    }

    #[test]
    fn overlay_contains_backing_path() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.qcow2");
        create_qcow2(&base, "1G").unwrap();

        let canonical = std::fs::canonicalize(&base).unwrap();
        let path_str = canonical.to_string_lossy();

        let overlay = dir.path().join("overlay.qcow2");
        create_qcow2_overlay(&overlay, &base).unwrap();

        let data = std::fs::read(&overlay).unwrap();
        let stored = std::str::from_utf8(&data[72..72 + path_str.len()]).unwrap();
        assert_eq!(stored, path_str.as_ref());
    }

    #[test]
    fn overlay_inherits_virtual_size() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base.qcow2");
        create_qcow2(&base, "20G").unwrap();

        let overlay = dir.path().join("overlay.qcow2");
        create_qcow2_overlay(&overlay, &base).unwrap();

        let data = std::fs::read(&overlay).unwrap();
        let overlay_size = u64::from_be_bytes(data[24..32].try_into().unwrap());
        assert_eq!(overlay_size, 20 * 1024 * 1024 * 1024);
    }
}
