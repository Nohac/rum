# Flexible multi-NIC networking

**ID:** ab81286c | **Status:** Done | **Created:** 2026-02-15T22:24:58+01:00

Make network configuration flexible: support multiple NICs, bridge mode with custom IPs, and the ability to disable NAT.

## Config

```toml
# NAT is added by default. Set to false to disable.
[network]
nat = true  # default: true

# Add additional NICs via [[network.interfaces]]
[[network.interfaces]]
mode = "bridge"
bridge = "rum-hostonly"
ip = "192.168.50.10"

[[network.interfaces]]
mode = "bridge"
bridge = "dev-net"
ip = "10.0.0.5"
```

No `[[network.interfaces]]` entries and `nat = true` (default) gives the same behavior as today — a single NAT NIC.

Setting `nat = false` with no interfaces gives the VM no networking at all (useful for air-gapped/isolated VMs).

## Implementation

1. **Config parsing** — add `nat` bool and `interfaces` vec to `NetworkConfig`
2. **Domain XML** — generate one `<interface>` per entry; conditionally include the NAT interface based on `nat` flag
3. **Bridge management** — create host-only libvirt networks on demand (or require pre-existing bridges)
4. **IP discovery** — expose guest IPs from bridge DHCP leases or static config

## Open questions

- Should rum auto-create host-only libvirt networks, or require the user to set them up?
- Static IP assignment: cloud-init netplan vs. libvirt DHCP reservation?

## Related

- Port forwarding (`f0939dea`) — vsock-based approach for explicit port mappings
- `rum init` wizard (`e2b0661e`) includes a networking prompt
