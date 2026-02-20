use std::sync::LazyLock;

/// Raw rum-agent binary, embedded at compile time via artifact dependency.
const AGENT_BINARY_RAW: &[u8] = include_bytes!(env!("CARGO_BIN_FILE_RUM_AGENT"));

/// Patched rum-agent binary with standard Linux interpreter/rpath.
/// NixOS builds have `/nix/store/...` paths that don't exist on Ubuntu etc.
/// arwen rewrites these to standard paths; no-op if already standard.
pub static AGENT_BINARY: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let mut writer = arwen::elf::Writer::read(AGENT_BINARY_RAW)
        .expect("rum-agent should be a valid ELF binary");
    writer
        .elf_set_interpreter(b"/lib64/ld-linux-x86-64.so.2".to_vec())
        .expect("failed to set interpreter");
    writer
        .elf_set_runpath(b"/lib/x86_64-linux-gnu:/lib64:/usr/lib".to_vec())
        .expect("failed to set rpath");
    let mut out = Vec::new();
    writer.write(&mut out).expect("failed to write patched ELF");
    out
});

pub const AGENT_SERVICE: &str = "\
[Unit]
Description=rum guest agent
After=local-fs.target

[Service]
Type=simple
ExecStart=/usr/local/bin/rum-agent
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
";
