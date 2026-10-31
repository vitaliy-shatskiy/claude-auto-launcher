//! Every Win32 call the launcher makes, behind safe wrappers: console input and modes, the
//! window size, the code page, Ctrl+C, the prefs mutex, SHA-256, local time, the process table and
//! the TCP listener table.
#![expect(unsafe_code, reason = "the one FFI module: Win32 console, mutex, hash and process calls")]

use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, INVALID_HANDLE_VALUE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::NetworkManagement::IpHelper::{GetExtendedTcpTable, TCP_TABLE_OWNER_PID_LISTENER};
use windows_sys::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::Win32::System::Console::{
    CONSOLE_SCREEN_BUFFER_INFO, FlushConsoleInputBuffer, GetConsoleMode, GetConsoleOutputCP, GetConsoleScreenBufferInfo,
    GetNumberOfConsoleInputEvents, INPUT_RECORD, PeekConsoleInputW, ReadConsoleInputW, SetConsoleCtrlHandler, SetConsoleMode,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// An owned Win32 handle, closed on drop.
#[derive(Debug)]
pub(crate) struct Handle(HANDLE);

impl Handle {
    /// `CONIN$` or `CONOUT$` opened read/write: `SetConsoleMode` on the input buffer fails on a
    /// read-only handle, and `CONIN$` still opens when stdin is redirected.
    pub(crate) fn open_console(name: &str) -> Option<Self> {
        let path = wide(name);
        // SAFETY: `path` is a NUL-terminated UTF-16 string that outlives the call; the other
        // pointers are null, which CreateFileW accepts.
        let h = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            None
        } else {
            Some(Self(h))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned open by a Win32 call and is closed exactly once here.
        unsafe { CloseHandle(self.0) };
    }
}

pub(crate) fn console_mode(h: &Handle) -> Option<u32> {
    let mut mode = 0u32;
    // SAFETY: `h` is an open console handle and `mode` a valid out pointer.
    let ok = unsafe { GetConsoleMode(h.0, &raw mut mode) };
    (ok != 0).then_some(mode)
}

pub(crate) fn set_console_mode(h: &Handle, mode: u32) -> bool {
    // SAFETY: `h` is an open console handle.
    unsafe { SetConsoleMode(h.0, mode) != 0 }
}

/// Waits up to `timeout_ms` for the input handle to signal (the queue is non-empty).
pub(crate) fn wait_signalled(h: &Handle, timeout_ms: u32) -> bool {
    // SAFETY: `h` is an open, waitable console input handle.
    unsafe { WaitForSingleObject(h.0, timeout_ms) == WAIT_OBJECT_0 }
}

pub(crate) fn read_record(h: &Handle) -> Option<INPUT_RECORD> {
    // SAFETY: INPUT_RECORD is plain data; an all-zero value is a valid bit pattern.
    let mut rec: INPUT_RECORD = unsafe { std::mem::zeroed() };
    let mut n = 0u32;
    // SAFETY: one-record buffer and count pointer, both valid for the call.
    let ok = unsafe { ReadConsoleInputW(h.0, &raw mut rec, 1, &raw mut n) };
    (ok != 0 && n == 1).then_some(rec)
}

pub(crate) fn peek_records(h: &Handle, max: usize) -> Vec<INPUT_RECORD> {
    // SAFETY: INPUT_RECORD is plain data; zeroed values are valid.
    let mut buf: Vec<INPUT_RECORD> = vec![unsafe { std::mem::zeroed() }; max];
    let mut n = 0u32;
    let len = u32::try_from(max).unwrap_or(u32::MAX);
    // SAFETY: `buf` holds `max` records and `n` is a valid out pointer.
    let ok = unsafe { PeekConsoleInputW(h.0, buf.as_mut_ptr(), len, &raw mut n) };
    if ok == 0 {
        return Vec::new();
    }
    buf.truncate(n as usize);
    buf
}

pub(crate) fn pending_events(h: &Handle) -> u32 {
    let mut n = 0u32;
    // SAFETY: valid handle and out pointer.
    let ok = unsafe { GetNumberOfConsoleInputEvents(h.0, &raw mut n) };
    if ok == 0 { 0 } else { n }
}

pub(crate) fn flush_input(h: &Handle) -> bool {
    // SAFETY: valid console input handle.
    unsafe { FlushConsoleInputBuffer(h.0) != 0 }
}

/// The visible window of the console: (width, height, top row in buffer coordinates).
pub(crate) fn window_size(h: &Handle) -> Option<(i32, i32, i32)> {
    // SAFETY: plain data, zero is valid.
    let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { std::mem::zeroed() };
    // SAFETY: valid output handle and out pointer.
    let ok = unsafe { GetConsoleScreenBufferInfo(h.0, &raw mut info) };
    if ok == 0 {
        return None;
    }
    let w = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
    let ht = i32::from(info.srWindow.Bottom) - i32::from(info.srWindow.Top) + 1;
    Some((w, ht, i32::from(info.srWindow.Top)))
}

pub(crate) fn output_code_page() -> u32 {
    // SAFETY: no arguments; returns 0 when there is no console.
    unsafe { GetConsoleOutputCP() }
}

// The console control handler: Ctrl+C and Ctrl+Break reach both this process and the child
// `claude`. Handled here (returning TRUE) so the launcher keeps waiting for the child; a NULL
// handler with TRUE would be inherited by the child and take Ctrl+C away from claude itself.
unsafe extern "system" fn swallow_ctrl(kind: u32) -> i32 {
    i32::from(kind <= 1)
}

pub(crate) fn ignore_ctrl_c() {
    // SAFETY: registers a static function that touches no state.
    unsafe { SetConsoleCtrlHandler(Some(swallow_ctrl), 1) };
}

/// A named mutex, released and closed on drop when held.
#[derive(Debug)]
pub(crate) struct NamedMutex {
    handle: Handle,
    held: bool,
}

impl NamedMutex {
    /// Opens `name` and waits up to `timeout_ms`. An abandoned mutex counts as held: the previous
    /// owner died mid-write and the caller is about to rewrite the file anyway.
    pub(crate) fn acquire(name: &str, timeout_ms: u32) -> Option<Self> {
        let w = wide(name);
        // SAFETY: NUL-terminated name, null security attributes.
        let h = unsafe { CreateMutexW(ptr::null(), 0, w.as_ptr()) };
        if h.is_null() {
            return None;
        }
        let handle = Handle(h);
        // SAFETY: `handle` is a valid mutex handle.
        let r = unsafe { WaitForSingleObject(handle.0, timeout_ms) };
        Some(Self {
            held: r == WAIT_OBJECT_0 || r == WAIT_ABANDONED,
            handle,
        })
    }

    pub(crate) fn held(&self) -> bool {
        self.held
    }
}

impl Drop for NamedMutex {
    fn drop(&mut self) {
        if self.held {
            // SAFETY: this thread owns the mutex.
            unsafe { ReleaseMutex(self.handle.0) };
        }
    }
}

pub(crate) fn sha256(data: &[u8]) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    let len = u32::try_from(data.len()).ok()?;
    // SAFETY: the SHA-256 pseudo-handle needs no open; input and output buffers are valid.
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, ptr::null(), 0, data.as_ptr(), len, out.as_mut_ptr(), 32) };
    (status == 0).then_some(out)
}

/// Local wall clock now and its offset from UTC in minutes:
/// (year, month, day, hour, minute, second, 100ns fraction, offset minutes).
#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalNow {
    pub(crate) year: i32,
    pub(crate) month: u32,
    pub(crate) day: u32,
    pub(crate) hour: u32,
    pub(crate) minute: u32,
    pub(crate) second: u32,
    pub(crate) ticks: u32,
    pub(crate) offset_minutes: i32,
}

pub(crate) fn local_now() -> Option<LocalNow> {
    // SAFETY: plain data out parameters.
    let mut ft = unsafe { std::mem::zeroed() };
    // SAFETY: valid out pointer.
    unsafe { GetSystemTimeAsFileTime(&raw mut ft) };
    // SAFETY: plain data.
    let mut utc = unsafe { std::mem::zeroed() };
    // SAFETY: valid in and out pointers.
    if unsafe { FileTimeToSystemTime(&raw const ft, &raw mut utc) } == 0 {
        return None;
    }
    // SAFETY: plain data.
    let mut local: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
    // SAFETY: null = the current time zone; valid in and out pointers.
    if unsafe { SystemTimeToTzSpecificLocalTime(ptr::null(), &raw const utc, &raw mut local) } == 0 {
        return None;
    }
    let ticks100 = (u64::from(ft.dwHighDateTime) << 32 | u64::from(ft.dwLowDateTime)) % 10_000_000;
    let minutes = |t: &windows_sys::Win32::Foundation::SYSTEMTIME| {
        days_from_civil(i32::from(t.wYear), u32::from(t.wMonth), u32::from(t.wDay)) * 1440 + i64::from(t.wHour) * 60 + i64::from(t.wMinute)
    };
    let offset = minutes(&local) - minutes(&utc);
    Some(LocalNow {
        year: i32::from(local.wYear),
        month: u32::from(local.wMonth),
        day: u32::from(local.wDay),
        hour: u32::from(local.wHour),
        minute: u32::from(local.wMinute),
        second: u32::from(local.wSecond),
        ticks: u32::try_from(ticks100).unwrap_or(0),
        offset_minutes: i32::try_from(offset).unwrap_or(0),
    })
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub(crate) fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// One process-table row: pid, parent pid, executable name.
#[derive(Debug, Clone)]
pub(crate) struct ProcessRow {
    pub(crate) pid: u32,
    pub(crate) parent: u32,
    pub(crate) name: String,
}

pub(crate) fn process_table() -> Vec<ProcessRow> {
    // SAFETY: snapshot of all processes; checked for failure below.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap.is_null() || snap == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let snap = Handle(snap);
    let mut out = Vec::new();
    // SAFETY: plain data.
    let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    e.dwSize = u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(0);
    // SAFETY: valid snapshot handle and entry with dwSize set.
    let mut ok = unsafe { Process32FirstW(snap.0, &raw mut e) } != 0;
    while ok {
        let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
        let name = String::from_utf16_lossy(e.szExeFile.get(..len).unwrap_or(&[]));
        out.push(ProcessRow {
            pid: e.th32ProcessID,
            parent: e.th32ParentProcessID,
            name,
        });
        // SAFETY: same snapshot and entry.
        ok = unsafe { Process32NextW(snap.0, &raw mut e) } != 0;
    }
    out
}

/// Opaque pointer type re-exported for the console module's record decoding.
pub(crate) type RawRecord = INPUT_RECORD;

/// Reads the key fields of a `KEY_EVENT` record: (down, vk, char, control state).
pub(crate) fn key_fields(r: &RawRecord) -> (bool, u16, u16, u32) {
    // SAFETY: the caller checked EventType == KEY_EVENT, so the KeyEvent arm is the live one.
    let k = unsafe { r.Event.KeyEvent };
    // SAFETY: UnicodeChar is the arm ReadConsoleInputW (the W variant) fills.
    let ch = unsafe { k.uChar.UnicodeChar };
    (k.bKeyDown != 0, k.wVirtualKeyCode, ch, k.dwControlKeyState)
}

/// Reads a `MOUSE_EVENT` record: (x, y, buttons, flags).
pub(crate) fn mouse_fields(r: &RawRecord) -> (i32, i32, u32, u32) {
    // SAFETY: the caller checked EventType == MOUSE_EVENT.
    let m = unsafe { r.Event.MouseEvent };
    (
        i32::from(m.dwMousePosition.X),
        i32::from(m.dwMousePosition.Y),
        m.dwButtonState,
        m.dwEventFlags,
    )
}

/// Reads a `WINDOW_BUFFER_SIZE_EVENT` record: (columns, rows).
pub(crate) fn size_fields(r: &RawRecord) -> (i32, i32) {
    // SAFETY: the caller checked EventType == WINDOW_BUFFER_SIZE_EVENT.
    let s = unsafe { r.Event.WindowBufferSizeEvent };
    (i32::from(s.dwSize.X), i32::from(s.dwSize.Y))
}

/// The IP family of a TCP table (`AF_INET` / `AF_INET6`, from the `WinSock` feature not enabled here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    V4,
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

/// Calls a size-negotiating IP Helper function until the buffer fits; the bytes it filled.
pub(crate) fn sized_table(call: &dyn Fn(*mut c_void, *mut u32) -> u32) -> Option<Vec<u8>> {
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

/// `Get-NetTCPConnection -State Listen` for one family (`GetExtendedTcpTable`); empty on failure.
pub(crate) fn tcp_listeners(family: Family) -> Vec<Listener> {
    let af = match family {
        Family::V4 => 2,
        Family::V6 => 23,
    };
    // SAFETY: `t` and `s` are the buffer and its size from `sized_table`.
    let call = |t: *mut c_void, s: *mut u32| unsafe { GetExtendedTcpTable(t, s, 0, af, TCP_TABLE_OWNER_PID_LISTENER, 0) };
    let Some(b) = sized_table(&call) else {
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
