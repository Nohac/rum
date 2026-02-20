# Benchmark vsock throughput and latency

**ID:** 22554152 | **Status:** Open | **Created:** 2026-02-20T21:31:10+01:00

Now that roam RPC works over vsock, measure actual performance to understand what's feasible for the inotify bridge and future file transfer features.

## What to measure

- **RPC round-trip latency**: ping() call time (should be sub-millisecond for virtio-vsock)
- **Bulk throughput**: sustained transfer rate for large payloads (relevant for file sync)
- **Compare with SSH**: current inotify bridge uses russh over TCP — how does vsock compare?

## Approach

Add a simple bench command to rum-agent (or a standalone test binary) that:
1. Measures ping() latency over N iterations (min/avg/p99/max)
2. Sends increasing payload sizes and measures throughput
3. Optionally compares against SSH/TCP to the same guest
