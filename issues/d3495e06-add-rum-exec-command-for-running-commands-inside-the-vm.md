# Add rum exec command for running commands inside the VM

**ID:** d3495e06 | **Status:** Done | **Created:** 2026-02-22T14:11:33+01:00

## Summary

Add a `rum exec <command>` CLI command that runs an arbitrary command inside the VM via the rum-agent, streaming stdout/stderr back to the host in real time and exiting with the command's exit code.

## Context

The agent already exposes an `exec(command, output: Tx<LogEvent>) -> ExecResult` RPC method over vsock. The plumbing exists — this just needs CLI wiring and proper output streaming.

## Approach

1. **`src/cli.rs`** — Add `Exec` variant to `Command`:
   ```
   Exec {
       /// Command to run inside the VM
       #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
       args: Vec<String>,
   }
   ```

2. **`src/main.rs`** — Dispatch `Exec` after config loading. Connect to the agent via vsock CID, call `exec()`, stream `LogEvent`s to stdout/stderr, and exit with the returned exit code.

3. **`src/agent.rs`** — Add a public `run_exec()` function (or similar) that:
   - Connects to the agent (reuse `connect_to_agent` / `wait_for_agent`)
   - Joins args into a single shell command string
   - Calls `agent.exec(command, tx)`
   - Prints `LogEvent::Stdout` to stdout and `LogEvent::Stderr` to stderr
   - Returns the exit code from `ExecResult`

4. **`src/main.rs`** — Use `std::process::exit(code)` to propagate the guest exit code to the host, so `rum exec false` returns 1.

## Notes

- The VM must already be running — error with a helpful message if not
- No TTY/interactive support needed initially — just pipe-friendly streaming
- Should work with: `rum exec -- apt-get update`, `rum exec echo hello`, `rum exec -- bash -c "ls /tmp"`
