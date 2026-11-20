//! The two file-system calls std does not offer: an NTFS file identity and junction creation.
#![expect(unsafe_code, reason = "W3's FFI module: GetFileInformationByHandle")]

use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle};

#[cfg(any(feature = "sharing", test))]
mod junction;
#[cfg(any(feature = "sharing", test))]
pub(crate) use junction::create_junction;

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
