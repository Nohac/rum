//! Minimal ISO 9660 image generator with Rock Ridge extensions.
//!
//! # Background
//!
//! ISO 9660 is the standard filesystem for CD-ROMs, dating back to 1988.  It was
//! designed for maximum portability across operating systems, which means the base
//! format has severe filename restrictions (8.3 uppercase only).  Rock Ridge is an
//! extension that bolts POSIX semantics on top — lowercase names, long filenames,
//! permissions, symlinks — by hiding extra metadata in "System Use" areas that
//! ISO-unaware readers simply ignore.
//!
//! # Why we need this
//!
//! Cloud-init's NoCloud datasource expects a small ISO (the "seed") with volume
//! label `CIDATA` containing files like `meta-data`, `user-data`, and
//! `network-config`.  These lowercase, hyphenated names are impossible to represent
//! in base ISO 9660 Level 1, so we need Rock Ridge NM (alternate name) entries to
//! preserve them.
//!
//! # Scope
//!
//! This module only supports flat ISOs (files in the root directory, no
//! subdirectories).  It is not a general-purpose ISO authoring library — it does
//! exactly what cloud-init seed images need and nothing more.
//!
//! # References
//!
//! - ECMA-119 (ISO 9660): <https://ecma-international.org/publications-and-standards/standards/ecma-119/>
//! - SUSP (IEEE P1281):    System Use Sharing Protocol
//! - RRIP (IEEE P1282):    Rock Ridge Interchange Protocol

/// Each sector (also called a "logical block") in an ISO 9660 image is 2048 bytes.
/// This is the native sector size of CD-ROMs and is hardcoded in the spec.
const SECTOR_SIZE: usize = 2048;

/// A file to include in the ISO image.
pub struct IsoFile<'a> {
    /// The filename as it should appear on Linux (e.g. `"meta-data"`).
    /// Will be stored as a Rock Ridge alternate name.
    pub name: &'a str,
    /// The file contents (arbitrary bytes).
    pub data: &'a [u8],
}

/// Build an ISO 9660 image with Rock Ridge extensions.
///
/// Returns the complete ISO image as a byte vector, ready to be written to disk.
///
/// `volume_id` must be ASCII uppercase, max 32 chars (e.g. `"CIDATA"`).
/// All files are placed in the root directory.
///
/// # Panics
///
/// Panics if `volume_id` is not ASCII or exceeds 32 characters.
pub fn build_iso(volume_id: &str, files: &[IsoFile<'_>]) -> Vec<u8> {
    assert!(
        volume_id.len() <= 32 && volume_id.is_ascii(),
        "volume_id must be ASCII, max 32 chars"
    );

    // ┌─────────────────────────────────────────────────────────────────┐
    // │                      ISO IMAGE LAYOUT                           │
    // ├──────────┬──────────────────────────────────────────────────────┤
    // │ Sectors  │ Contents                                             │
    // ├──────────┼──────────────────────────────────────────────────────┤
    // │  0 – 15  │ System Area (all zeros, reserved for boot loaders)   │
    // │    16    │ Primary Volume Descriptor (PVD)                      │
    // │    17    │ Volume Descriptor Set Terminator                     │
    // │    18    │ Path Table (L-type, little-endian)                   │
    // │    19    │ Path Table (M-type, big-endian)                      │
    // │    20    │ Root Directory (., .., and file entries)             │
    // │    21    │ SUSP Continuation Area (Rock Ridge ER entry)         │
    // │  22+     │ File data (each file starts on a sector boundary)    │
    // └──────────┴──────────────────────────────────────────────────────┘

    let root_dir_sector = 20u32;
    let ce_sector = 21u32;
    let first_file_sector = 22usize;

    // Pre-calculate where each file's data will land.  Every file starts on a
    // fresh sector boundary (required by ISO 9660).
    let mut file_layout: Vec<(usize, usize)> = Vec::with_capacity(files.len());
    let mut next_sector = first_file_sector;
    for f in files {
        file_layout.push((next_sector, f.data.len()));
        next_sector += sectors_for(f.data.len());
    }
    let total_sectors = next_sector;

    // Allocate the entire image as zeroed bytes upfront.
    let mut iso = vec![0u8; total_sectors * SECTOR_SIZE];

    // Write each structural component.
    write_pvd(&mut iso, volume_id, total_sectors as u32, root_dir_sector);
    write_vdst(&mut iso);
    write_path_table(&mut iso, 18, root_dir_sector, Endian::Little);
    write_path_table(&mut iso, 19, root_dir_sector, Endian::Big);

    let er_entry = susp_er();
    write_root_directory(
        &mut iso,
        root_dir_sector,
        ce_sector,
        &er_entry,
        files,
        &file_layout,
    );

    // Write the SUSP Continuation Area (sector 21).  This contains the ER
    // (Extension Reference) entry that identifies Rock Ridge.  It lives in its
    // own sector because the ER entry is ~240 bytes — too large to fit in the
    // "." directory record's system use area alongside the SP entry.
    let ce_start = ce_sector as usize * SECTOR_SIZE;
    iso[ce_start..ce_start + er_entry.len()].copy_from_slice(&er_entry);

    // Write file contents into their pre-calculated sectors.
    for (i, f) in files.iter().enumerate() {
        let offset = file_layout[i].0 * SECTOR_SIZE;
        iso[offset..offset + f.data.len()].copy_from_slice(f.data);
    }

    iso
}

/// Write the Primary Volume Descriptor (PVD) at sector 16.
///
/// The PVD is the main metadata block of the ISO.  It always lives at sector 16
/// (the first sector after the system area) and is exactly one sector (2048 bytes).
///
/// Key fields and their byte offsets within the sector:
///
/// | Offset | Size | Field                           |
/// |--------|------|---------------------------------|
/// |   0    |   1  | Type (1 = PVD)                  |
/// |   1    |   5  | Standard Identifier (`"CD001"`) |
/// |   6    |   1  | Version (1)                     |
/// |   8    |  32  | System Identifier (space-padded) |
/// |  40    |  32  | Volume Identifier (space-padded) — the "label" |
/// |  80    |   8  | Volume Space Size (both-endian) — total sectors |
/// | 120    |   4  | Volume Set Size (both-endian)   |
/// | 124    |   4  | Volume Sequence Number (both-endian) |
/// | 128    |   4  | Logical Block Size (both-endian) — always 2048 |
/// | 132    |   8  | Path Table Size (both-endian)   |
/// | 140    |   4  | L Path Table Location (LE u32)  |
/// | 148    |   4  | M Path Table Location (BE u32)  |
/// | 156    |  34  | Root Directory Record (inline!)  |
/// | 190    | 624  | Identifier strings (space-padded) |
/// | 881    |   1  | File Structure Version (1)      |
fn write_pvd(iso: &mut [u8], volume_id: &str, total_sectors: u32, root_dir_sector: u32) {
    let pvd = &mut iso[16 * SECTOR_SIZE..17 * SECTOR_SIZE];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[6] = 1;

    // System identifier and volume identifier are space-padded fixed fields.
    pvd[8..40].fill(b' ');
    pvd[40..72].fill(b' ');
    let vid = volume_id.as_bytes();
    pvd[40..40 + vid.len()].copy_from_slice(vid);

    put_u32_both(&mut pvd[80..88], total_sectors);
    put_u16_both(&mut pvd[120..124], 1);
    put_u16_both(&mut pvd[124..128], 1);
    put_u16_both(&mut pvd[128..132], SECTOR_SIZE as u16);
    put_u32_both(&mut pvd[132..140], 10); // path table = 10 bytes (one root entry)
    pvd[140..144].copy_from_slice(&18u32.to_le_bytes()); // L path table at sector 18
    pvd[148..152].copy_from_slice(&19u32.to_be_bytes()); // M path table at sector 19

    // The root directory record is embedded directly in the PVD at byte 156.
    // Its name is a single 0x00 byte (meaning "self" / ".").
    write_fixed_dir_record(
        &mut pvd[156..190],
        root_dir_sector,
        SECTOR_SIZE as u32,
        b"\x00",
        true,
    );

    // Remaining identifier fields (publisher, preparer, etc.) — space-padded.
    pvd[190..814].fill(b' ');
    pvd[881] = 1; // file structure version
}

/// ═══════════════════════════════════════════════════════════════════════════
/// Volume Descriptor Set Terminator
/// ═══════════════════════════════════════════════════════════════════════════
///
/// Marks the end of the volume descriptor sequence.  Readers scan sectors 16, 17,
/// 18... until they find type 255.  We only have one descriptor (the PVD), so the
/// terminator goes right at sector 17.
fn write_vdst(iso: &mut [u8]) {
    let vdst = &mut iso[17 * SECTOR_SIZE..18 * SECTOR_SIZE];
    vdst[0] = 255; // type = terminator
    vdst[1..6].copy_from_slice(b"CD001");
    vdst[6] = 1;
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

/// Write a path table entry at the given sector.
///
/// Path tables provide a flat index of all directories for fast lookup.  The
/// spec requires two copies: one in little-endian (L-type) and one in big-endian
/// (M-type), because ISO 9660 was designed to work on both architectures without
/// byte-swapping.
///
/// Since we only have the root directory, each path table is just one 10-byte
/// entry:
///
/// | Offset | Size | Field                                              |
/// |--------|------|----------------------------------------------------|
/// |   0    |   1  | Directory Identifier Length (1 for root)           |
/// |   1    |   1  | Extended Attribute Record Length (0)                |
/// |   2    |   4  | Extent Location (sector of the directory)          |
/// |   6    |   2  | Parent Directory Number (1 = self for root)        |
/// |   8    |   1  | Directory Identifier (`0x00` for root)             |
/// |   9    |   1  | Padding (to even length)                           |
fn write_path_table(iso: &mut [u8], sector: usize, root_extent: u32, endian: Endian) {
    let buf = &mut iso[sector * SECTOR_SIZE..];
    buf[0] = 1; // identifier length
    buf[1] = 0; // no extended attributes
    match endian {
        Endian::Little => {
            buf[2..6].copy_from_slice(&root_extent.to_le_bytes());
            buf[6..8].copy_from_slice(&1u16.to_le_bytes());
        }
        Endian::Big => {
            buf[2..6].copy_from_slice(&root_extent.to_be_bytes());
            buf[6..8].copy_from_slice(&1u16.to_be_bytes());
        }
    }
    buf[8] = 0x00; // root identifier
    buf[9] = 0x00; // padding
}

/// Write the root directory extent at the given sector.
///
/// The root directory is a sequence of variable-length Directory Records packed
/// into one or more sectors.  It always starts with `.` (self) and `..` (parent),
/// followed by entries for each file/subdirectory.
///
/// For Rock Ridge, each directory record can carry a "System Use" area after the
/// filename — this is where we put NM (alternate name) and PX (POSIX attributes)
/// entries.  The `.` record also carries an SP (SUSP indicator) entry, plus a CE
/// (continuation) pointer to the ER (extension reference) in a separate sector.
///
/// ```text
/// ┌─────────────────────────────── Sector 20 ───────────────────────────────┐
/// │ "."  record  [SP][CE→sector 21]                                        │
/// │ ".." record                                                            │
/// │ file record  [NM="meta-data"][PX=0644]                                 │
/// │ file record  [NM="user-data"][PX=0644]                                 │
/// │ file record  [NM="network-config"][PX=0644]                            │
/// │ (zero padding to end of sector)                                        │
/// └────────────────────────────────────────────────────────────────────────-┘
///
/// ┌─────────────────────────────── Sector 21 ───────────────────────────────┐
/// │ ER entry (RRIP_1991A identification, ~240 bytes)                       │
/// │ (zero padding to end of sector)                                        │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
fn write_root_directory(
    iso: &mut [u8],
    root_sector: u32,
    ce_sector: u32,
    er_entry: &[u8],
    files: &[IsoFile<'_>],
    file_layout: &[(usize, usize)],
) {
    let dir_start = root_sector as usize * SECTOR_SIZE;
    let mut pos = dir_start;
    let root_size = SECTOR_SIZE as u32;

    // "." entry — includes SP (SUSP presence marker) and CE (pointer to the ER
    // entry in the continuation area at sector 21).
    let sp = susp_sp();
    let ce = susp_ce(ce_sector, 0, er_entry.len() as u32);
    let mut dot_su = Vec::with_capacity(sp.len() + ce.len());
    dot_su.extend_from_slice(&sp);
    dot_su.extend_from_slice(&ce);
    let dot = dir_record(root_sector, root_size, b"\x00", true, &dot_su);
    iso[pos..pos + dot.len()].copy_from_slice(&dot);
    pos += dot.len();

    // ".." entry — for the root directory, parent is itself.
    let dotdot = dir_record(root_sector, root_size, b"\x01", true, &[]);
    iso[pos..pos + dotdot.len()].copy_from_slice(&dotdot);
    pos += dotdot.len();

    // File entries.  Each gets:
    //   - An ISO 9660 Level 1 name (8.3 uppercase, e.g. "META_DAT;1")
    //   - A Rock Ridge NM entry with the real filename (e.g. "meta-data")
    //   - A Rock Ridge PX entry with POSIX permissions (0644, regular file)
    for (i, f) in files.iter().enumerate() {
        let (sector, size) = file_layout[i];
        let iso_name = to_level1_name(f.name);
        let nm = rrip_nm(f.name);
        let px = rrip_px(0o100644, 1);
        let mut su = Vec::with_capacity(nm.len() + px.len());
        su.extend_from_slice(&nm);
        su.extend_from_slice(&px);

        let rec = dir_record(sector as u32, size as u32, iso_name.as_bytes(), false, &su);
        iso[pos..pos + rec.len()].copy_from_slice(&rec);
        pos += rec.len();
    }
}

/// Write a fixed-size (34-byte) directory record into a buffer.
///
/// Used for the root directory record embedded in the PVD, which has no system
/// use area.  See [`dir_record`] for the variable-length version with Rock Ridge
/// support.
///
/// Each directory record describes one file or subdirectory:
///
/// | Offset    | Size | Field                                            |
/// |-----------|------|--------------------------------------------------|
/// |  0        |   1  | Record Length (total bytes, including this field) |
/// |  1        |   1  | Extended Attribute Record Length (0)              |
/// |  2        |   8  | Extent Location (both-endian u32) — starting sector |
/// | 10        |   8  | Data Length (both-endian u32) — file size in bytes |
/// | 18        |   7  | Recording Date/Time                              |
/// | 25        |   1  | File Flags (bit 1 = directory)                   |
/// | 26        |   1  | File Unit Size (0)                               |
/// | 27        |   1  | Interleave Gap Size (0)                          |
/// | 28        |   4  | Volume Sequence Number (both-endian u16)         |
/// | 32        |   1  | File Identifier Length                           |
/// | 33        |   N  | File Identifier (the ISO 9660 name)              |
/// | 33+N      |  pad | Padding byte if N is even (to align to even offset) |
/// | 33+N+pad  |   *  | System Use area (Rock Ridge entries go here)     |
///
/// The "both-endian" encoding stores each number twice: first as little-endian,
/// then as big-endian, in adjacent bytes.  This avoids byte-swapping on any
/// architecture.
fn write_fixed_dir_record(buf: &mut [u8], extent: u32, size: u32, name: &[u8], is_dir: bool) {
    let name_len = name.len();
    let record_len = 33 + name_len + (if name_len.is_multiple_of(2) { 1 } else { 0 });
    buf[0] = record_len as u8;
    put_u32_both(&mut buf[2..10], extent);
    put_u32_both(&mut buf[10..18], size);
    buf[25] = if is_dir { 0x02 } else { 0x00 };
    put_u16_both(&mut buf[28..32], 1); // volume sequence number
    buf[32] = name_len as u8;
    buf[33..33 + name_len].copy_from_slice(name);
}

/// Build a variable-length directory record with an optional System Use area.
/// Returns the record as a new Vec.
fn dir_record(extent: u32, size: u32, name: &[u8], is_dir: bool, su: &[u8]) -> Vec<u8> {
    let name_len = name.len();
    // ISO 9660 requires the system use area to start at an even offset, so we
    // add a padding byte when the name length is even (because 33 + even = odd).
    let padding = if name_len.is_multiple_of(2) { 1 } else { 0 };
    let record_len = 33 + name_len + padding + su.len();
    let mut buf = vec![0u8; record_len];
    buf[0] = record_len as u8;
    put_u32_both(&mut buf[2..10], extent);
    put_u32_both(&mut buf[10..18], size);
    buf[25] = if is_dir { 0x02 } else { 0x00 };
    put_u16_both(&mut buf[28..32], 1);
    buf[32] = name_len as u8;
    buf[33..33 + name_len].copy_from_slice(name);
    let su_start = 33 + name_len + padding;
    buf[su_start..su_start + su.len()].copy_from_slice(su);
    buf
}

/// SUSP SP — presence marker.  Goes in the `.` record's system use area.
///
/// SUSP (System Use Sharing Protocol) defines a framework for embedding
/// extension data in directory records' System Use areas.  Rock Ridge (RRIP) is
/// the most common extension, adding POSIX filesystem semantics.
///
/// All SUSP entries share a common 4-byte header:
///
/// | Offset | Size | Field                                         |
/// |--------|------|-----------------------------------------------|
/// |   0    |   2  | Signature (two ASCII chars, e.g. `"SP"`)      |
/// |   2    |   1  | Entry Length (total bytes including header)    |
/// |   3    |   1  | Version (always 1)                            |
/// |   4+   |   *  | Entry-specific data                           |
///
/// The entries we use:
///
/// - **SP** — SUSP presence marker.  Must appear in the `.` record of every
///   directory that uses SUSP.  Contains magic bytes `0xBE 0xEF`.
/// - **CE** — Continuation Area pointer.  Points to additional SUSP data that
///   didn't fit in the directory record.  Contains (sector, offset, length).
/// - **ER** — Extension Reference.  Identifies which extension is in use
///   (`RRIP_1991A`).  Too large (~240 bytes) to fit in a directory record, so
///   we put it in a continuation area and point to it with CE.
/// - **NM** — Alternate Name (Rock Ridge).  Stores the real POSIX filename,
///   e.g. `"meta-data"`, alongside the mangled ISO 9660 name `"META_DAT;1"`.
/// - **PX** — POSIX Attributes (Rock Ridge).  Stores file mode, link count,
///   uid, gid.
///
///   Bytes: "SP" | len=7 | ver=1 | check_byte_1=0xBE | check_byte_2=0xEF | skip=0
fn susp_sp() -> Vec<u8> {
    vec![b'S', b'P', 7, 1, 0xBE, 0xEF, 0]
}

/// SUSP CE — continuation area pointer.
///
/// Points to additional SUSP data stored outside this directory record.
///
///   Bytes: "CE" | len=28 | ver=1 | block(8) | offset(8) | length(8)
///
/// Each of the three fields (block, offset, length) is stored in both-endian
/// format (4 bytes LE + 4 bytes BE = 8 bytes each).
fn susp_ce(block: u32, offset: u32, length: u32) -> Vec<u8> {
    let mut buf = vec![0u8; 28];
    buf[0] = b'C';
    buf[1] = b'E';
    buf[2] = 28;
    buf[3] = 1;
    put_u32_both(&mut buf[4..12], block);
    put_u32_both(&mut buf[12..20], offset);
    put_u32_both(&mut buf[20..28], length);
    buf
}

/// SUSP ER — extension reference identifying Rock Ridge (RRIP_1991A).
///
/// This entry declares "I'm using Rock Ridge" so that readers know to look for
/// NM, PX, SL, etc. entries.  The strings are specified by the RRIP standard.
///
///   Bytes: "ER" | len | ver=1 | id_len | desc_len | src_len | ext_ver=1
///          | id_bytes | desc_bytes | src_bytes
fn susp_er() -> Vec<u8> {
    let id = b"RRIP_1991A";
    let desc =
        b"THE ROCK RIDGE INTERCHANGE PROTOCOL PROVIDES SUPPORT FOR POSIX FILE SYSTEM SEMANTICS";
    let src = b"PLEASE CONTACT DISC PUBLISHER FOR SPECIFICATION SOURCE.  SEE PUBLISHER IDENTIFIER IN PRIMARY VOLUME DESCRIPTOR FOR CONTACT INFORMATION.";
    let total = 8 + id.len() + desc.len() + src.len();
    let mut buf = vec![0u8; total];
    buf[0] = b'E';
    buf[1] = b'R';
    buf[2] = total as u8;
    buf[3] = 1;
    buf[4] = id.len() as u8;
    buf[5] = desc.len() as u8;
    buf[6] = src.len() as u8;
    buf[7] = 1; // extension version
    let mut p = 8;
    buf[p..p + id.len()].copy_from_slice(id);
    p += id.len();
    buf[p..p + desc.len()].copy_from_slice(desc);
    p += desc.len();
    buf[p..p + src.len()].copy_from_slice(src);
    buf
}

/// RRIP NM — alternate (POSIX) filename.
///
/// This is the key entry that lets Linux see "meta-data" instead of "META_DAT;1".
///
///   Bytes: "NM" | len | ver=1 | flags=0 | name_bytes...
///
/// Flags=0 means this entry contains the complete name (no continuation needed).
fn rrip_nm(name: &str) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let total = 5 + name_bytes.len();
    let mut buf = vec![0u8; total];
    buf[0] = b'N';
    buf[1] = b'M';
    buf[2] = total as u8;
    buf[3] = 1;
    // buf[4] = 0 (flags: name is complete)
    buf[5..].copy_from_slice(name_bytes);
    buf
}

/// RRIP PX — POSIX file attributes.
///
///   Bytes: "PX" | len=44 | ver=1 | mode(8) | nlinks(8) | uid(8) | gid(8) | serial(8)
///
/// All numeric fields are both-endian (4 LE + 4 BE = 8 bytes each).
/// We use mode 0100644 (regular file, rw-r--r--) for all files.
fn rrip_px(mode: u32, nlinks: u32) -> Vec<u8> {
    let mut buf = vec![0u8; 44];
    buf[0] = b'P';
    buf[1] = b'X';
    buf[2] = 44;
    buf[3] = 1;
    put_u32_both(&mut buf[4..12], mode); // st_mode
    put_u32_both(&mut buf[12..20], nlinks); // st_nlink
    // uid (20..28), gid (28..36), serial (36..44) all stay zero.
    buf
}

/// Convert a filename to ISO 9660 Level 1 format.
///
/// Level 1 is the most restrictive (and most compatible) filename format:
///   - Max 8 characters for the base name, 3 for the extension
///   - Uppercase A-Z, digits 0-9, and underscore only
///   - A ";1" version suffix is appended
///
/// Examples:
///   "meta-data"       → "META_DAT;1"    (no dot → 8-char base, truncated)
///   "file.txt"        → "FILE.TXT;1"    (fits in 8.3)
///   "network-config"  → "NETWORK_;1"    (no dot, truncated to 8 chars)
///
/// These mangled names are only used by ISO-unaware readers.  Linux uses the
/// Rock Ridge NM entry instead.
fn to_level1_name(name: &str) -> String {
    let sanitized: String = name
        .to_ascii_uppercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if let Some(dot) = sanitized.find('.') {
        let base = &sanitized[..dot.min(8)];
        let ext_end = (dot + 1 + 3).min(sanitized.len());
        let ext = &sanitized[dot + 1..ext_end];
        format!("{base}.{ext};1")
    } else {
        let base = &sanitized[..sanitized.len().min(8)];
        format!("{base};1")
    }
}

/// How many sectors are needed to hold `bytes` of data.
/// Empty files still occupy one sector.
fn sectors_for(bytes: usize) -> usize {
    if bytes == 0 {
        1
    } else {
        bytes.div_ceil(SECTOR_SIZE)
    }
}

/// Write a u32 in "both-endian" format: 4 bytes LE followed by 4 bytes BE.
///
/// ISO 9660 uses this encoding for all multi-byte numbers so that readers on
/// both little-endian and big-endian architectures can read them without
/// byte-swapping.
///
///   buf[0..4] = val as little-endian
///   buf[4..8] = val as big-endian
fn put_u32_both(buf: &mut [u8], val: u32) {
    buf[0..4].copy_from_slice(&val.to_le_bytes());
    buf[4..8].copy_from_slice(&val.to_be_bytes());
}

/// Write a u16 in "both-endian" format: 2 bytes LE followed by 2 bytes BE.
fn put_u16_both(buf: &mut [u8], val: u16) {
    buf[0..2].copy_from_slice(&val.to_le_bytes());
    buf[2..4].copy_from_slice(&val.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_iso() -> Vec<u8> {
        build_iso(
            "CIDATA",
            &[
                IsoFile {
                    name: "meta-data",
                    data: b"instance-id: test\n",
                },
                IsoFile {
                    name: "user-data",
                    data: b"#cloud-config\n",
                },
                IsoFile {
                    name: "network-config",
                    data: b"version: 2\n",
                },
            ],
        )
    }

    #[test]
    fn iso_has_cd001_magic() {
        let iso = sample_iso();
        assert_eq!(&iso[0x8001..0x8006], b"CD001");
    }

    #[test]
    fn iso_has_volume_id() {
        let iso = sample_iso();
        let vid = &iso[16 * SECTOR_SIZE + 40..16 * SECTOR_SIZE + 46];
        assert_eq!(vid, b"CIDATA");
    }

    #[test]
    fn iso_has_terminator() {
        let iso = sample_iso();
        assert_eq!(iso[17 * SECTOR_SIZE], 255);
        assert_eq!(&iso[17 * SECTOR_SIZE + 1..17 * SECTOR_SIZE + 6], b"CD001");
    }

    #[test]
    fn iso_size_is_sector_aligned() {
        let iso = sample_iso();
        assert_eq!(iso.len() % SECTOR_SIZE, 0);
    }

    #[test]
    fn iso_contains_file_data() {
        let iso = sample_iso();
        let has_meta = iso.windows(18).any(|w| w == b"instance-id: test\n");
        let has_user = iso.windows(14).any(|w| w == b"#cloud-config\n");
        let has_net = iso.windows(11).any(|w| w == b"version: 2\n");
        assert!(has_meta, "missing meta-data content");
        assert!(has_user, "missing user-data content");
        assert!(has_net, "missing network-config content");
    }

    #[test]
    fn iso_contains_rock_ridge_nm_entries() {
        let iso = sample_iso();
        let has_meta = iso.windows(9).any(|w| w == b"meta-data");
        let has_user = iso.windows(9).any(|w| w == b"user-data");
        let has_net = iso.windows(14).any(|w| w == b"network-config");
        assert!(has_meta, "missing Rock Ridge NM for meta-data");
        assert!(has_user, "missing Rock Ridge NM for user-data");
        assert!(has_net, "missing Rock Ridge NM for network-config");
    }

    #[test]
    fn iso_contains_susp_sp_marker() {
        let iso = sample_iso();
        let sp = [b'S', b'P', 7, 1, 0xBE, 0xEF];
        let has_sp = iso.windows(sp.len()).any(|w| w == sp);
        assert!(has_sp, "missing SUSP SP marker");
    }

    #[test]
    fn iso_contains_rrip_er_entry() {
        let iso = sample_iso();
        let has_er = iso.windows(10).any(|w| w == b"RRIP_1991A");
        assert!(has_er, "missing RRIP extension reference");
    }

    #[test]
    fn iso_root_directory_has_dot_entries() {
        let iso = sample_iso();
        let root_start = 20 * SECTOR_SIZE;
        let first_name_len = iso[root_start + 32] as usize;
        assert_eq!(first_name_len, 1);
        assert_eq!(iso[root_start + 33], 0x00); // "." = 0x00
        assert_eq!(iso[root_start + 25] & 0x02, 0x02); // directory flag set
    }

    #[test]
    fn iso_level1_name_conversion() {
        // No dot → 8-char max base, no extension.
        assert_eq!(to_level1_name("meta-data"), "META_DAT;1");
        assert_eq!(to_level1_name("user-data"), "USER_DAT;1");
        assert_eq!(to_level1_name("network-config"), "NETWORK_;1");
        assert_eq!(to_level1_name("README"), "README;1");
        // With dot → 8.3 split.
        assert_eq!(to_level1_name("file.txt"), "FILE.TXT;1");
        assert_eq!(to_level1_name("longfilename.extension"), "LONGFILE.EXT;1");
        assert_eq!(to_level1_name("network-config.yaml"), "NETWORK_.YAM;1");
    }

    #[test]
    fn iso_empty_file() {
        let iso = build_iso(
            "TEST",
            &[IsoFile {
                name: "empty",
                data: b"",
            }],
        );
        assert_eq!(&iso[0x8001..0x8006], b"CD001");
        assert_eq!(iso.len() % SECTOR_SIZE, 0);
    }

    #[test]
    fn iso_large_file_spans_sectors() {
        let big = vec![0xABu8; 5000]; // >2 sectors
        let iso = build_iso(
            "TEST",
            &[IsoFile {
                name: "big.bin",
                data: &big,
            }],
        );
        // system(16) + pvd(1) + vdst(1) + pt_l(1) + pt_m(1) + rootdir(1) + ce(1) + file(3)
        let expected_sectors = 16 + 1 + 1 + 1 + 1 + 1 + 1 + 3;
        assert_eq!(iso.len(), expected_sectors * SECTOR_SIZE);
        let file_start = 22 * SECTOR_SIZE;
        assert_eq!(&iso[file_start..file_start + 5000], big.as_slice());
    }

    #[test]
    fn iso_path_table_points_to_root() {
        let iso = sample_iso();
        // L-type path table at sector 18.
        let pt = &iso[18 * SECTOR_SIZE..];
        let extent = u32::from_le_bytes([pt[2], pt[3], pt[4], pt[5]]);
        assert_eq!(
            extent, 20,
            "L path table should point to root dir at sector 20"
        );

        // M-type path table at sector 19.
        let pt = &iso[19 * SECTOR_SIZE..];
        let extent = u32::from_be_bytes([pt[2], pt[3], pt[4], pt[5]]);
        assert_eq!(
            extent, 20,
            "M path table should point to root dir at sector 20"
        );
    }
}
