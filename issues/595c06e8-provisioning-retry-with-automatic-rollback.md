# Provisioning retry with automatic rollback

**ID:** 595c06e8 | **Status:** Open | **Created:** 2026-02-22T14:30:00+01:00

**Depends on:** c48a9373 (VM snapshot and rollback support)

## Summary

Fast provisioning iteration loop: `rum up` automatically snapshots before provisioning, and `rum retry` rolls back to that snapshot and re-runs with the latest config. Works whether provisioning succeeded or failed — the user decides when they're done iterating.

No more `rum destroy && rum up` cycle — iterate on provisioning in seconds instead of minutes.

## User experience

### During `rum up` — automatic pre-provision snapshot

When `rum up` reaches the provisioning phase (after VM boot, agent ready), it automatically creates an internal snapshot called `pre-provision`. This happens transparently as a progress step.

### After `rum up` completes (success or failure)

```
  ✓ Running system provisioning
  ✓ Running boot provisioning
  — Ready

  VM is running. Press Ctrl+C to stop...
  Tip: edit your scripts and run `rum retry` to re-provision from a clean state.
```

On failure:
```
  ✗ Running system provisioning (exit code 1)
  — Running boot provisioning (skipped)
  — Provisioning failed — VM kept running for debugging

  Use `rum ssh` to debug, `rum log --failed` for logs.
  Edit your script, then run `rum retry` to rollback and re-provision.
  Press Ctrl+C to stop...
```

### `rum retry` command

```
rum retry              # rollback to pre-provision snapshot, re-run all provisioning
rum retry --from boot  # rollback and re-run only boot provisioning
```

`rum retry`:
1. Restores the `pre-provision` snapshot (instant — no reboot needed with live snapshot)
2. Re-reads `rum.toml` from disk (picks up script edits)
3. Runs provisioning scripts via agent RPC (same as `rum up` does)
4. On completion: keeps the snapshot for further retries
5. User runs `rum retry --done` or `rum snapshot delete pre-provision` when satisfied

This works regardless of whether the previous run succeeded or failed. The user is in control — want to tweak a package list? Edit the script and `rum retry`. Want to try a different approach? Same flow.

### Keyboard shortcut during `rum up`

While the VM is idle after provisioning (success or failure):
- `r` — trigger retry inline (same as `rum retry`)
- `Ctrl+C` — stop VM

## Implementation

### `rum up` changes (`backend/libvirt.rs`)

Before running provisioning scripts (step 7), create an internal snapshot:
```rust
// Between step 6 (agent ready) and step 7 (provision scripts)
if just_started && !provision_scripts.is_empty() {
    create_snapshot(&dom, "pre-provision")?;
}
```

### `rum retry` command

1. **`src/cli.rs`** — Add `Retry` variant with optional `--from` flag and `--done` flag
2. **`src/main.rs`** — Dispatch: load config, connect to libvirt, restore snapshot, run provisioning
3. **`src/backend/libvirt.rs`** — Add `retry()` method:
   - `--done`: delete `pre-provision` snapshot, print "Snapshot removed", exit
   - Otherwise: restore `pre-provision` snapshot
   - Wait for agent (should be fast — VM state is restored)
   - Re-read config from disk
   - Build provision scripts from config
   - Run via `agent::run_provision()`
   - Print results + retry instructions

### Snapshot lifecycle

- **Created:** automatically during first `rum up` before provisioning
- **Restored:** by `rum retry` (repeatable — snapshot persists)
- **Deleted:** by `rum retry --done` or `rum destroy`
- **Preserved:** across `rum down` / `rum up` cycles (lives in qcow2 overlay)
- **Not recreated:** on subsequent `rum up` if snapshot already exists (idempotent)

## Considerations

- Live snapshots include memory state — restore is instant (no reboot)
- If user edits `rum.toml` between retries, the new config is used
- Multiple retries work — each restore goes back to the same clean pre-provision state
- The `r` keyboard shortcut during failure output requires the terminal input handling from 05a5d038
- Consider adding `rum retry --reset` to also regenerate the seed ISO (for cloud-init changes)
- The snapshot is user-visible via `rum snapshot list` — full transparency
