use facet::Facet;
use facet_xml as xml;

// ── XML model structs ──────────────────────────────────────
//
// Each struct maps to a libvirt XML element. Attributes use
// `#[facet(xml::attribute)]`, text content uses `#[facet(xml::text)]`,
// and child elements are nested structs.

#[derive(Debug, Facet)]
#[facet(rename = "domain")]
pub(super) struct Domain {
    #[facet(xml::attribute, rename = "type")]
    pub(super) domain_type: String,
    pub(super) name: String,
    pub(super) memory: Memory,
    pub(super) vcpu: u32,
    pub(super) os: Os,
    #[facet(default, rename = "memoryBacking")]
    pub(super) memory_backing: Option<MemoryBacking>,
    pub(super) features: Features,
    pub(super) devices: Devices,
}

#[derive(Debug, Facet)]
pub(super) struct Memory {
    #[facet(xml::attribute)]
    pub(super) unit: String,
    #[facet(xml::text)]
    pub(super) value: u64,
}

// ── OS ─────────────────────────────────────────────────────

#[derive(Debug, Facet)]
pub(super) struct Os {
    #[facet(rename = "type")]
    pub(super) os_type: OsType,
    pub(super) boot: Boot,
}

#[derive(Debug, Facet)]
#[facet(rename = "type")]
pub(super) struct OsType {
    #[facet(xml::attribute)]
    pub(super) arch: String,
    #[facet(xml::attribute)]
    pub(super) machine: String,
    #[facet(xml::text)]
    pub(super) value: String,
}

#[derive(Debug, Facet)]
pub(super) struct Boot {
    #[facet(xml::attribute)]
    pub(super) dev: String,
}

// ── memoryBacking (required for virtiofs) ──────────────────

#[derive(Debug, Facet)]
pub(super) struct MemoryBacking {
    pub(super) source: MemoryBackingSource,
    pub(super) access: MemoryBackingAccess,
}

#[derive(Debug, Facet)]
pub(super) struct MemoryBackingSource {
    #[facet(xml::attribute, rename = "type")]
    pub(super) source_type: String,
}

#[derive(Debug, Facet)]
pub(super) struct MemoryBackingAccess {
    #[facet(xml::attribute)]
    pub(super) mode: String,
}

// ── features ───────────────────────────────────────────────

#[derive(Debug, Facet)]
pub(super) struct Features {
    pub(super) acpi: Empty,
    pub(super) apic: Empty,
}

#[derive(Debug, Default, Facet)]
#[facet(default)]
pub(super) struct Empty {}

// ── devices ────────────────────────────────────────────────

#[derive(Debug, Facet)]
pub(super) struct Devices {
    pub(super) disk: Vec<Disk>,
    pub(super) filesystem: Vec<Filesystem>,
    pub(super) interface: Vec<Interface>,
    pub(super) serial: Serial,
    pub(super) console: Console,
    pub(super) vsock: Vsock,
}

#[derive(Debug, Facet)]
pub(super) struct Disk {
    #[facet(xml::attribute, rename = "type")]
    pub(super) disk_type: String,
    #[facet(xml::attribute)]
    pub(super) device: String,
    pub(super) driver: DiskDriver,
    pub(super) source: DiskSource,
    pub(super) target: DiskTarget,
    #[facet(default)]
    pub(super) readonly: Option<Empty>,
}

#[derive(Debug, Facet)]
pub(super) struct DiskDriver {
    #[facet(xml::attribute)]
    pub(super) name: String,
    #[facet(xml::attribute, rename = "type")]
    pub(super) driver_type: String,
}

#[derive(Debug, Facet)]
pub(super) struct DiskSource {
    #[facet(xml::attribute)]
    pub(super) file: String,
}

#[derive(Debug, Facet)]
pub(super) struct DiskTarget {
    #[facet(xml::attribute)]
    pub(super) dev: String,
    #[facet(xml::attribute)]
    pub(super) bus: String,
}

// ── virtiofs filesystem ────────────────────────────────────

#[derive(Debug, Facet)]
pub(super) struct Filesystem {
    #[facet(xml::attribute, rename = "type")]
    pub(super) fs_type: String,
    #[facet(xml::attribute)]
    pub(super) accessmode: String,
    pub(super) driver: FsDriver,
    pub(super) source: FsSource,
    pub(super) target: FsTarget,
    #[facet(default)]
    pub(super) readonly: Option<Empty>,
}

#[derive(Debug, Facet)]
pub(super) struct FsDriver {
    #[facet(xml::attribute, rename = "type")]
    pub(super) driver_type: String,
}

#[derive(Debug, Facet)]
pub(super) struct FsSource {
    #[facet(xml::attribute)]
    pub(super) dir: String,
}

#[derive(Debug, Facet)]
pub(super) struct FsTarget {
    #[facet(xml::attribute)]
    pub(super) dir: String,
}

// ── network ────────────────────────────────────────────────

#[derive(Debug, Facet)]
pub(super) struct Interface {
    #[facet(xml::attribute, rename = "type")]
    pub(super) iface_type: String,
    #[facet(default)]
    pub(super) mac: Option<InterfaceMac>,
    pub(super) source: InterfaceSource,
    pub(super) model: InterfaceModel,
}

#[derive(Debug, Facet)]
pub(super) struct InterfaceMac {
    #[facet(xml::attribute)]
    pub(super) address: String,
}

#[derive(Debug, Facet)]
pub(super) struct InterfaceSource {
    #[facet(xml::attribute)]
    pub(super) network: String,
}

#[derive(Debug, Facet)]
pub(super) struct InterfaceModel {
    #[facet(xml::attribute, rename = "type")]
    pub(super) model_type: String,
}

// ── vsock ─────────────────────────────────────────────────

#[derive(Debug, Facet)]
pub(super) struct Vsock {
    #[facet(xml::attribute)]
    pub(super) model: String,
    pub(super) cid: VsockCid,
}

#[derive(Debug, Facet)]
pub(super) struct VsockCid {
    #[facet(xml::attribute)]
    pub(super) auto: String,
}

// ── vsock deserialization (live XML) ─────────────────────

/// Deserialization struct for the `<vsock>` element in live domain XML.
///
/// Live XML includes an `address` attribute on `<cid>` that is not present
/// in the generation struct (since libvirt auto-assigns the CID).
#[derive(Debug, Facet)]
#[facet(rename = "vsock")]
pub(super) struct LiveVsock {
    #[facet(xml::attribute)]
    pub(super) model: String,
    pub(super) cid: LiveVsockCid,
}

#[derive(Debug, Default, Facet)]
#[facet(default)]
pub(super) struct LiveVsockCid {
    #[facet(xml::attribute)]
    pub(super) auto: String,
    #[facet(xml::attribute, default)]
    pub(super) address: Option<String>,
}

// ── serial / console ───────────────────────────────────────

#[derive(Debug, Facet)]
pub(super) struct Serial {
    #[facet(xml::attribute, rename = "type")]
    pub(super) serial_type: String,
    pub(super) target: SerialTarget,
}

#[derive(Debug, Facet)]
#[facet(rename = "target")]
pub(super) struct SerialTarget {
    #[facet(xml::attribute)]
    pub(super) port: String,
}

#[derive(Debug, Facet)]
pub(super) struct Console {
    #[facet(xml::attribute, rename = "type")]
    pub(super) console_type: String,
    pub(super) target: ConsoleTarget,
}

#[derive(Debug, Facet)]
#[facet(rename = "target")]
pub(super) struct ConsoleTarget {
    #[facet(xml::attribute, rename = "type")]
    pub(super) target_type: String,
    #[facet(xml::attribute)]
    pub(super) port: String,
}
