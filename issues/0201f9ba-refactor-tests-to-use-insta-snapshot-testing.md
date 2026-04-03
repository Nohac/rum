# Refactor tests to use insta snapshot testing

**ID:** 0201f9ba | **Status:** Open | **Created:** 2026-02-27T17:27:08+01:00

## Summary

Replace most manual `assert_eq!` / `assert!` checks with `insta` snapshot tests. This
makes tests more maintainable — updating expected output is a single `cargo insta review`
instead of editing dozens of string literals. Also makes test failures much easier to read
(unified diff of expected vs actual).

## Scope

16 test modules with ~450 assertions across 182 tests. Best candidates for snapshot testing:

### High value (string/structured output)

- **`domain_xml.rs`** (27 assertions) — XML generation tests currently check substrings.
  Snapshot the full XML output instead. Changes to XML format are immediately visible.
- **`cloudinit.rs`** (60 assertions) — user-data YAML content checks. Snapshot the entire
  generated cloud-config. Covers user-data, meta-data, network-config.
- **`init.rs`** (35 assertions) — generated TOML content. Snapshot the full TOML output
  for each variant (default, with mounts, with drives, etc.)
- **`network_xml.rs`** (8 assertions) — network XML and subnet derivation
- **`iso9660.rs`** (30 assertions) — ISO binary structure checks. Some assertions on
  magic bytes / offsets may be better left as explicit asserts; the content/structure
  checks could be snapshots.

### Medium value (enum/state transitions)

- **`flow/*.rs`** (173 assertions across 7 files) — transition tests check `(state, effects)`.
  Could snapshot the full transition trace for happy-path tests. Individual transition
  assertions might stay as explicit asserts for clarity.
- **`config.rs`** (63 assertions) — config parsing/validation. Error messages and parsed
  struct values are good snapshot candidates.

### Lower value (keep as-is)

- **`util.rs`** (7 assertions) — simple parse_size tests, explicit asserts are clearer
- **`qcow2.rs`** (15 assertions) — binary format checks at specific offsets
- **`tests/cli.rs`** (5 assertions) — integration tests using `assert_cmd`, different pattern

## Approach

1. Add `insta` as a dev-dependency in `Cargo.toml`
2. Start with the highest-value modules: `domain_xml`, `cloudinit`, `init`
3. For each test: replace multi-line assert chains with `insta::assert_snapshot!()` or
   `insta::assert_yaml_snapshot!()` / `insta::assert_debug_snapshot!()`
4. Run `cargo insta review` to accept initial snapshots
5. Snapshot files go in `src/snapshots/` (insta default)

## Example

Before:
```rust
#[test]
fn xml_from_minimal_toml_has_defaults() {
    let xml = generate_domain_xml(&config, "test", &[]);
    assert!(xml.contains("<name>test</name>"));
    assert!(xml.contains("<vcpu>2</vcpu>"));
    assert!(xml.contains("<memory unit='MiB'>2048</memory>"));
    assert!(xml.contains("<type>kvm</type>"));
}
```

After:
```rust
#[test]
fn xml_from_minimal_toml_has_defaults() {
    let xml = generate_domain_xml(&config, "test", &[]);
    insta::assert_snapshot!(xml);
}
```

The snapshot file captures the entire XML, and any change shows up as a clear diff.
