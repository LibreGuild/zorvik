//! This computer's network interfaces and addresses, e.g. to reach a local
//! server from a phone on the same network.

use std::collections::BTreeMap;
use std::net::IpAddr;

use serde::Serialize;
use ts_rs::TS;

use crate::error::{EngineError, ErrorKind, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum IpFamily {
    Ipv4,
    Ipv6,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetAddress {
    pub ip: String,
    /// Network prefix length (e.g. 24 for 255.255.255.0).
    pub prefix: Option<u8>,
    pub family: IpFamily,
    /// Only usable on the local link (169.254.x.x, fe80::).
    pub link_local: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetInterface {
    pub name: String,
    /// IPv4 first, then IPv6; link-local addresses last.
    pub addresses: Vec<NetAddress>,
    pub loopback: bool,
    /// The interface is up (connected).
    pub up: bool,
}

/// All interfaces with at least one address: usable ones first (up, not
/// loopback, with a routable IPv4 address), loopback last.
pub fn list_interfaces() -> Result<Vec<NetInterface>> {
    let raw = if_addrs::get_if_addrs()
        .map_err(|e| EngineError::new(ErrorKind::Io, format!("Could not list network interfaces: {e}")))?;
    Ok(group(raw.into_iter().map(|i| {
        let (ip, prefix) = match &i.addr {
            if_addrs::IfAddr::V4(a) => (IpAddr::V4(a.ip), a.prefixlen),
            if_addrs::IfAddr::V6(a) => (IpAddr::V6(a.ip), a.prefixlen),
        };
        (i.name.clone(), ip, prefix, i.is_oper_up())
    })))
}

/// Group `(interface, ip, prefix, up)` rows by interface and sort them.
fn group(rows: impl Iterator<Item = (String, IpAddr, u8, bool)>) -> Vec<NetInterface> {
    let mut by_name: BTreeMap<String, NetInterface> = BTreeMap::new();
    for (name, ip, prefix, up) in rows {
        let entry = by_name.entry(name.clone()).or_insert_with(|| NetInterface {
            name,
            addresses: Vec::new(),
            loopback: true,
            up: false,
        });
        entry.loopback &= ip.is_loopback();
        entry.up |= up;
        let address = NetAddress {
            ip: ip.to_string(),
            prefix: Some(prefix),
            family: if ip.is_ipv4() { IpFamily::Ipv4 } else { IpFamily::Ipv6 },
            link_local: is_link_local(&ip),
        };
        if !entry.addresses.iter().any(|a| a.ip == address.ip) {
            entry.addresses.push(address);
        }
    }
    let mut list: Vec<NetInterface> = by_name.into_values().collect();
    for i in &mut list {
        i.addresses.sort_by_key(|a| (a.link_local, a.family == IpFamily::Ipv6));
    }
    list.sort_by_key(|i| {
        let routable_v4 = i.addresses.iter().any(|a| a.family == IpFamily::Ipv4 && !a.link_local);
        (i.loopback, !i.up, !routable_v4)
    });
    list
}

fn is_link_local(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_and_sorts_interfaces() {
        let rows = vec![
            ("lo0".to_string(), "127.0.0.1".parse().unwrap(), 8, true),
            ("lo0".to_string(), "::1".parse().unwrap(), 128, true),
            ("en0".to_string(), "fe80::1".parse().unwrap(), 64, true),
            ("en0".to_string(), "192.168.1.20".parse().unwrap(), 24, true),
            ("en0".to_string(), "2001:db8::5".parse().unwrap(), 64, true),
            ("awdl0".to_string(), "fe80::2".parse().unwrap(), 64, true),
            ("en5".to_string(), "10.0.0.3".parse().unwrap(), 8, false),
        ];
        let list = group(rows.into_iter());
        let names: Vec<&str> = list.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["en0", "awdl0", "en5", "lo0"]);
        let en0: Vec<&str> = list[0].addresses.iter().map(|a| a.ip.as_str()).collect();
        assert_eq!(en0, ["192.168.1.20", "2001:db8::5", "fe80::1"]);
        assert!(list[0].addresses[2].link_local);
        assert!(list[3].loopback && !list[0].loopback);
        assert!(!list[2].up);
    }
}
