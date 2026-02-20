# Integration test library for cargo test

**ID:** 6215eef7 | **Status:** Open | **Created:** 2026-02-20T22:51:40+01:00

## Summary

Expose rum's VM lifecycle as a Rust library so users can spin up VMs, run commands, and assert on output directly from `cargo test`. Think testcontainers but for full VMs.

## API sketch

```rust
use rum_test::VM;

#[tokio::test]
async fn test_postgres_replication() {
    let vm = VM::up("rum.toml").await.unwrap();

    let out = vm.exec("pg_isready").await.unwrap();
    assert!(out.status.success());

    let out = vm.exec("psql -c 'SELECT 1'").await.unwrap();
    assert!(out.stdout.contains("1 row"));
}

#[tokio::test]
async fn test_web_server() {
    let vm = VM::up("rum.toml").await.unwrap();

    // Port forwards are available immediately
    let resp = reqwest::get("http://localhost:8080/health").await.unwrap();
    assert_eq!(resp.status(), 200);
}
```

VM is torn down on drop (or kept alive with `VM::up_persistent()` for debugging failed tests).

### Core API

```rust
impl VM {
    /// Boot a VM from a config file. Blocks until agent is ready.
    async fn up(config: &str) -> Result<VM, rum::Error>;

    /// Execute a command in the guest via agent RPC.
    async fn exec(&self, cmd: &str) -> Result<ExecOutput, rum::Error>;

    /// Copy a file into the guest.
    async fn push(&self, local: &Path, remote: &str) -> Result<(), rum::Error>;

    /// Read a file from the guest.
    async fn pull(&self, remote: &str) -> Result<Vec<u8>, rum::Error>;

    /// Get the VM's IP address.
    fn ip(&self) -> &str;

    /// Get the forwarded port mapping (host_port for a given guest_port).
    fn forwarded_port(&self, guest_port: u16) -> Option<u16>;

    /// Destroy the VM and clean up artifacts.
    async fn destroy(self) -> Result<(), rum::Error>;
}

struct ExecOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}
```

### VM lifecycle for tests

**Per-test VMs** (default):
- `VM::up()` boots a fresh VM per test
- Drop impl calls `destroy()`
- Slow but fully isolated

**Shared VMs** (for speed):
- `VM::up_shared("name")` — reuses a running VM if it exists, boots if not
- Tests share state — use for read-only tests or when boot cost dominates
- Cleanup at end of test suite, not per test

**Snapshot-based reset** (future optimization):
- Boot once, snapshot after provisioning
- Each test reverts to snapshot — fast reset without full boot
- Requires qcow2 snapshot support in overlay management

### Boot latency mitigation

Full VM boot is 20-60s. Strategies:
1. **Shared VMs** across tests in the same module (see above)
2. **Parallel test VMs** — `cargo test` runs tests in parallel; each gets its own VM with unique port mappings
3. **Pre-built overlays** — cache provisioned overlays so subsequent boots skip cloud-init
4. **`exec` over vsock** — command execution is fast once the VM is running (no SSH overhead)

### Crate structure

New crate: `rum-test` (in workspace), depends on `rum` as a library.

```toml
[dev-dependencies]
rum-test = { path = "../rum-test" }
```

Internally, `rum-test` calls rum's backend API directly (not CLI), so no process spawning overhead.

## Dependencies

- Depends on: agent `exec` RPC method (`042ee8d2`)
- Benefits from: agent log forwarding (for test output on failure)
- Nice to have: file push/pull via agent RPC

## Tasks

- [ ] Extract rum backend into a clean library API (currently tied to CLI flow)
- [ ] Implement agent `exec` RPC (in issue `042ee8d2`)
- [ ] Create `rum-test` crate with `VM` struct and lifecycle management
- [ ] Shared VM support with reference counting
- [ ] Port mapping helpers for test assertions
- [ ] File push/pull via agent RPC
- [ ] Example test suite demonstrating the API
- [ ] Documentation
