# replace cloud-init string concatenation with facet-value structured YAML

**ID:** 22a8d7fa | **Status:** Open | **Created:** 2026-02-14T13:43:26+01:00

## Summary

`build_user_data` in `cloudinit.rs` constructs cloud-init YAML via string concatenation (`push_str`). This is fragile — indentation bugs, heredoc quoting issues, and invalid YAML have caused multiple issues (autologin not working, multiline scripts broken, YAML special chars in packages).

## Approach

Use `facet-value`'s `value!` macro to build the cloud-init config as a structured `Value`, then serialize to YAML with `facet-yaml`. This guarantees valid YAML output and makes the code easier to reason about.

- Add `facet-value` and `facet-yaml` dependencies
- Build user-data as a `Value` using `value!({ ... })` with conditionally-added keys
- Serialize with `facet_yaml::to_string()`, prepend `#cloud-config\n`
- Also fixes the autologin bootcmd issue (proper YAML multiline strings instead of shell heredocs)
