//! Libvirt network XML generation using facet-xml struct serialization.

use facet::Facet;
use facet_xml as xml;

// ── XML model structs ──────────────────────────────────────

#[derive(Debug, Facet)]
#[facet(rename = "network")]
struct NetworkDef {
    name: String,
    ip: NetworkIp,
}

#[derive(Debug, Facet)]
struct NetworkIp {
    #[facet(xml::attribute)]
    address: String,
    #[facet(xml::attribute)]
    netmask: String,
    dhcp: NetworkDhcp,
}

#[derive(Debug, Facet)]
struct NetworkDhcp {
    range: DhcpRange,
}

#[derive(Debug, Facet)]
struct DhcpRange {
    #[facet(xml::attribute)]
    start: String,
    #[facet(xml::attribute)]
    end: String,
}

// ── naming ─────────────────────────────────────────────────

/// Build the libvirt network name from the VM's config id and the interface network name.
/// E.g. `rum-a1b2c3d4-hostonly`
pub fn prefixed_name(id: &str, config_network: &str) -> String {
    format!("rum-{id}-{config_network}")
}

// ── public API ─────────────────────────────────────────────

/// Generate libvirt network XML for a host-only network with DHCP.
pub fn generate_network_xml(name: &str, subnet: &str) -> String {
    let net = NetworkDef {
        name: name.into(),
        ip: NetworkIp {
            address: format!("{subnet}.1"),
            netmask: "255.255.255.0".into(),
            dhcp: NetworkDhcp {
                range: DhcpRange {
                    start: format!("{subnet}.100"),
                    end: format!("{subnet}.254"),
                },
            },
        },
    };

    facet_xml::to_string(&net).expect("network XML serialization should not fail")
}

/// Derive a /24 subnet prefix (first 3 octets) for a host-only network.
///
/// If an IP hint is provided (e.g. "192.168.50.10"), uses its first 3 octets.
/// Otherwise, generates `192.168.<hash>` from the network name.
pub fn derive_subnet(name: &str, ip_hint: &str) -> String {
    if !ip_hint.is_empty()
        && let Some((prefix, _)) = ip_hint.rsplit_once('.')
    {
        return prefix.to_string();
    }
    // Hash-based: pick a third octet from 2..254 based on network name
    let mut hash: u32 = 5381;
    for b in name.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u32);
    }
    let octet = (hash % 253) + 2; // 2..254
    format!("192.168.{octet}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_xml_has_name_and_dhcp() {
        let xml = generate_network_xml("rum-hostonly", "192.168.50");
        assert!(xml.contains("<name>rum-hostonly</name>"));
        assert!(xml.contains(r#"address="192.168.50.1""#));
        assert!(xml.contains(r#"start="192.168.50.100""#));
        assert!(xml.contains(r#"end="192.168.50.254""#));
    }

    #[test]
    fn derive_subnet_from_ip_hint() {
        assert_eq!(derive_subnet("net", "192.168.50.10"), "192.168.50");
        assert_eq!(derive_subnet("net", "10.0.0.5"), "10.0.0");
    }

    #[test]
    fn derive_subnet_without_hint_is_deterministic() {
        let s1 = derive_subnet("rum-hostonly", "");
        let s2 = derive_subnet("rum-hostonly", "");
        assert_eq!(s1, s2);
        assert!(s1.starts_with("192.168."));
    }

    #[test]
    fn derive_subnet_without_hint_differs_by_name() {
        let s1 = derive_subnet("net-a", "");
        let s2 = derive_subnet("net-b", "");
        assert_ne!(s1, s2);
    }
}
