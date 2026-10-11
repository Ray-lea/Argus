//! TCP and UDP endpoints with their owning process (what `netstat -ano` shows).

use std::net::{Ipv4Addr, Ipv6Addr};

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP_STATE_DELETE_TCB, MIB_TCP6ROW_OWNER_PID,
    MIB_TCPROW_LH, MIB_TCPROW_LH_0, MIB_TCPROW_OWNER_PID, MIB_UDP6ROW_OWNER_PID,
    MIB_UDPROW_OWNER_PID, SetTcpEntry, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

use crate::actions::ActionError;
use crate::util::AlignedBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    /// `tcp` | `udp`
    pub protocol: &'static str,
    pub ipv6: bool,
    pub local_address: String,
    pub local_port: u16,
    /// None for listening TCP sockets and for UDP.
    pub remote_address: Option<String>,
    pub remote_port: Option<u16>,
    /// TCP only: `listen` | `established` | `synSent` | `synReceived` | `finWait1` |
    /// `finWait2` | `closeWait` | `closing` | `lastAck` | `timeWait` | `closed`
    pub state: Option<&'static str>,
    pub pid: u32,
}

pub fn list() -> Vec<Connection> {
    let mut out = Vec::new();
    tcp4(&mut out);
    tcp6(&mut out);
    udp4(&mut out);
    udp6(&mut out);
    out
}

/// Ports are stored in network byte order in the low 16 bits.
fn port(raw: u32) -> u16 {
    u16::from_be(raw as u16)
}

fn table(fill: impl Fn(Option<*mut core::ffi::c_void>, &mut u32) -> u32) -> Option<AlignedBuf> {
    let mut size = 0u32;
    let mut buf = AlignedBuf::with_capacity(64 * 1024);
    for _ in 0..4 {
        size = size.max(buf.len() as u32);
        match fill(Some(buf.as_mut_ptr()), &mut size) {
            r if r == NO_ERROR.0 => return Some(buf),
            r if r == ERROR_INSUFFICIENT_BUFFER.0 => buf.grow_to(size as usize + 4096),
            _ => return None,
        }
    }
    None
}

/// Rows of a `MIB_*TABLE_OWNER_PID`: a u32 count followed by the rows.
fn rows<T>(buf: &AlignedBuf) -> &[T] {
    let count = unsafe { *(buf.as_ptr() as *const u32) } as usize;
    // Rows start at the first offset aligned for T after the count.
    let offset = std::mem::align_of::<T>().max(4);
    let max = (buf.len() - offset) / std::mem::size_of::<T>();
    unsafe { std::slice::from_raw_parts(buf.as_ptr().add(offset) as *const T, count.min(max)) }
}

fn tcp_state(state: u32) -> &'static str {
    match state {
        1 => "closed",
        2 => "listen",
        3 => "synSent",
        4 => "synReceived",
        5 => "established",
        6 => "finWait1",
        7 => "finWait2",
        8 => "closeWait",
        9 => "closing",
        10 => "lastAck",
        11 => "timeWait",
        _ => "closed",
    }
}

fn tcp4(out: &mut Vec<Connection>) {
    let Some(buf) = table(|p, size| unsafe {
        GetExtendedTcpTable(p, size, false, AF_INET.0 as u32, TCP_TABLE_OWNER_PID_ALL, 0)
    }) else {
        return;
    };
    for r in rows::<MIB_TCPROW_OWNER_PID>(&buf) {
        let listening = r.dwState == 2;
        out.push(Connection {
            protocol: "tcp",
            ipv6: false,
            local_address: Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes()).to_string(),
            local_port: port(r.dwLocalPort),
            remote_address: (!listening)
                .then(|| Ipv4Addr::from(r.dwRemoteAddr.to_ne_bytes()).to_string()),
            remote_port: (!listening).then(|| port(r.dwRemotePort)),
            state: Some(tcp_state(r.dwState)),
            pid: r.dwOwningPid,
        });
    }
}

fn tcp6(out: &mut Vec<Connection>) {
    let Some(buf) = table(|p, size| unsafe {
        GetExtendedTcpTable(
            p,
            size,
            false,
            AF_INET6.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    }) else {
        return;
    };
    for r in rows::<MIB_TCP6ROW_OWNER_PID>(&buf) {
        let listening = r.dwState == 2;
        out.push(Connection {
            protocol: "tcp",
            ipv6: true,
            local_address: Ipv6Addr::from(r.ucLocalAddr).to_string(),
            local_port: port(r.dwLocalPort),
            remote_address: (!listening).then(|| Ipv6Addr::from(r.ucRemoteAddr).to_string()),
            remote_port: (!listening).then(|| port(r.dwRemotePort)),
            state: Some(tcp_state(r.dwState)),
            pid: r.dwOwningPid,
        });
    }
}

fn udp4(out: &mut Vec<Connection>) {
    let Some(buf) = table(|p, size| unsafe {
        GetExtendedUdpTable(p, size, false, AF_INET.0 as u32, UDP_TABLE_OWNER_PID, 0)
    }) else {
        return;
    };
    for r in rows::<MIB_UDPROW_OWNER_PID>(&buf) {
        out.push(Connection {
            protocol: "udp",
            ipv6: false,
            local_address: Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes()).to_string(),
            local_port: port(r.dwLocalPort),
            remote_address: None,
            remote_port: None,
            state: None,
            pid: r.dwOwningPid,
        });
    }
}

fn udp6(out: &mut Vec<Connection>) {
    let Some(buf) = table(|p, size| unsafe {
        GetExtendedUdpTable(p, size, false, AF_INET6.0 as u32, UDP_TABLE_OWNER_PID, 0)
    }) else {
        return;
    };
    for r in rows::<MIB_UDP6ROW_OWNER_PID>(&buf) {
        out.push(Connection {
            protocol: "udp",
            ipv6: true,
            local_address: Ipv6Addr::from(r.ucLocalAddr).to_string(),
            local_port: port(r.dwLocalPort),
            remote_address: None,
            remote_port: None,
            state: None,
            pid: r.dwOwningPid,
        });
    }
}

/// Resets an IPv4 TCP connection (needs administrator rights; Windows offers
/// no equivalent for IPv6).
pub fn close_tcp(
    local: &str,
    local_port: u16,
    remote: &str,
    remote_port: u16,
) -> Result<(), ActionError> {
    let parse = |s: &str| {
        s.parse::<Ipv4Addr>()
            .map_err(|_| ActionError::Os("只能关闭 IPv4 TCP 连接".into()))
    };
    let row = MIB_TCPROW_LH {
        Anonymous: MIB_TCPROW_LH_0 {
            State: MIB_TCP_STATE_DELETE_TCB,
        },
        dwLocalAddr: u32::from_ne_bytes(parse(local)?.octets()),
        dwLocalPort: local_port.to_be() as u32,
        dwRemoteAddr: u32::from_ne_bytes(parse(remote)?.octets()),
        dwRemotePort: remote_port.to_be() as u32,
    };
    match unsafe { SetTcpEntry(&row) } {
        0 => Ok(()),
        // 317 is what SetTcpEntry returns without elevation.
        5 | 317 => Err(ActionError::AccessDenied),
        code => Err(windows::core::Error::from(windows::core::HRESULT::from_win32(code)).into()),
    }
}
