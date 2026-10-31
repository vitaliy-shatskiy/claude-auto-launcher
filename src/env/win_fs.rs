//! The two file-system calls std does not offer: an NTFS file identity and junction creation.
#![expect(unsafe_code, reason = "W3's FFI module: GetFileInformationByHandle and FSCTL_SET_REPARSE_POINT")]

use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, GetFileInformationByHandle,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const GENERIC_WRITE: u32 = 0x4000_0000;
const SHARE_ALL: u32 = 7;

/// `Get-SharedFileId`: volume serial plus file index = identity. Equal contents never mean one
/// file. `None` = the question was not answered, never "linked".
pub(crate) fn file_id(path: &Path) -> Option<String> {
    let f = OpenOptions::new()
        .access_mode(0)
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    // SAFETY: plain data out parameter.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `f` is an open handle for the duration of the call; `info` is a valid out pointer.
    let ok = unsafe { GetFileInformationByHandle(f.as_raw_handle(), &raw mut info) };
    (ok != 0).then(|| {
        format!(
            "{:08x}-{:08x}{:08x}",
            info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
        )
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junction_reaches_its_target_and_hardlinks_share_an_id() {
        let dir = std::env::temp_dir().join(format!("claude-auto-rs-winfs-{}", crate::log::new_run_id()));
        let target = dir.join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("a.txt"), "x").unwrap();
        let link = dir.join("link");
        create_junction(&link, &target).unwrap();
        assert_eq!(std::fs::read_to_string(link.join("a.txt")).unwrap(), "x");
        assert_eq!(std::fs::read_link(&link).unwrap(), target);

        std::fs::hard_link(target.join("a.txt"), dir.join("b.txt")).unwrap();
        std::fs::write(dir.join("c.txt"), "x").unwrap();
        let a = file_id(&target.join("a.txt")).unwrap();
        assert_eq!(Some(a.clone()), file_id(&dir.join("b.txt")));
        assert_ne!(Some(a), file_id(&dir.join("c.txt")));
        assert_eq!(file_id(&dir.join("missing")), None);

        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
