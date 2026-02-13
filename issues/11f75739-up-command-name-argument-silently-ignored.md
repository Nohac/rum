# Up command name argument silently ignored

**ID:** 11f75739 | **Status:** Open | **Created:** 2026-02-13T21:18:40+01:00

## Summary

`cli.rs:23` defines `Up { name: Option<String>, reset: bool }` but `main.rs:28` destructures it as `{ reset, .. }`, silently discarding `name`. Running `rum up myvm` appears to work but ignores the name entirely.

## Approach

Remove `name` from the `Up` variant until it's properly supported. Avoids confusing users who think they're overriding the VM name.

## Tasks

- [ ] Remove `name` from the `Up` CLI variant
