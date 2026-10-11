//! Network adapters: throughput from the interface table, addresses from
//! GetAdaptersAddresses.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
    GetAdaptersAddresses, GetIfTable2, IP_ADAPTER_ADDRESSES_LH, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC};

use crate::util::{AlignedBuf, from_wide};

const ADDRESS_REFRESH: Duration = Duration::from_secs(10);
const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_LOOPBACK: u32 = 24;
const IF_TYPE_WIFI: u32 = 71;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NicInfo {
    /// Stable interface LUID, as a string (JS numbers cannot hold it).
    pub id: String,
    /// User-visible connection name, e.g. `以太网`, `WLAN`.
    pub name: String,
    /// Adapter model, e.g. `Intel(R) Wi-Fi 6E AX211`.
    pub description: String,
    /// `ethernet` | `wifi` | `virtual` | `other`
    pub kind: &'static str,
    /// Link speed in bits per second.
    pub link_speed: u64,
    pub mac: Option<String>,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NicStats {
    pub info: Arc<NicInfo>,
    /// Bytes per second.
    pub rx: u64,
    pub tx: u64,
}

pub(crate) struct NicTracker {
    prev: HashMap<u64, (u64, u64)>,
    infos: HashMap<u64, Arc<NicInfo>>,
    addresses: HashMap<u64, (Vec<String>, Vec<String>)>,
    addresses_at: Option<Instant>,
    last: Option<Instant>,
}

impl NicTracker {
    pub fn new() -> Self {
        Self {
            prev: HashMap::new(),
            infos: HashMap::new(),
            addresses: HashMap::new(),
            addresses_at: None,
            last: None,
        }
    }

    pub fn sample(&mut self) -> Vec<NicStats> {
        let now = Instant::now();
        let secs = self.last.map(|t| now.duration_since(t).as_secs_f64());
        self.last = Some(now);

        if self
            .addresses_at
            .is_none_or(|t| t.elapsed() > ADDRESS_REFRESH)
        {
            self.addresses = adapter_addresses();
            self.addresses_at = Some(now);
            self.infos.clear(); // rebuilt below with fresh addresses
        }

        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if unsafe { GetIfTable2(&mut table) }.is_err() || table.is_null() {
            return Vec::new();
        }
        let rows = unsafe {
            std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize)
        };

        let mut out = Vec::new();
        let mut next_prev = HashMap::new();
        for row in rows {
            if !wanted(row) {
                continue;
            }
            let luid = unsafe { row.InterfaceLuid.Value };
            let info = self
                .infos
                .entry(luid)
                .or_insert_with(|| Arc::new(describe(row, self.addresses.get(&luid))))
                .clone();
            let (rx, tx) = match (self.prev.get(&luid), secs) {
                (Some(&(pin, pout)), Some(s)) if s > 0.0 => (
                    (row.InOctets.saturating_sub(pin) as f64 / s) as u64,
                    (row.OutOctets.saturating_sub(pout) as f64 / s) as u64,
                ),
                _ => (0, 0),
            };
            next_prev.insert(luid, (row.InOctets, row.OutOctets));
            out.push(NicStats { info, rx, tx });
        }
        unsafe { FreeMibTable(table.cast()) };
        self.prev = next_prev;

        // Physical adapters first, busiest-capable first.
        out.sort_by_key(|n| {
            (
                n.info.kind == "virtual",
                std::cmp::Reverse(n.info.link_speed),
            )
        });
        out
    }
}

/// Connected, non-loopback interfaces, excluding the per-filter-driver
/// duplicates Windows lists for every adapter.
fn wanted(row: &MIB_IF_ROW2) -> bool {
    let flags = row.InterfaceAndOperStatusFlags._bitfield;
    let hardware = flags & 0x01 != 0;
    let filter = flags & 0x02 != 0;
    if filter || row.Type == IF_TYPE_LOOPBACK || row.OperStatus != IfOperStatusUp {
        return false;
    }
    hardware || row.InOctets + row.OutOctets > 0
}

fn describe(row: &MIB_IF_ROW2, addrs: Option<&(Vec<String>, Vec<String>)>) -> NicInfo {
    let hardware = row.InterfaceAndOperStatusFlags._bitfield & 0x01 != 0;
    let mac_len = (row.PhysicalAddressLength as usize).min(row.PhysicalAddress.len());
    let mac = (mac_len == 6).then(|| {
        row.PhysicalAddress[..6]
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join("-")
    });
    let (ipv4, ipv6) = addrs.cloned().unwrap_or_default();
    NicInfo {
        id: unsafe { row.InterfaceLuid.Value }.to_string(),
        name: from_wide(&row.Alias),
        description: from_wide(&row.Description),
        kind: match row.Type {
            _ if !hardware => "virtual",
            IF_TYPE_ETHERNET => "ethernet",
            IF_TYPE_WIFI => "wifi",
            _ => "other",
        },
        link_speed: row.ReceiveLinkSpeed.max(row.TransmitLinkSpeed),
        mac,
        ipv4,
        ipv6,
    }
}

/// Unicast addresses per interface LUID.
fn adapter_addresses() -> HashMap<u64, (Vec<String>, Vec<String>)> {
    let mut map = HashMap::new();
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size = 16 * 1024u32;
    let mut buf = AlignedBuf::with_capacity(size as usize);
    for _ in 0..3 {
        let err = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                flags,
                None,
                Some(buf.as_mut_ptr().cast()),
                &mut size,
            )
        };
        match err {
            0 => break,
            111 => buf.grow_to(size as usize), // ERROR_BUFFER_OVERFLOW
            _ => return map,
        }
    }

    let mut cur = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !cur.is_null() {
        let a = unsafe { &*cur };
        let (mut v4, mut v6) = (Vec::new(), Vec::new());
        let mut u = a.FirstUnicastAddress;
        while !u.is_null() {
            let ua = unsafe { &*u };
            let sa = ua.Address.lpSockaddr;
            let len = ua.Address.iSockaddrLength as usize;
            if !sa.is_null() && len >= 8 {
                let bytes = unsafe { std::slice::from_raw_parts(sa as *const u8, len) };
                let family = u16::from_le_bytes([bytes[0], bytes[1]]);
                if family == AF_INET.0 && len >= 8 {
                    v4.push(Ipv4Addr::new(bytes[4], bytes[5], bytes[6], bytes[7]).to_string());
                } else if family == AF_INET6.0 && len >= 24 {
                    let mut octets = [0u8; 16];
                    octets.copy_from_slice(&bytes[8..24]);
                    let ip = Ipv6Addr::from(octets);
                    // Link-local addresses are noise for most users; keep global ones.
                    if !ip.is_unicast_link_local() {
                        v6.push(ip.to_string());
                    }
                }
            }
            u = ua.Next;
        }
        map.insert(unsafe { a.Luid.Value }, (v4, v6));
        cur = a.Next;
    }
    map
}
