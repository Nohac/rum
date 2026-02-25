# JSON output mode for agent-friendly structured output

**ID:** 1cc70306 | **Status:** Open | **Created:** 2026-02-24T20:41:07+01:00

Add `--json` flag (or `--format json`) to key commands so AI agents and scripts can parse output without regex. Priority commands:

- `rum status --json` → `{"name": "...", "state": "running", "ips": [...], "daemon": true}`
- `rum exec --json` → `{"exit_code": 0, "stdout": "...", "stderr": "..."}`
- `rum log --json` → structured log entries
