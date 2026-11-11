//! Whole-file writes no concurrent reader sees half of (`write_atomic` for caches, `write_durable`
//! for user data: a temp beside the target, then a rename), reads and renames that ride out a
//! moment's lock (`read`, `retry_transient`), the check that tells a missing file from one another
//! writer is replacing (`exists_settled`), the name of the temps (`is_temp_of`) and the sweep of
//! temps an exit left behind.
//!
//! What a write keeps of the target: a symlink stays a link (its target is written), a file with
//! more than one hard link stays shared (rewritten in place, see `write_in_place`), and the hidden
//! and system attributes carry over to the replacement. Owner, ACL and creation time are the
//! temp's, as with any replace-by-rename.

use std::fs;
use std::io::{self, Write as _};
use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM`: what a replace carries over from the old file.
const KEPT_ATTRIBUTES: u32 = 0x2 | 0x4;
/// Retries after access denied (5) or a sharing violation (32): an antivirus scan, the indexer or
/// another launcher's read holds the file for a moment.
const TRANSIENT_RETRIES: u32 = 3;
const RETRY_GAP: Duration = Duration::from_millis(25);
/// A temp this young beside a missing target is another writer's replace in flight.
const FRESH_TEMP: Duration = Duration::from_mins(1);
/// How often `exists_settled` asks again while such a temp sits there (`RETRY_GAP` apart).
const SETTLE_TRIES: u32 = 10;

/// A cache write: whole or not at all, not flushed to disk (a power cut may lose it; the next run
/// recomputes). Day-old temps of `path` are swept first; on an error the target is untouched.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_with(path, bytes, |_| Ok(()))
}

/// A user-data write (config, prefs): as `write_atomic`, and the bytes are on disk before the
/// rename, so a power cut leaves the old file or the new one, never an empty one.
pub(crate) fn write_durable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_with(path, bytes, fs::File::sync_all)
}

/// The write itself, `flush` run on the written temp before the rename.
fn write_with(path: &Path, bytes: &[u8], flush: impl FnOnce(&fs::File) -> io::Result<()>) -> io::Result<()> {
    let path = &resolve_link(path)?;
    remove_stale_temps(path, SystemTime::now());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    if fs::File::open(path)
        .ok()
        .and_then(|f| crate::sys::link_count(&f))
        .is_some_and(|n| n > 1)
    {
        return write_in_place(path, bytes, flush);
    }
    let attributes = fs::metadata(path).map_or(0, |m| m.file_attributes() & KEPT_ATTRIBUTES);
    let tmp = temp_path(path);
    let result = write_new(&tmp, bytes, attributes, flush).and_then(|()| retry_transient(|| fs::rename(&tmp, path)));
    if result.is_err() {
        // The temp may never have been created; one that cannot go now is swept a day later.
        fs::remove_file(&tmp).unwrap_or_default();
    }
    result
}

/// A symlink's final target (the write then keeps the link), any other path as it is.
fn resolve_link(path: &Path) -> io::Result<PathBuf> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        fs::canonicalize(path)
    } else {
        Ok(path.to_owned())
    }
}

/// The temp, created new with `attributes`, written and flushed; closed before the rename.
fn write_new(tmp: &Path, bytes: &[u8], attributes: u32, flush: impl FnOnce(&fs::File) -> io::Result<()>) -> io::Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .attributes(attributes)
        .open(tmp)?;
    f.write_all(bytes)?;
    flush(&f)
}

/// A file with other hard links: a rename would cut this name off from them, so the bytes go into
/// the file itself. Not atomic (a reader may see a mix, a power cut mid-write may leave one), the
/// price of keeping the link; the new bytes go first and the length is cut last, so the file never
/// holds less than the new text.
fn write_in_place(path: &Path, bytes: &[u8], flush: impl FnOnce(&fs::File) -> io::Result<()>) -> io::Result<()> {
    let mut f = retry_transient(|| fs::OpenOptions::new().write(true).open(path))?;
    f.write_all(bytes)?;
    f.set_len(u64::try_from(bytes.len()).map_err(io::Error::other)?)?;
    flush(&f)
}

/// Runs `op` again, up to `TRANSIENT_RETRIES` times `RETRY_GAP` apart, while it fails with access
/// denied or a sharing violation; any other result is returned at once.
pub(crate) fn retry_transient<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut left = TRANSIENT_RETRIES;
    loop {
        match op() {
            Err(e) if left > 0 && matches!(e.raw_os_error(), Some(5 | 32)) => {
                left -= 1;
                std::thread::sleep(RETRY_GAP);
            }
            r => return r,
        }
    }
}

/// `fs::read` that rides out a moment's lock (`retry_transient`).
pub(crate) fn read(path: &Path) -> io::Result<Vec<u8>> {
    retry_transient(|| fs::read(path))
}

/// `path` exists. While it is missing but a fresh temp of it sits beside it (another writer's
/// replace in flight), asks again for a while, then answers with an error: a caller that took the
/// absence for "missing" would start from nothing and write over the user's file.
pub(crate) fn exists_settled(path: &Path) -> io::Result<bool> {
    for _ in 0..SETTLE_TRIES {
        if path.try_exists()? {
            return Ok(true);
        }
        let cutoff = SystemTime::now().checked_sub(FRESH_TEMP).unwrap_or(UNIX_EPOCH);
        if !temps_of(path).iter().any(|e| !modified_before(e, cutoff)) {
            return Ok(false);
        }
        std::thread::sleep(RETRY_GAP);
    }
    Err(io::Error::other(format!("{} is being replaced by another writer", path.display())))
}

/// A temp name unique across processes (the pid) and across this process's threads (the count).
fn temp_path(path: &Path) -> PathBuf {
    // The one process-wide counter the temp names need; nothing else reads it.
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    temp_name(path, std::process::id(), nanos, NEXT.fetch_add(1, Ordering::Relaxed))
}

/// The whole count goes into the name, so two writes of one process never share a temp.
fn temp_name(path: &Path, pid: u32, nanos: u32, n: u32) -> PathBuf {
    PathBuf::from(format!("{}.{pid}-{nanos:08x}{n:04x}.tmp", path.display()))
}

/// `name` is a temp of the file named `target`: `<target>.<pid>-<hex>.tmp`.
pub(crate) fn is_temp_of(name: &str, target: &str) -> bool {
    name.strip_prefix(target)
        .and_then(|r| r.strip_prefix('.'))
        .and_then(|r| r.strip_suffix(".tmp"))
        .and_then(|r| r.split_once('-'))
        .is_some_and(|(pid, hex)| {
            !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()) && !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit())
        })
}

/// The day-old temps beside `target` that an exit cut off; younger ones may be another run's write.
pub(crate) fn remove_stale_temps(target: &Path, now: SystemTime) {
    let cutoff = now.checked_sub(Duration::from_hours(24)).unwrap_or(UNIX_EPOCH);
    for e in temps_of(target).iter().filter(|e| modified_before(e, cutoff)) {
        fs::remove_file(e.path()).unwrap_or_default();
    }
}

/// The temps of `target` in its folder; none when the folder cannot be listed.
fn temps_of(target: &Path) -> Vec<fs::DirEntry> {
    let (Some(dir), Some(name)) = (target.parent(), target.file_name().map(|n| n.to_string_lossy().into_owned())) else {
        return Vec::new();
    };
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter(|e| is_temp_of(&e.file_name().to_string_lossy(), &name))
        .collect()
}

/// A file entry last written before `cutoff`.
pub(crate) fn modified_before(e: &fs::DirEntry, cutoff: SystemTime) -> bool {
    e.metadata()
        .ok()
        .filter(fs::Metadata::is_file)
        .and_then(|m| m.modified().ok())
        .is_some_and(|m| m < cutoff)
}

#[cfg(test)]
mod tests;
