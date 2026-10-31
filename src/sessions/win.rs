//! The one Win32 call the holder test needs: when a process started (`Process.StartTime`).
#![expect(unsafe_code, reason = "OpenProcess and GetProcessTimes have no safe std form")]

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

/// The process creation time as a UTC FILETIME, or `None` when it is gone or not readable.
pub(crate) fn process_start(pid: u32) -> Option<u64> {
    // SAFETY: plain call; a null handle is checked below.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return None;
    }
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: a valid process handle and four valid out pointers.
    let ok = unsafe { GetProcessTimes(h, &raw mut created, &raw mut exited, &raw mut kernel, &raw mut user) } != 0;
    // SAFETY: the handle was opened above and is closed once.
    unsafe { CloseHandle(h) };
    ok.then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}
