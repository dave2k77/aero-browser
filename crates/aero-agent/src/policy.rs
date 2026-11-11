//! E01 baseline (plan §7.4): which navigation targets and export names are
//! acceptable at all, before any grant or approval is consulted.
//!
//! A target that fails here is refused outright; passing says nothing about
//! permission (that is `capabilities`). Limits: host names are not resolved, so
//! DNS rebinding, redirects to private addresses and browser subrequests are
//! not covered; full egress control needs a network namespace or proxy.

use std::net::{Ipv4Addr, Ipv6Addr};
use url::{Host, Url};

/// Why a target is refused. Static text; the URL is never echoed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsafeTarget {
    Invalid,
    /// Not `http`/`https`, or no host.
    Scheme,
    /// A user name or password in the URL.
    Credentials,
    /// Loopback, private, link-local, shared or otherwise non-public address.
    PrivateAddress,
    /// `localhost` or a `.localhost` name.
    Localhost,
}

/// Parse `raw` and accept it only as a public HTTP(S) navigation target.
/// `allow_private_network` admits loopback and private hosts (test fixtures,
/// or an explicit local-network permission).
pub fn allowed_navigation(raw: &str, allow_private_network: bool) -> Result<Url, UnsafeTarget> {
    let url = Url::parse(raw).map_err(|_| UnsafeTarget::Invalid)?;
    allowed_target(&url, allow_private_network)?;
    Ok(url)
}

/// [`allowed_navigation`] for an already parsed URL.
pub fn allowed_target(url: &Url, allow_private_network: bool) -> Result<(), UnsafeTarget> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(UnsafeTarget::Scheme);
    }
    let host = url.host().ok_or(UnsafeTarget::Scheme)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UnsafeTarget::Credentials);
    }
    if allow_private_network {
        return Ok(());
    }
    match host {
        Host::Ipv4(ip) if non_public_v4(ip) => Err(UnsafeTarget::PrivateAddress),
        Host::Ipv6(ip) if non_public_v6(ip) => Err(UnsafeTarget::PrivateAddress),
        Host::Domain(name) => {
            let name = name.strip_suffix('.').unwrap_or(name);
            if name == "localhost" || name.ends_with(".localhost") {
                Err(UnsafeTarget::Localhost)
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

/// A generated-file display name: 1–128 ASCII letters, digits, `.`, `-`,
/// `_`, and not `.` or `..`. Not a race-free filesystem boundary by itself.
pub fn safe_export_basename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// Not a public unicast address: this network, private, shared (carrier-grade
/// NAT), loopback, link-local (including cloud metadata), IETF protocol
/// assignments, documentation, benchmarking, multicast, reserved, broadcast.
fn non_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    a == 0
        || ip.is_private()
        || (a == 100 && (64..128).contains(&b))
        || ip.is_loopback()
        || ip.is_link_local()
        || (a == 192 && b == 0 && c == 0)
        || ip.is_documentation()
        || (a == 198 && (18..20).contains(&b))
        || ip.is_multicast()
        || a >= 240
}

/// Not a public unicast address, judging IPv4 embedded in mapped, compatible,
/// NAT64 and 6to4 forms by the IPv4 rules.
fn non_public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    let embedded = |high: u16, low: u16| {
        non_public_v4(Ipv4Addr::from((u32::from(high) << 16) | u32::from(low)))
    };
    if let Some(v4) = ip.to_ipv4_mapped() {
        return non_public_v4(v4);
    }
    match s {
        // Unspecified, loopback and deprecated IPv4-compatible (::/96).
        [0, 0, 0, 0, 0, 0, _, _] => true,
        // NAT64 well-known prefix 64:ff9b::/96.
        [0x64, 0xff9b, 0, 0, 0, 0, high, low] => embedded(high, low),
        // Local-use NAT64 64:ff9b:1::/48.
        [0x64, 0xff9b, 1, ..] => true,
        // 6to4 2002::/16 carries an IPv4 address in bits 16–47.
        [0x2002, high, low, ..] => embedded(high, low),
        // Teredo 2001::/32 and documentation 2001:db8::/32.
        [0x2001, 0, ..] | [0x2001, 0xdb8, ..] => true,
        _ => {
            let first = s[0];
            (first & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (first & 0xffc0) == 0xfec0 // deprecated site-local fec0::/10
                || (first & 0xff00) == 0xff00 // multicast
        }
    }
}
