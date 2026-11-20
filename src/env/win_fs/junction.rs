//! `New-Item -ItemType Junction` (`FSCTL_SET_REPARSE_POINT`): only sharing repair and tests make junctions.
#![expect(unsafe_code, reason = "W3's FFI module: FSCTL_SET_REPARSE_POINT")]

use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;

use super::SHARE_ALL;

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const GENERIC_WRITE: u32 = 0x4000_0000;

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// The `REPARSE_DATA_BUFFER` of a mount point: `\??\<target>` as the substitute name, the plain
/// path as the print name, each NUL-terminated.
fn mount_point_buffer(target: &str) -> Option<Vec<u8>> {
    let sub = utf16(&format!(r"\??\{target}"));
    let print = utf16(target);
    let sub_len = u16::try_from(sub.len() * 2).ok()?;
    let print_len = u16::try_from(print.len() * 2).ok()?;
    let data_len = 8u16.checked_add(sub_len)?.checked_add(2)?.checked_add(print_len)?.checked_add(2)?;
    let mut b = Vec::with_capacity(8 + usize::from(data_len));
    b.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    b.extend_from_slice(&data_len.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    for v in [0, sub_len, sub_len + 2, print_len] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for c in sub.iter().chain(&[0]).chain(&print).chain(&[0]) {
        b.extend_from_slice(&c.to_le_bytes());
    }
    Some(b)
}

fn set_mount_point(dir: &File, target: &str) -> std::io::Result<()> {
    let buf = mount_point_buffer(target).ok_or_else(|| std::io::Error::other("junction target path too long"))?;
    let len = u32::try_from(buf.len()).map_err(std::io::Error::other)?;
    let mut returned = 0u32;
    // SAFETY: `dir` is an open handle with write access; `buf` is a valid input buffer of `len`
    // bytes; no output buffer; `returned` is a valid out pointer; no OVERLAPPED.
    let ok = unsafe {
        DeviceIoControl(
            dir.as_raw_handle(),
            FSCTL_SET_REPARSE_POINT,
            buf.as_ptr().cast(),
            len,
            ptr::null_mut(),
            0,
            &raw mut returned,
            ptr::null_mut(),
        )
    };
    if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

/// `New-Item -ItemType Junction`: an empty directory at `link` turned into a mount point onto
/// `target` (made absolute). A failed conversion removes the directory again.
pub(crate) fn create_junction(link: &Path, target: &Path) -> std::io::Result<()> {
    let abs = std::path::absolute(target)?;
    let text = abs.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned();
    std::fs::create_dir(link)?;
    let made = OpenOptions::new()
        .access_mode(GENERIC_WRITE)
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(link)
        .and_then(|d| set_mount_point(&d, &text));
    if let Err(e) = made {
        if let Err(cleanup) = std::fs::remove_dir(link) {
            return Err(std::io::Error::other(format!("{e} (and the empty directory stayed: {cleanup})")));
        }
        return Err(e);
    }
    Ok(())
}
