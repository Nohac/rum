# Add rum log command for accessing provisioning logs

**ID:** 753c605b | **Status:** Done | **Created:** 2026-02-21T16:17:56+01:00

When a provisioning script fails, the output is shown during `rum up` but lost once the terminal scrolls or the session ends. There's no way to review what happened after the fact.

## Log types

Two separate log categories, both under `~/.local/share/rum/<name>/logs/`:

1. **rum log** (`rum.log`) — rum's own tracing output (libvirt calls, config loading, errors). Single file, appended each run. Rotated by size or count.

2. **Script logs** (`scripts/`) — one file per provisioning script execution, named with timestamp + script name + exit status (e.g. `2026-02-21T16-00-00_rum-system_ok.log`, `2026-02-21T16-00-00_rum-boot_failed.log`). Contains the script's stdout/stderr. Having status in the filename makes filtering for failed runs trivial without parsing.

## Log rotation

Keep only the last 10 log files per script name. On each `rum up`, delete oldest files beyond the limit.

## `rum log` subcommand

- `rum log` — show the most recent script log
- `rum log --failed` — show only failed script logs (filename contains `_failed`)
- `rum log --all` — list all available script logs
- `rum log --follow` — tail the active script log in real-time
- `rum log --rum` — show rum's own internal log
