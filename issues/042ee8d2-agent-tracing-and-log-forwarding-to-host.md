# Agent tracing and log forwarding to host

**ID:** 042ee8d2 | **Status:** Done | **Created:** 2026-02-20T22:51:40+01:00

## Summary

Replace `eprintln!` in rum-agent with the `tracing` crate and forward structured log events to the host over vsock. This gives the host real-time visibility into what's happening inside the guest — cloud-init progress, script execution, service starts — and feeds directly into the UX overhaul (issue `a5c0d91a`).

## Current state

- Agent uses `eprintln!` for logging — only visible if you SSH in and check journalctl
- Host is blind during cloud-init and provisioning — just shows a spinner
- No way to surface script output or errors to the user

## Design

### Agent side

Switch rum-agent to `tracing` with a custom subscriber that:
1. Logs locally to stderr/journald (for debugging when SSH'd in)
2. Sends structured events over vsock to the host

Two options for the transport:

**Option A: Streaming RPC** — Add a `subscribe_logs` method that returns a stream of log events over the existing roam channel (port 2222). Roam may support streaming; if not, use polling.

**Option B: Dedicated vsock port** — Bind a third vsock listener (e.g. port 2224) for raw log streaming. Simpler framing: length-prefixed JSON or msgpack log records. Host connects after agent readiness, receives events.

Option B is probably better — keeps log traffic separate from RPC, avoids backpressure on RPC calls, and is simpler to implement.

### Log event structure

```rust
struct LogEvent {
    timestamp: u64,     // unix millis
    level: Level,       // trace/debug/info/warn/error
    target: String,     // tracing target (module path)
    message: String,
    fields: HashMap<String, String>,  // structured fields
}
```

### Host side

- Connect to vsock log port after `wait_for_agent()`
- Spawn a task that reads events and feeds them to the UX layer
- For now (before UX overhaul), just print with `tracing::info!` prefixed with `[guest]`
- Later, the UX layer consumes these events for live log display under progress steps

### Agent-driven deployment scripts

This issue also lays groundwork for moving deployment scripts from cloud-init to the agent. Currently `provision.system` and `provision.boot` scripts are embedded in the cloud-init seed ISO. Instead:

1. Agent receives scripts via RPC (or reads them from a well-known path deployed via cloud-init)
2. Agent executes scripts, streaming stdout/stderr as log events back to the host
3. Host displays script output in real time
4. Exit code is returned via RPC response

Benefits:
- Real-time script output visible on host (no need to SSH in to debug)
- Scripts can be updated without regenerating the seed ISO
- Better error reporting — host can show the exact line that failed
- Agent can report structured progress (e.g. "installing package X of Y")

New RPC methods:
```rust
async fn exec(&self, command: String) -> Result<ExecResponse, String>;
```

Where log events from the script execution are forwarded via the log stream, and `ExecResponse` contains the exit code.

## Dependencies

- Blocks: UX overhaul (`a5c0d91a`), integration test lib (`6215eef7`)
- Prereq: none (can start immediately)

## Tasks

- [ ] Add `tracing` + `tracing-subscriber` deps to rum-agent
- [ ] Replace `eprintln!` with `tracing::info!`/`tracing::error!` etc.
- [ ] Implement vsock log stream (port 2224) with length-prefixed events
- [ ] Host-side: connect to log stream, display with `[guest]` prefix
- [ ] Add `exec` RPC method for running scripts via agent
- [ ] Migrate provisioning from cloud-init runcmd to agent exec
