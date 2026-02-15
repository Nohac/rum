# Port forwarding support

**ID:** f0939dea | **Status:** Open | **Created:** 2026-02-15T14:31:21+01:00

Forward host ports to guest ports so services running in the VM are accessible from the host via `localhost:<port>`.

## Recommended approach: dual-NIC (NAT + host-only bridge)

Instead of traditional port forwarding, use two network interfaces:

1. **NAT** (existing) — reliable internet access, works everywhere including WiFi
2. **Host-only bridge** — direct host ↔ guest communication on a private subnet

The guest gets a routable IP on the host-only bridge (e.g. `192.168.50.x`), making all guest ports directly accessible from the host without any forwarding rules. NAT handles internet. This avoids the fundamental limitation that libvirt has no native port forwarding in domain XML.

### Why this is better than port forwarding

- No iptables/nftables rules to manage
- No SSH tunneling overhead
- All guest ports accessible, not just configured ones
- Works with WiFi (bridge is host-only, not connected to physical network)
- Simple domain XML — just add a second `<interface>`

### Implementation

1. **Create a host-only libvirt network** (e.g. `rum-hostonly`) with a fixed subnet and DHCP
2. **Add second NIC** in domain XML: `<interface type='network'>` pointing to the host-only network
3. **Guest gets two IPs**: one from NAT (internet), one from host-only bridge (host access)
4. **rum** can discover the guest's bridge IP via libvirt's DHCP lease info

### Config

Port-specific forwarding may still be useful for exposing services on specific host ports:

```toml
[network]
mode = "nat"  # default, provides internet

[[ports]]
host = 8080
guest = 80
```

With dual-NIC, `[[ports]]` could be implemented as simple iptables DNAT on the host-only bridge (known subnet, predictable) or even just documented as "connect to `<guest-ip>:<port>`".

## Research notes

### How Lima does it

Lima uses QEMU directly (no libvirt) with **gvisor-tap-vsock** (userspace network stack) and tunnels ports through a gRPC guest agent connection (~5.4 Gbits/sec). They also support SSH-based forwarding as a fallback. This gives them automatic catch-all forwarding of all non-privileged ports with zero config.

The trade-off: Lima had to build all VM lifecycle management themselves. Libvirt gives us domain lifecycle, network management, `virsh console`, etc. for free — but locks us out of QEMU's user-mode networking features like `hostfwd`.

### Other approaches considered

1. **SSH tunneling** — forward ports over SSH (`-L host:guest`). Simple, no root needed. Downside: requires SSH to be up first, adds latency.
2. **iptables/nftables hook scripts** — `/etc/libvirt/hooks/qemu` script that adds DNAT rules on guest start/stop. Requires root, knowing the guest IP, managing firewall rules.
3. **passt backend** — newer QEMU networking backend with built-in port forwarding, unprivileged. Libvirt supports it via `<interface type='passt'>` (libvirt 9.2+).
4. **Drop libvirt, use QEMU directly** — full control over networking but lose all libvirt conveniences.

## Related

- `rum init` wizard (`e2b0661e`) includes a port forwarding prompt
