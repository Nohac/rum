# Replace string-templated domain XML with structured XML generation

**ID:** 0c7caf77 | **Status:** Open | **Created:** 2026-02-14T16:05:47+01:00

`domain_xml.rs` currently builds libvirt domain XML via `writeln!` string concatenation. This is fragile — no escaping of special characters in values, hard to read, easy to produce malformed XML.

## Options to investigate

- **`xml-builder`** / **`xmlwriter`** — lightweight XML writing crates, builder-pattern API
- **`quick-xml`** — popular, supports both reading and writing, serde integration
- **Inline XML macros** — crates like `html!` / `maud` style but for XML; unclear if any are mature for generic XML
- **facet-xml** ([facet-rs/facet-xml](https://github.com/facet-rs/facet-xml)) — XML backend for facet, already used in this project for TOML/YAML
- **Struct-based approach** — define Rust structs matching the libvirt domain schema, serialize with quick-xml + serde (or facet if an XML backend appears)

## Approach

1. Research crate options — prioritize minimal dependencies and good ergonomics
2. Pick one that gives type-safe or at least properly-escaped XML generation
3. Rewrite `generate_domain_xml()` to use the chosen approach
4. Ensure existing tests still pass (output doesn't need to be identical, just semantically correct)
5. The `xml_has_changed()` comparison may need updating if whitespace/formatting changes
