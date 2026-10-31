//! Win32 for the companion path: who listens on a TCP port (IPv4 and IPv6), a process's command
//! line, stopping a process, and this machine's IPv4 addresses. IP Helper and `ntdll` are linked
//! here directly (`raw-dylib`): their `windows-sys` features are not enabled in this crate.
#![expect(unsafe_code, reason = "IP Helper, ntdll and process FFI")]

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess};

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
const TCP_TABLE_OWNER_PID_LISTENER: i32 = 3;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;

#[link(name = "iphlpapi", kind = "raw-dylib")]
unsafe extern "system" {
    fn GetExtendedTcpTable(table: *mut c_void, size: *mut u32, order: i32, af: u32, class: i32, reserved: u32) -> u32;
    fn GetIpAddrTable(table: *mut c_void, size: *mut u32, order: i32) -> u32;
}

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    fn NtQueryInformationProcess(process: HANDLE, class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
}

struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: a handle OpenProcess returned, closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4).and_then(|s| s.try_into().ok()).map(u32::from_ne_bytes)
}

/// Calls a size-negotiating IP Helper function until the buffer fits; the bytes it filled.
fn sized_table(call: &dyn Fn(*mut c_void, *mut u32) -> u32) -> Option<Vec<u8>> {
    let mut want = 4096u32;
    for _ in 0..4 {
        // u64 words: the tables are read as 4-byte fields, and this keeps them aligned.
        let mut buf = vec![0u64; (want as usize).div_ceil(8)];
        let mut size = u32::try_from(buf.len() * 8).ok()?;
        match call(buf.as_mut_ptr().cast(), &raw mut size) {
            0 => return Some(buf.iter().flat_map(|w| w.to_ne_bytes()).take(size as usize).collect()),
            ERROR_INSUFFICIENT_BUFFER => want = size.max(want * 2),
            _ => return None,
        }
    }
    None
}

fn listener_in(af: u32, port: u16) -> Option<u32> {
    // SAFETY: `t` and `s` are the buffer and its size from `sized_table`.
    let call = |t: *mut c_void, s: *mut u32| unsafe { GetExtendedTcpTable(t, s, 0, af, TCP_TABLE_OWNER_PID_LISTENER, 0) };
    let b = sized_table(&call)?;
    let n = u32_at(&b, 0)? as usize;
    // MIB_TCPROW_OWNER_PID (24 bytes) or MIB_TCP6ROW_OWNER_PID (56): local port and owning pid.
    let (row, port_at, pid_at) = if af == AF_INET { (24, 8, 20) } else { (56, 20, 52) };
    (0..n).find_map(|i| {
        let base = 4 + i * row;
        // The port sits in network order in the field's first two bytes.
        let local = u16::from_be_bytes(b.get(base + port_at..base + port_at + 2)?.try_into().ok()?);
        (local == port).then(|| u32_at(&b, base + pid_at)).flatten()
    })
}

/// `Get-NetTCPConnection -LocalPort <port> -State Listen`: the owning pid of a listener on any
/// address of either family (node listens on `::` by default).
pub(crate) fn listener_pid(port: u16) -> Option<u32> {
    listener_in(AF_INET, port).or_else(|| listener_in(AF_INET6, port))
}

/// A process's command line, `None` when it cannot be opened or read.
pub(crate) fn command_line(pid: u32) -> Option<String> {
    // SAFETY: plain call; a null handle is checked below.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return None;
    }
    let h = Owned(h);
    let mut len = 0u32;
    // SAFETY: a zero-length probe for the size; the status is ignored, `len` is what is read.
    unsafe { NtQueryInformationProcess(h.0, PROCESS_COMMAND_LINE_INFORMATION, std::ptr::null_mut(), 0, &raw mut len) };
    if len == 0 {
        return None;
    }
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    let cap = u32::try_from(buf.len() * 8).ok()?;
    // SAFETY: `buf` has `cap` writable, 8-aligned bytes for the UNICODE_STRING and its text.
    let st = unsafe { NtQueryInformationProcess(h.0, PROCESS_COMMAND_LINE_INFORMATION, buf.as_mut_ptr().cast(), cap, &raw mut len) };
    if st < 0 {
        return None;
    }
    let base = buf.as_ptr().addr();
    let bytes: Vec<u8> = buf.iter().flat_map(|w| w.to_ne_bytes()).collect();
    // UNICODE_STRING: Length (u16) at 0, Buffer (pointer) at 8, pointing into this same buffer.
    let chars = usize::from(u16::from_ne_bytes(bytes.get(0..2)?.try_into().ok()?));
    let ptr = usize::try_from(u64::from_ne_bytes(bytes.get(8..16)?.try_into().ok()?)).ok()?;
    let off = ptr.checked_sub(base)?;
    let text: Vec<u16> = bytes
        .get(off..off + chars)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_ne_bytes(*c))
        .collect();
    Some(String::from_utf16_lossy(&text))
}

/// `Stop-Process -Force`.
pub(crate) fn terminate(pid: u32) -> Result<(), String> {
    // SAFETY: plain call; a null handle is checked below.
    let h = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if h.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let h = Owned(h);
    // SAFETY: a handle opened with PROCESS_TERMINATE.
    if unsafe { TerminateProcess(h.0, 1) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

/// Every IPv4 address of this machine, in table order.
pub(crate) fn ipv4_addresses() -> Vec<[u8; 4]> {
    // SAFETY: `t` and `s` are the buffer and its size from `sized_table`.
    let call = |t: *mut c_void, s: *mut u32| unsafe { GetIpAddrTable(t, s, 0) };
    let Some(b) = sized_table(&call) else {
        return Vec::new();
    };
    let n = u32_at(&b, 0).unwrap_or(0) as usize;
    // MIB_IPADDRROW is 24 bytes; dwAddr first, in network order, so its bytes read a.b.c.d.
    (0..n)
        .filter_map(|i| b.get(4 + i * 24..8 + i * 24).and_then(|s| s.try_into().ok()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loopback_listener_is_found_with_this_pid_in_both_families() {
        let v4 = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let p4 = v4.local_addr().unwrap().port();
        assert_eq!(listener_pid(p4), Some(std::process::id()));
        let v6 = std::net::TcpListener::bind("[::1]:0").unwrap();
        let p6 = v6.local_addr().unwrap().port();
        assert_eq!(listener_pid(p6), Some(std::process::id()));
        drop((v4, v6));
        assert_eq!(listener_pid(p4), None, "a closed port has no listener");
    }

    #[test]
    fn own_command_line_names_this_test_binary() {
        let exe = std::env::current_exe().unwrap();
        let stem = exe.file_stem().unwrap().to_string_lossy().into_owned();
        assert!(command_line(std::process::id()).unwrap().contains(&stem));
        assert_eq!(command_line(u32::MAX - 3), None);
    }

    #[test]
    fn loopback_is_among_the_ipv4_addresses() {
        assert!(ipv4_addresses().contains(&[127, 0, 0, 1]));
    }

    #[test]
    fn terminating_a_missing_pid_is_an_error_not_a_panic() {
        assert!(terminate(u32::MAX - 3).is_err());
    }
}
