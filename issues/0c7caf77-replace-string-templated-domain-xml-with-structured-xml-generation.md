# Replace string-templated domain XML with structured XML generation

**ID:** 0c7caf77 | **Status:** Done | **Created:** 2026-02-14T16:05:47+01:00

`domain_xml.rs` now uses facet-xml struct-based serialization instead of `writeln!` string concatenation. Rust structs model the libvirt domain XML schema, and `facet_xml::to_string()` serializes them.

## Caveats (facet-xml v0.43)

- **Compact output only** — pretty-print corrupts text nodes. Tracked: [facet#1982](https://github.com/facet-rs/facet/issues/1982)
- **No self-closing tags** — `<boot dev="hd"></boot>` instead of `<boot dev="hd"/>`. Libvirt accepts both.
- **`#[facet(flatten)]` is broken** for enum variants — double-wraps elements. Avoided by using separate structs.
