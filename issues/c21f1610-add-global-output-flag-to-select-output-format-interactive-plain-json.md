# Add global --output flag to select output format (interactive, plain, json)

**ID:** c21f1610 | **Status:** Done | **Created:** 2026-02-27T17:38:32+01:00

## Summary

There's no way to select the output format from the CLI. The `JsonObserver` and
`PlainObserver` exist but can't be activated. Need a global `--output` flag (or `-o`) to
choose between interactive, plain, and json output.

## Current state

`OutputMode` in `progress.rs` has 4 variants: `Normal`, `Verbose`, `Quiet`, `Plain`.
Selection in `main.rs`:

```rust
let mode = if cli.quiet {
    OutputMode::Quiet
} else if cli.verbose {
    OutputMode::Verbose
} else if !stdout().is_terminal() || !stdin().is_terminal() {
    OutputMode::Plain
} else {
    OutputMode::Normal
};
```

No `Json` variant. No explicit flag — plain is auto-detected from non-TTY, the rest come
from `--verbose` / `--quiet`. The three observer implementations (`InteractiveObserver`,
`PlainObserver`, `JsonObserver`) have no path to be selected.

## Approach

Add a `--output` / `-o` global flag to `Cli` with a `clap::ValueEnum`:

```rust
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum OutputFormat {
    /// Spinners, progress bars, colored output (default for TTY)
    Auto,
    /// Interactive TTY output (force even in non-TTY)
    Interactive,
    /// Plain text, no ANSI codes (default for pipes)
    Plain,
    /// JSON-lines to stdout (machine-readable)
    Json,
}
```

```rust
pub struct Cli {
    #[arg(short, long, default_value = "auto")]
    pub output: OutputFormat,
    // ...
}
```

Resolution logic in `main.rs`:

```rust
let output = match cli.output {
    OutputFormat::Auto => {
        if !stdout().is_terminal() { OutputFormat::Plain }
        else { OutputFormat::Interactive }
    }
    other => other,
};
```

Then use `output` to select the observer:

```rust
let observer: Box<dyn Observer> = match output {
    OutputFormat::Interactive => Box::new(InteractiveObserver::new()),
    OutputFormat::Plain => Box::new(PlainObserver),
    OutputFormat::Json => Box::new(JsonObserver),
};
```

### Interaction with `--verbose` / `--quiet`

These become modifiers within the interactive/plain modes (e.g. verbose shows debug logs,
quiet suppresses step logs). They don't apply to JSON mode — JSON always emits everything.
Could warn or error if `--quiet --output json` is used, or just ignore the conflict.

### `OutputMode` refactor

The old `OutputMode` enum in `progress.rs` should be consolidated with the new
`OutputFormat`. Once observers handle all rendering, `OutputMode` and `StepProgress` can
be deprecated in favor of the observer pattern. For now they can coexist — `OutputMode`
drives the legacy `StepProgress` rendering, `OutputFormat` drives the new observer
selection.

## Files

- `src/cli.rs` — add `OutputFormat` enum and `--output` flag to `Cli`
- `src/main.rs` — resolve auto-detection, select observer based on format
- `src/progress.rs` — eventually deprecate `OutputMode` once observers take over
