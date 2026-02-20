# Dynamic auto port forwarding

**ID:** e214ac49 | **Status:** Open | **Created:** 2026-02-20T22:24:37+01:00

## Summary

Auto-detect when guest programs start listening on ports within a configured range and dynamically forward them to the host. Builds on the static `[[ports]]` forwarding (vsock port 2223) already implemented.

Use case: tools like vibedb that spawn database forks on dynamic ports in a range — you don't know which ports ahead of time.

## Config

```toml
[auto_ports]
range = "5000-6000"
bind = "127.0.0.1"   # optional, default 127.0.0.1
```

Static `[[ports]]` and `[auto_ports]` work side by side.

## Design: host polls agent via RPC

Every ~1s the host calls a new `listening_ports` RPC method over the existing roam channel (vsock port 2222). The agent reads `/proc/net/tcp` + `/proc/net/tcp6`, filters to the requested range, returns the set. The host diffs against currently forwarded ports and starts/stops TCP listeners.

### Agent side (rum-agent)

1. **lib.rs** — add RPC method to trait:
   ```rust
   async fn listening_ports(&self, min: u16, max: u16) -> Result<ListeningPortsResponse, String>;
   ```
   With `ListeningPortsResponse { ports: Vec<u16> }`.

2. **main.rs** — implement by parsing `/proc/net/tcp` and `/proc/net/tcp6`:
   - Skip header line
   - Field 1 = `hex_ip:hex_port` (local address)
   - Field 3 = state (`0A` = LISTEN)
   - Filter port to `[min, max]`, dedup, return

### Host side (rum/src/agent.rs)

New function:
```rust
pub async fn start_auto_forwards(
    cid: u32,
    min_port: u16,
    max_port: u16,
    bind: &str,
) -> Result<JoinHandle<()>, RumError>
```

Spawns a task that:
- Creates a persistent roam RPC client to the agent (separate connection from wait_for_agent)
- Every 1s, calls `listening_ports(min, max)`
- Maintains `HashMap<u16, JoinHandle<()>>` of active forwards
- New port detected → bind TCP listener, start proxy (same mechanism as static forwards), print `Auto-forwarding bind:port → guest:port`
- Port gone → abort handle, print `Stopped forwarding port`

### Wiring (libvirt.rs)

After static `start_port_forwards()`, if `config.auto_ports` is set, start the auto-forward task. Abort on Ctrl+C alongside the other handles.

### Config (config.rs)

```rust
#[derive(Debug, Clone, Facet)]
pub struct AutoPortsConfig {
    pub range: String,        // "5000-6000"
    #[facet(default = "127.0.0.1")]
    pub bind: String,
}
```

Add `pub auto_ports: Option<AutoPortsConfig>` to `Config`. Validation: parse range as `min-max`, ensure min <= max, both > 0, no overlap with static `[[ports]]` host ports.

## Testing

- Unit test `/proc/net/tcp` parsing with sample data
- Config parsing + validation tests (valid range, invalid range, overlap with static ports)
- Manual: start a listener on a port in range inside guest, verify host auto-forwards within ~1s
