# Refactor up() control flow from scattered atomics to a state machine

**ID:** 307566fa | **Status:** Done | **Created:** 2026-02-23T23:34:52+01:00

## Problem

`up()` in `libvirt.rs` uses a tangle of `AtomicBool` flags (`vm_started`, `ctrl_c_pressed`, `first_boot`, `detached`) scattered across nested `tokio::select!` arms, with shutdown logic that reads these atomics after the select to decide what to do. This causes bugs:

- After "Ready" / "VM is running. Press Ctrl+C to stop..." it still runs "[10/10] Destroying interrupted first boot..." because the atomics combine in unexpected ways
- The `detached` flag was added as a band-aid to skip shutdown but the root issue remains
- Adding new states (like daemon mode) keeps adding more atomics

## Observed bug

```
[9/10] — Ready
      → Forwarding 127.0.0.1:8080 → guest:8080
VM is running. Press Ctrl+C to stop...
[10/10] ✓ Destroying interrupted first boot...
First boot interrupted — VM state destroyed. Run `rum up` again for a clean start.
```

The VM reached "Ready" (fully provisioned, services running) but Ctrl+C still took the `first_boot` path and destroyed everything.

## Approach

Replace the atomic flags with an explicit state enum that tracks where `up()` is in its lifecycle:

```rust
enum UpPhase {
    Preparing,       // image/overlay/seed/domain — safe to just exit
    Starting,        // dom.create() called but agent not ready — first boot
    Provisioning,    // agent connected, scripts running — first boot
    Running,         // fully ready, services active — normal shutdown
    Detached,        // daemon spawned, exit without shutdown
}
```

The shutdown logic becomes a single `match` on the phase:
- `Preparing` → nothing to do
- `Starting` / `Provisioning` → destroy + wipe (interrupted first boot)
- `Running` → ACPI shutdown
- `Detached` → exit cleanly

This eliminates all `AtomicBool`s and the `tokio::select!` around the entire body. Ctrl+C handling moves to a simpler structure where the phase is checked at the point of interruption.

Subsumes: #60d8296d (progress step consolidation) — the phase enum naturally groups steps.
