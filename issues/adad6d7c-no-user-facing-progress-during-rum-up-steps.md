# No user-facing progress during rum up steps

**ID:** adad6d7c | **Status:** Done | **Created:** 2026-02-13T21:18:40+01:00

## Summary

Between image download and console attachment, `rum up` is silent — domain definition, network checks, VM startup only produce `tracing::info` output requiring `--verbose`. Same for `rum down` which silently waits up to 30s for ACPI shutdown. Users think the command is hung.

## Approach

Add user-facing status messages (println or indicatif spinners) for each major step. Keep it minimal — one line per step.

## Tasks

- [ ] Add status messages for each step in `up`
- [ ] Add spinner/progress during `down` shutdown wait
