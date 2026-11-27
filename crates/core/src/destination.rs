//! Per-request socket observations. IP registration, product policy and HTTP egress
//! are independent. The optional address book is local and never performs a lookup online.
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;

#[derive(Clone, Debug, Default)]
pub struct AddressBook(HashMap<IpNet, (String, u32)>);

impl AddressBook {
    /// One CIDR,country,ASN per line. More specific prefixes take precedence.
    pub fn parse(input: &str) -> Result<Self, &'static str> {
        let mut rows = HashMap::new();
        for line in input
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let mut fields = line.split(',').map(str::trim);
            let prefix: IpNet = fields
                .next()
                .and_then(|v| v.parse().ok())
                .ok_or("invalid CIDR")?;
            let country = fields
                .next()
                .and_then(crate::normalize_country)
                .ok_or("invalid country")?;
            let asn = fields
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .filter(|n| *n != 0)
                .ok_or("invalid ASN")?;
            if fields.next().is_some() || rows.insert(prefix.trunc(), (country, asn)).is_some() {
                return Err("invalid or duplicate address row");
            }
        }
        Ok(Self(rows))
    }
    fn get(&self, ip: IpAddr) -> Option<&(String, u32)> {
        if self.0.is_empty() {
            return None;
        }
        let ip = ip.to_canonical();
        (0..=if ip.is_ipv4() { 32 } else { 128 })
            .rev()
            .find_map(|len| self.0.get(&IpNet::new(ip, len).ok()?.trunc()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PeerRole {
    DirectSocket,
    ProxyNode,
    #[default]
    NotObserved,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Comparison {
    Match,
    Mismatch,
    ProxyNotComparable,
    #[default]
    NotObserved,
}

/// Default wire view has counts and labels only. Raw observations stay in memory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Destination {
    pub peer_role: PeerRole,
    pub resolved_count: usize,
    pub resolved_v6: usize,
    pub peer_observed: bool,
    pub resolved_addresses: Vec<IpAddr>,
    pub peer_address: Option<IpAddr>,
    pub address_consistency: Comparison,
    pub resolved_countries: BTreeSet<String>,
    pub peer_country: Option<String>,
    pub peer_asn: Option<u32>,
    pub expected_country: Option<String>,
    pub region_consistency: Comparison,
    #[serde(skip)]
    raw_resolved: Vec<IpAddr>,
    #[serde(skip)]
    raw_peer: Option<IpAddr>,
}
impl Destination {
    pub fn observe(
        mut resolved: Vec<IpAddr>,
        peer: Option<IpAddr>,
        proxy: bool,
        book: &AddressBook,
        expected: Option<&str>,
    ) -> Self {
        for ip in &mut resolved {
            *ip = ip.to_canonical();
        }
        resolved.sort_unstable();
        resolved.dedup();
        let peer = peer.map(|ip| ip.to_canonical());
        let compare = |matches| {
            if matches {
                Comparison::Match
            } else {
                Comparison::Mismatch
            }
        };
        let info = peer.and_then(|ip| book.get(ip));
        let expected_country = expected.and_then(crate::normalize_country);
        Self {
            peer_role: if peer.is_none() {
                PeerRole::NotObserved
            } else if proxy {
                PeerRole::ProxyNode
            } else {
                PeerRole::DirectSocket
            },
            resolved_count: resolved.len(),
            resolved_v6: resolved.iter().filter(|ip| ip.is_ipv6()).count(),
            peer_observed: peer.is_some(),
            address_consistency: if proxy {
                Comparison::ProxyNotComparable
            } else if let Some(ip) = peer.filter(|_| !resolved.is_empty()) {
                compare(resolved.contains(&ip))
            } else {
                Comparison::NotObserved
            },
            resolved_countries: resolved
                .iter()
                .filter_map(|ip| book.get(*ip).map(|v| v.0.clone()))
                .collect(),
            peer_country: info.map(|v| v.0.clone()),
            peer_asn: info.map(|v| v.1),
            region_consistency: if proxy {
                Comparison::ProxyNotComparable
            } else if let (Some((actual, _)), Some(wanted)) = (info, &expected_country) {
                compare(actual == wanted)
            } else {
                Comparison::NotObserved
            },
            expected_country,
            raw_resolved: resolved,
            raw_peer: peer,
            ..Default::default()
        }
    }
    /// Call on a display/export copy, after explicit disclosure. Never reconstruct missing IPs.
    pub fn set_show_ip(&mut self, show: bool) {
        self.resolved_addresses = if show {
            self.raw_resolved.clone()
        } else {
            Vec::new()
        };
        self.peer_address = if show { self.raw_peer } else { None };
    }
    pub fn has_observation(&self) -> bool {
        self.peer_observed || self.resolved_count > 0
    }
    pub fn has_risk_markers(&self) -> bool {
        self.address_consistency == Comparison::Mismatch
            || self.region_consistency == Comparison::Mismatch
    }
    pub fn markers(&self) -> impl Iterator<Item = &'static str> {
        [
            (self.address_consistency, "ADDRESS_MISMATCH"),
            (self.region_consistency, "UNEXPECTED_REGION"),
        ]
        .into_iter()
        .filter_map(|(c, m)| (c == Comparison::Mismatch).then_some(m))
    }
}
