# Port forwarding support

**ID:** f0939dea | **Status:** Open | **Created:** 2026-02-15T14:31:21+01:00

Forward host ports to guest ports so services running in the VM are accessible from the host via `localhost:<port>`.

## Approach: vsock-based forwarding

Use **virtio-vsock** (AF_VSOCK) as a direct host↔guest transport to forward configured ports. This bypasses the network stack entirely — no extra NICs, no iptables rules, no IP addressing needed for the forwarding path.

### How it works

1. **Domain XML** — add a `<vsock>` device with auto-assigned CID
2. **Guest agent** — a small binary (shipped via cloud-init) listens on vsock and forwards incoming connections to `localhost:<guest-port>`
3. **Host proxy** — rum listens on `localhost:<host-port>` and forwards over vsock to the guest agent

### Why vsock

- No network configuration needed for the forwarding channel
- Very low overhead — no TCP/IP stack in the host↔guest path
- Works even without guest networking configured
- Libvirt supports it natively (`<vsock>` element in domain XML)
- This is the approach Lima/Colima use (via gvisor-tap-vsock)

### Crate

[vsock-rs](https://github.com/rust-vsock/vsock-rs) — Rust bindings for AF_VSOCK sockets (`VsockListener`, `VsockStream`). Thin wrapper around the kernel API.

### Config

```toml
[[ports]]
host = 8080
guest = 80

[[ports]]
host = 5432
guest = 5432
```

### Implementation tasks

- [ ] Add `<vsock>` device to domain XML generation
- [ ] Build a minimal guest agent binary that accepts vsock connections and proxies to local TCP ports
- [ ] Ship guest agent via cloud-init (download or embed in seed ISO)
- [ ] Host-side proxy: rum listens on configured host ports and forwards over vsock
- [ ] Guest agent protocol: multiplexed connections with target port info
- [ ] Discover CID from libvirt domain XML after define

### Trade-offs

- Requires a guest agent (scope increase vs. dual-NIC approach)
- Only forwards explicitly configured ports (dual-NIC exposes all ports)
- Guest agent needs to be built, shipped, and started inside the VM

## Research notes

### How Lima does it

Lima uses QEMU directly (no libvirt) with **gvisor-tap-vsock** (userspace network stack) and tunnels ports through a gRPC guest agent connection (~5.4 Gbits/sec). They also support SSH-based forwarding as a fallback. This gives them automatic catch-all forwarding of all non-privileged ports with zero config.

The trade-off: Lima had to build all VM lifecycle management themselves. Libvirt gives us domain lifecycle, network management, `virsh console`, etc. for free — but locks us out of QEMU's user-mode networking features like `hostfwd`.

### Other approaches considered

1. **Dual-NIC (NAT + host-only bridge)** — separated into its own issue (`ab81286c`). Exposes all guest ports via a routable bridge IP with zero config. Simpler but less precise than port forwarding.
2. **SSH tunneling** — forward ports over SSH (`-L host:guest`). Simple, no root needed. Downside: requires SSH to be up first, adds latency.
3. **iptables/nftables hook scripts** — `/etc/libvirt/hooks/qemu` script that adds DNAT rules on guest start/stop. Requires root, knowing the guest IP, managing firewall rules.
4. **passt backend** — newer QEMU networking backend with built-in port forwarding, unprivileged. Libvirt supports it via `<interface type='passt'>` (libvirt 9.2+).
5. **Drop libvirt, use QEMU directly** — full control over networking but lose all libvirt conveniences.

## Related

- Dual-NIC networking (`ab81286c`) — complementary approach for exposing all ports
- `rum init` wizard (`e2b0661e`) includes a port forwarding prompt
