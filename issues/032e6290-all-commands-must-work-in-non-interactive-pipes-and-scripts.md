# All commands must work in non-interactive pipes and scripts

**ID:** 032e6290 | **Status:** Open | **Created:** 2026-02-22T14:57:18+01:00

## Summary

Every rum command should produce clean, pipe-friendly output when stdout is not a TTY. Interactive prompts (inquire selectors, spinners, progress bars) should be skipped or replaced with plain text so that `rum image search | grep ubuntu` and similar pipelines work.

## Currently broken commands

### `rum image search`

Uses `inquire::Select` for interactive image picker. When piped, inquire panics or hangs because there's no TTY to render to.

**Non-interactive behavior:** print a plain list of available images (one per line, `label\tURL` format) and exit. With a query filter, only show matches.

```bash
$ rum image search | grep -i ubuntu
Ubuntu 24.04 LTS (Noble)	https://cloud-images.ubuntu.com/noble/...
Ubuntu 22.04 LTS (Jammy)	https://cloud-images.ubuntu.com/jammy/...

$ rum image search ubuntu
Ubuntu 24.04 LTS (Noble)	https://cloud-images.ubuntu.com/noble/...
Ubuntu 22.04 LTS (Jammy)	https://cloud-images.ubuntu.com/jammy/...
```

### `rum init`

Uses `inquire::Select`, `Confirm`, `Text`, `CustomType` for the setup wizard. Already has `--defaults` flag for non-interactive use, but running without `--defaults` in a pipe will fail.

**Non-interactive behavior:** detect non-TTY and behave as if `--defaults` was passed, or error with a helpful message like `"rum init requires a terminal (use --defaults for non-interactive mode)"`.

## Commands that already work

- `rum up` — already detects non-TTY via `std::io::stdout().is_terminal()` and switches to `OutputMode::Plain`
- `rum status`, `rum down`, `rum destroy` — plain println output
- `rum log` — plain text output
- `rum image list` — plain text output
- `rum ssh-config` — plain text output

## Approach

### General pattern

Check `std::io::stdout().is_terminal()` (already imported in `main.rs`) before using interactive UI. Each command should have two paths:

1. **TTY:** current interactive behavior (inquire selectors, spinners, colors)
2. **Non-TTY:** plain text output, tab-separated columns, no prompts, no ANSI codes

### `src/registry.rs` changes

In `search()`, check if stdout is a terminal:
- **TTY:** current `inquire::Select` picker → user selects → update config
- **Non-TTY:** print filtered presets as `label\tURL` lines, exit 0. Don't update config (that's an interactive action).

### `src/init.rs` changes

In `run()`, if `!defaults && !std::io::stdout().is_terminal()`:
- Either auto-enable defaults mode
- Or return an error: `"rum init requires a terminal for the interactive wizard. Use --defaults for non-interactive mode."`

## Testing

- Add integration tests that run commands with stdout piped (they already run this way in `cargo test`)
- `rum image search ubuntu` should succeed and produce grep-able output in tests
