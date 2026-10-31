//! The two Win32 calls the registry needs: the drive class of a letter (`[IO.DriveInfo]::DriveType`)
//! and the long form of an 8.3 path (the expansion `[IO.Path]::GetFullPath` does).
#![expect(unsafe_code, reason = "GetDriveTypeW and GetLongPathNameW have no safe std wrapper")]

use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLongPathNameW};

/// How a drive letter is treated by the presence check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DriveClass {
    /// A network drive: never probed, taken as present.
    Skip,
    /// No such drive, or a local one that is not ready.
    Absent,
    /// A local drive: the path itself is probed.
    Probe,
}

const DRIVE_NO_ROOT_DIR: u32 = 1;
const DRIVE_REMOTE: u32 = 4;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The drive table's answer. Readiness is asked of local drives only: on a network drive it is
/// the network call this check exists to avoid.
pub(super) fn drive_class(letter: char) -> DriveClass {
    let root = format!("{letter}:\\");
    let w = wide(&root);
    // SAFETY: `w` is a NUL-terminated UTF-16 string that lives across the call.
    let kind = unsafe { GetDriveTypeW(w.as_ptr()) };
    match kind {
        DRIVE_REMOTE => DriveClass::Skip,
        0 | DRIVE_NO_ROOT_DIR => DriveClass::Absent,
        _ if std::path::Path::new(&root).exists() => DriveClass::Probe,
        _ => DriveClass::Absent,
    }
}

/// The long form of a path that exists, `None` when it does not (or the call fails).
pub(super) fn long_path(path: &str) -> Option<String> {
    convert(path, |src, dst, cap| {
        // SAFETY: `src` is NUL-terminated and `dst` holds `cap` u16 slots, both live across the call.
        unsafe { GetLongPathNameW(src, dst, cap) }
    })
}

/// The 8.3 form, for the tests that prove both spellings are one key.
#[cfg(test)]
pub(super) fn short_path(path: &str) -> Option<String> {
    convert(path, |src, dst, cap| {
        // SAFETY: as in `long_path`.
        unsafe { windows_sys::Win32::Storage::FileSystem::GetShortPathNameW(src, dst, cap) }
    })
}

fn convert(path: &str, call: impl Fn(*const u16, *mut u16, u32) -> u32) -> Option<String> {
    let w = wide(path);
    let mut buf = vec![0u16; 1024];
    let cap = u32::try_from(buf.len()).ok()?;
    let n = call(w.as_ptr(), buf.as_mut_ptr(), cap);
    let n = usize::try_from(n).ok()?;
    if n == 0 || n >= buf.len() {
        return None;
    }
    buf.get(..n).map(String::from_utf16_lossy)
}
