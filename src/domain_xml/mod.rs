//! Libvirt domain XML generation using facet-xml struct serialization.
//!
//! # Caveats (facet-xml v0.43)
//!
//! - **Compact output only.** Pretty-print (`to_string_pretty`) corrupts text
//!   nodes by inserting whitespace inside `<name>`, `<memory>`, etc.
//!   Tracked upstream: <https://github.com/facet-rs/facet/issues/1982>
//! - **No self-closing tags.** Attribute-only elements like `<boot dev="hd">`
//!   render as `<boot dev="hd"></boot>` instead of `<boot dev="hd"/>`.
//!   Libvirt accepts both forms, so this is cosmetic only.
//! - **`#[facet(flatten)]` is broken** for enum variants — double-wraps
//!   elements. Avoid for now; use separate struct fields instead.

mod model;
mod build;
mod support;

#[cfg(test)]
mod tests;

pub use build::generate_domain_xml;
pub use support::{generate_mac, parse_vsock_cid, xml_has_changed};
