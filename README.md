# rum

A lightweight CLI tool for provisioning and running single VM instances via libvirt.

rum uses a declarative TOML config to manage VMs with cloud images, virtiofs mounts, cloud-init provisioning, and SSH access.

## Requirements

- Linux with KVM support
- libvirt with QEMU driver (`qemu:///system`)
- `qemu-img` for disk overlay management
- `virtiofsd` for virtiofs mounts (optional, falls back to 9p)

## Usage

Create a `rum.toml` in your project directory:

```toml
name = "myvm"

[image]
base = "https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img"

[resources]
cpus = 4
memory_mb = 8192
```

Then:

```sh
rum up          # create and start the VM
rum ssh         # connect to the VM
rum down        # gracefully stop the VM
rum destroy     # remove the VM and artifacts
rum status      # show VM state, IP, mounts
rum logs        # show cloud-init output
```

## Building

```sh
cargo build --release
```

## License

MIT
