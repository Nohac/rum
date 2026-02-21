# rum skill command: generate agent-friendly docs for LLM tooling

**ID:** 2c3b8726 | **Status:** Open | **Created:** 2026-02-21T14:11:35+01:00

A command that outputs (or installs) a concise reference document that AI coding agents can use to create `rum.toml` configs and manage rum VMs.

## Proposed CLI

```
rum skill              # print skill/docs to stdout
rum skill --install    # install as CLAUDE.md snippet, MCP tool doc, or similar
```

## What the skill doc should cover

- Full `rum.toml` config schema with all fields, types, defaults, and examples
- Available CLI commands and their flags (`up`, `down`, `destroy`, `status`, `ssh`, `init`, `image`)
- Common workflows: create a VM, provision it, SSH in, destroy it
- Constraints and gotchas (Linux-only, KVM required, `--reset` for image changes, etc.)
- Example configs for common setups (minimal, with mounts, with drives, multi-NIC)

## Output formats to consider

- **stdout** (default): plain markdown, pipe-friendly — agent can read it directly or user can paste it
- **`--install claude`**: append to `.claude/commands/` or `CLAUDE.md` as a slash command/skill
- **`--install cursor`**: write to `.cursor/rules/` or similar
- Could also support `--install mcp` for MCP tool descriptions

## Open questions

- Should the doc be embedded in the binary (compiled in) or generated from the current binary's actual clap definitions + config struct?
- Generating from clap/config structs keeps it always in sync but is more work. Embedded markdown is simpler and easier to tune for LLM consumption.
