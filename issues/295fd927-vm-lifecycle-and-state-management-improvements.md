# VM lifecycle and state management improvements

**ID:** 295fd927 | **Status:** Done | **Created:** 2026-02-21T14:08:44+01:00

Several rough edges around VM lifecycle and concurrent state management.

## Problems

### 1. Ctrl+C during `rum up` doesn't shut down the VM

`rum up` waits on `ctrl_c()` after boot, but just exits without running `down` logic. The VM keeps running in the background. Ctrl+C should trigger a graceful ACPI shutdown (same as `rum down`) before exiting.

### 2. `rum down`/`rum destroy` while `rum up` is running — no coordination

If `rum up` is holding the terminal and you run `rum down` or `rum destroy` from another shell, the `rum up` process doesn't notice. It keeps sitting at "Press Ctrl+C to stop..." even though the VM is gone. Should detect that the domain was stopped/undefined externally and exit cleanly.

### 3. `rum destroy`/`rum down` reports success even when there's nothing to destroy

Running `rum destroy` always prints `VM '<id>' destroyed.` even if no domain exists and no work directory exists. Should check what actually existed and report accordingly:
- If nothing existed: "VM '<id>' not found — nothing to destroy."
- If only artifacts existed (no domain): "Removed artifacts for '<id>'."
- Same for `rum down` — currently prints nothing useful if the VM doesn't exist as a domain.

## Approach

- **Ctrl+C shutdown**: Replace the bare `ctrl_c().await` in `rum up` with shutdown logic — ACPI shutdown + wait (reuse existing `shutdown_domain` helper), then abort log/forward/watch handles and exit.
- **External stop detection**: After boot, poll domain state periodically (or watch for libvirt events) alongside `ctrl_c()`. If the domain disappears, print a message and exit.
- **Honest destroy/down output**: Check whether domain/artifacts actually existed before printing success messages. Use distinct messages for "nothing found", "only artifacts cleaned", "domain stopped + artifacts cleaned".
