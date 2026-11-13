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

use crate::measurement::{BASELINE, Measurement};
use crate::util::{AlignedBuf, from_wide};

const ADDRESS_REFRESH: Duration = Duration::from_secs(10);
const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_LOOPBACK: u32 = 24;
const IF_TYPE_WIFI: u32 = 71;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NicInfo {
    /// Stable interface LUID, as a string.
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
    pub rate_state: Measurement,
}

pub(crate) struct NicTracker {
    prev: HashMap<u64, (u64, u64)>,
    infos: HashMap<u64, Arc<NicInfo>>,
    addresses: HashMap<u64, (Vec<String>, Vec<String>)>,
    addresses_at: Option<Instant>,
    last: Option<Instant>,
    pub state: Measurement,
}

impl NicTracker {
    pub fn new() -> Self {
        Self {
            prev: HashMap::new(),
            infos: HashMap::new(),
            addresses: HashMap::new(),
            addresses_at: None,
            last: None,
            state: Measurement::default(),
        }
    }

    pub fn sample(&mut self) -> Vec<NicStats> {
        let now = Instant::now();
        if self
            .addresses_at
            .is_none_or(|t| t.elapsed() > ADDRESS_REFRESH)
        {
            self.addresses = adapter_addresses();
            self.addresses_at = Some(now);
            self.infos.clear(); // rebuilt below with fresh addresses
        }

        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        let status = unsafe { GetIfTable2(&mut table) };
        if status.is_err() || table.is_null() {
            self.unavailable(status.0);
            return Vec::new();
        }
        let rows = unsafe {
            std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize)
        };
        let out = self.sample_rows(rows, now);
        unsafe { FreeMibTable(table.cast()) };
        out
    }

    fn unavailable(&mut self, status: u32) {
        self.state = Measurement::failure(status);
        self.prev.clear();
        self.last = None;
    }

    fn sample_rows(&mut self, rows: &[MIB_IF_ROW2], now: Instant) -> Vec<NicStats> {
        let secs = self.last.map(|t| now.duration_since(t).as_secs_f64());
        self.last = Some(now);

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
            let rates = self
                .prev
                .get(&luid)
                .zip(secs)
                .and_then(|(&(pin, pout), s)| {
                    (s > 0.0).then_some(s).and_then(|s| {
                        Some((
                            (row.InOctets.checked_sub(pin)? as f64 / s) as u64,
                            (row.OutOctets.checked_sub(pout)? as f64 / s) as u64,
                        ))
                    })
                });
            let (rx, tx) = rates.unwrap_or((0, 0));
            let rate_state = rates.map_or(Measurement::missing(BASELINE), |_| Measurement::ready());
            next_prev.insert(luid, (row.InOctets, row.OutOctets));
            out.push(NicStats {
                info,
                rx,
                tx,
                rate_state,
            });
        }
        self.prev = next_prev;
        self.state = if out.is_empty() {
            Measurement::ready()
        } else {
            Measurement::combine(out.iter().map(|n| n.rate_state))
        };

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
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    adapter_addresses_with_query(|buffer, size| unsafe {
        GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(buffer), size)
    })
}

fn adapter_addresses_with_query(
    mut query: impl FnMut(*mut IP_ADAPTER_ADDRESSES_LH, &mut u32) -> u32,
) -> HashMap<u64, (Vec<String>, Vec<String>)> {
    let mut map = HashMap::new();
    let mut size = 16 * 1024u32;
    let mut buf = AlignedBuf::with_capacity(size as usize);
    let mut completed = false;
    for _ in 0..3 {
        let err = query(buf.as_mut_ptr().cast(), &mut size);
        match err {
            0 => {
                completed = true;
                break;
            }
            111 => buf.grow_to(size as usize), // ERROR_BUFFER_OVERFLOW
            _ => return map,
        }
    }
    // Failed calls do not provide a valid linked list, even after resizing.
    if !completed {
        crate::diagnostics::warn("network.addresses", crate::diagnostics::code(111));
        return map;
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::NetworkManagement::IpHelper::IP_ADAPTER_ADDRESSES_LH;

    #[test]
    fn exhausted_retries_never_consume_failed_output() {
        let mut calls = 0;
        let addresses = adapter_addresses_with_query(|buffer, size| {
            calls += 1;
            // The error-path contents are deliberately plausible, but invalid.
            unsafe {
                *buffer = IP_ADAPTER_ADDRESSES_LH::default();
                (*buffer).Luid.Value = 123;
            }
            *size *= 2;
            111
        });
        assert_eq!(calls, 3);
        assert!(addresses.is_empty());
    }

    #[test]
    fn successful_retry_uses_only_the_completed_response() {
        let mut calls = 0;
        let addresses = adapter_addresses_with_query(|buffer, size| {
            calls += 1;
            unsafe {
                *buffer = IP_ADAPTER_ADDRESSES_LH::default();
                (*buffer).Luid.Value = if calls == 1 { 123 } else { 456 };
            }
            if calls == 1 {
                *size *= 2;
                111
            } else {
                0
            }
        });
        assert_eq!(calls, 2);
        assert_eq!(addresses.len(), 1);
        assert!(addresses.contains_key(&456));
    }

    #[test]
    fn other_errors_discard_the_output_immediately() {
        let mut calls = 0;
        let addresses = adapter_addresses_with_query(|buffer, _| {
            calls += 1;
            unsafe {
                *buffer = IP_ADAPTER_ADDRESSES_LH::default();
                (*buffer).Luid.Value = 123;
            }
            5
        });
        assert_eq!(calls, 1);
        assert!(addresses.is_empty());
    }
    #[test]
    fn failed_table_resets_baseline_and_recovery_does_not_double_rates() {
        let now = Instant::now();
        let row = |bytes| {
            let mut row = MIB_IF_ROW2::default();
            row.InterfaceLuid.Value = 1;
            row.InterfaceAndOperStatusFlags._bitfield = 1;
            row.OperStatus = IfOperStatusUp;
            row.Type = IF_TYPE_ETHERNET;
            row.InOctets = bytes;
            row
        };
        let mut tracker = NicTracker::new();
        assert!(!tracker.sample_rows(&[row(100)], now)[0].rate_state.valid);
        assert_eq!(
            tracker.sample_rows(&[row(200)], now + Duration::from_secs(1))[0].rx,
            100
        );
        tracker.unavailable(5);
        assert_eq!(tracker.state.missing, crate::measurement::ACCESS_DENIED);
        let recovered = tracker.sample_rows(&[row(400)], now + Duration::from_secs(3));
        assert!(!recovered[0].rate_state.valid);
        assert_eq!(recovered[0].rate_state.missing, BASELINE);
        let fresh = tracker.sample_rows(&[row(500)], now + Duration::from_secs(4));
        assert_eq!(fresh[0].rx, 100);
        assert_eq!(fresh[0].rate_state, Measurement::ready());
        let reset = tracker.sample_rows(&[row(1)], now + Duration::from_secs(5));
        assert!(!reset[0].rate_state.valid);
        assert_eq!(
            tracker.sample_rows(&[], now + Duration::from_secs(6)).len(),
            0
        );
        assert_eq!(tracker.state, Measurement::ready());
    }
}
