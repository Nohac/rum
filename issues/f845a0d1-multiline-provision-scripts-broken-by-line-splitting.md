# Multiline provision scripts broken by line splitting

**ID:** f845a0d1 | **Status:** Done | **Created:** 2026-02-13T21:18:40+01:00

## Summary

`cloudinit.rs:59` splits `provision.script` by lines and emits each as a separate `runcmd` entry. This breaks multiline constructs (`if/fi`, `for` loops, heredocs, `\` continuations) since each line runs as a separate shell invocation.

## Approach

Write the entire script to a file via cloud-init `write_files` and execute it as a single `runcmd`: `- ["bash", "/var/lib/cloud/scripts/rum-provision.sh"]`.

## Tasks

- [ ] Change `build_user_data` to emit the script as a single unit
- [ ] Test with a multiline provision script
