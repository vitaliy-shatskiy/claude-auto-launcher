//! The IP Helper tables (`GetExtendedTcpTable`, `GetIpAddrTable`): TCP listeners with their pid and
//! the local IPv4 addresses. Only the Rider MCP scan and the remote module read them.
#![expect(unsafe_code, reason = "the IP Helper calls, beside the buffers they write")]

use std::ffi::c_void;

use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows_sys::Win32::NetworkManagement::IpHelper::{GetExtendedTcpTable, GetIpAddrTable, TCP_TABLE_OWNER_PID_LISTENER};

/// The IP family of a TCP table (`AF_INET` / `AF_INET6`, from the `WinSock` feature not enabled here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    V4,
    #[cfg_attr(not(feature = "remote"), expect(dead_code, reason = "only the remote module asks for IPv6"))]
    V6,
}

/// A listening TCP socket: the local IPv4 address (`None` for IPv6), port and owning pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Listener {
    pub(crate) v4: Option<[u8; 4]>,
    pub(crate) port: u16,
    pub(crate) pid: u32,
}

pub(crate) fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4).and_then(|s| s.try_into().ok()).map(u32::from_ne_bytes)
}

/// The size-negotiating IP Helper tables the launcher reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IpTable {
    /// `GetExtendedTcpTable`, listeners with their owning pid.
    TcpListeners(Family),
    /// `GetIpAddrTable`.
    #[cfg_attr(not(feature = "remote"), expect(dead_code, reason = "only the remote module reads the addresses"))]
    Ipv4Addresses,
}

/// Calls the table's IP Helper function until the buffer fits; the bytes it filled. The call
/// happens here, beside the buffer it writes, so the SAFETY proof is local.
pub(crate) fn sized_table(table: IpTable) -> Option<Vec<u8>> {
    let mut want = 4096u32;
    for _ in 0..4 {
        // u64 words: the tables are read as 4-byte fields, and this keeps them aligned.
        let mut buf = vec![0u64; (want as usize).div_ceil(8)];
        let mut size = u32::try_from(buf.len() * 8).ok()?;
        let (out, len) = (buf.as_mut_ptr().cast::<c_void>(), &raw mut size);
        let rc = match table {
            IpTable::TcpListeners(family) => {
                let af = match family {
                    Family::V4 => 2,
                    Family::V6 => 23,
                };
                // SAFETY: `out` is `buf`, writable for `size` bytes, `len` points at `size`, both
                // live across the call; the function writes no more than `*len` bytes.
                unsafe { GetExtendedTcpTable(out, len, 0, af, TCP_TABLE_OWNER_PID_LISTENER, 0) }
            }
            // SAFETY: as above; MIB_IPADDRTABLE needs 4-byte alignment and `buf` is u64-aligned.
            IpTable::Ipv4Addresses => unsafe { GetIpAddrTable(out.cast(), len, 0) },
        };
        match rc {
            0 => return Some(buf.iter().flat_map(|w| w.to_ne_bytes()).take(size as usize).collect()),
            ERROR_INSUFFICIENT_BUFFER => want = size.max(want * 2),
            _ => return None,
        }
    }
    None
}

/// `Get-NetTCPConnection -State Listen` for one family (`GetExtendedTcpTable`); empty on failure.
pub(crate) fn tcp_listeners(family: Family) -> Vec<Listener> {
    let Some(b) = sized_table(IpTable::TcpListeners(family)) else {
        return Vec::new();
    };
    let n = u32_at(&b, 0).unwrap_or(0) as usize;
    // MIB_TCPROW_OWNER_PID (24 bytes: addr at 4, port at 8, pid at 20) or MIB_TCP6ROW_OWNER_PID
    // (56 bytes: port at 20, pid at 52).
    let (row, port_at, pid_at) = match family {
        Family::V4 => (24, 8, 20),
        Family::V6 => (56, 20, 52),
    };
    (0..n)
        .filter_map(|i| {
            let base = 4 + i * row;
            // The port sits in network order in the field's first two bytes.
            let port = u16::from_be_bytes(b.get(base + port_at..base + port_at + 2)?.try_into().ok()?);
            let v4 = match family {
                Family::V4 => Some(b.get(base + 4..base + 8)?.try_into().ok()?),
                Family::V6 => None,
            };
            Some(Listener {
                v4,
                port,
                pid: u32_at(&b, base + pid_at)?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loopback_listener_is_listed_with_its_address_port_and_pid() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let me = Listener {
            v4: Some([127, 0, 0, 1]),
            port,
            pid: std::process::id(),
        };
        assert!(tcp_listeners(Family::V4).contains(&me));
    }
}
