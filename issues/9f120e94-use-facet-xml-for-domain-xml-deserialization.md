# Use facet-xml for domain XML deserialization

**ID:** 9f120e94 | **Status:** Done | **Created:** 2026-02-27T17:26:25+01:00

## Summary

Domain XML generation already uses facet-xml properly (`domain_xml.rs`, `network_xml.rs`),
but parsing live domain XML returned by libvirt is done with hand-rolled string search.
This should use facet-xml deserialization for type safety and correctness.

## Current problem

`parse_vsock_cid()` in both `backend/libvirt.rs:896` and `workers.rs:340` uses raw string
manipulation (`xml.find("<vsock")`, `find("address=\"")`) to extract the auto-assigned vsock
CID from the live domain XML. This is fragile — it could break on whitespace differences,
attribute ordering changes, or namespace prefixes.

```rust
// Current approach (fragile):
fn parse_vsock_cid(dom: &Domain) -> Option<u32> {
    let xml = dom.get_xml_desc(0).ok()?;
    let vsock_start = xml.find("<vsock")?;
    let vsock_section = &xml[vsock_start..vsock_end];
    let addr_prefix = "address=\"";
    // ... manual string slicing
}
```

## Approach

1. Define a subset of domain XML structs with `#[derive(Facet)]` that can deserialize the
   portions we care about (vsock CID, and potentially IP addresses, device info, etc.)
2. Use `facet_xml::from_str()` to parse the live XML returned by `dom.get_xml_desc(0)`
3. Replace `parse_vsock_cid()` with a proper typed accessor
4. Audit for other hand-rolled XML parsing that could benefit (e.g. any future need to
   read MAC addresses, disk paths, or network info from live domain XML)

## Files

- `src/backend/libvirt.rs` — `parse_vsock_cid()` (line 896)
- `src/workers.rs` — duplicated `parse_vsock_cid()` (line 340)
- `src/domain_xml.rs` — already has the generation structs; deserialization structs
  could live here or in a new `src/domain_xml_parse.rs`

## Notes

- `facet_xml::from_str()` needs to handle unknown/extra elements gracefully since the
  live XML from libvirt contains many elements not in our generation structs
- The deserialization structs don't need to cover the full domain XML — just the subset
  we need to extract (vsock CID for now, potentially more later)
- Both `backend/libvirt.rs` and `workers.rs` have identical copies of `parse_vsock_cid` —
  should be deduplicated regardless of the parsing approach
