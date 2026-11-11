//! A whole-file write no concurrent reader sees half of (`write_atomic`: a temp beside the target,
//! then a rename), the name of its temps (`is_temp_of`) and the sweep of temps an exit left behind.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Writes `bytes` to `path` through `<path>.<pid>-<hex>.tmp` in the same folder (a rename never
/// crosses volumes), creating the folder. Day-old temps of `path` are swept first. On an error the
/// target is untouched and the temp removed.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    remove_stale_temps(path, SystemTime::now());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let tmp = temp_path(path);
    let result = fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        // The temp may never have been created; one that cannot go now is swept a day later.
        fs::remove_file(&tmp).unwrap_or_default();
    }
    result
}

/// A temp name unique across processes (the pid) and across this process's threads (the count).
fn temp_path(path: &Path) -> PathBuf {
    // The one process-wide counter the temp names need; nothing else reads it.
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let nonce = (nanos ^ NEXT.fetch_add(1, Ordering::Relaxed).rotate_left(20)) & 0x00FF_FFFF;
    PathBuf::from(format!("{}.{}-{nonce:06x}.tmp", path.display(), std::process::id()))
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
    let (Some(dir), Some(name)) = (target.parent(), target.file_name().map(|n| n.to_string_lossy().into_owned())) else {
        return;
    };
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let Ok(rd) = fs::read_dir(dir) else { return };
    let cutoff = now.checked_sub(Duration::from_hours(24)).unwrap_or(UNIX_EPOCH);
    for e in rd.flatten() {
        if is_temp_of(&e.file_name().to_string_lossy(), &name) && modified_before(&e, cutoff) {
            fs::remove_file(e.path()).unwrap_or_default();
        }
    }
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
mod tests {
    use super::*;

    /// A test folder under `target/tmp`, removed on drop.
    struct Tree(PathBuf);

    impl Tree {
        fn new(tag: &str) -> Self {
            let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let d = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("target/tmp/af-{tag}-{}-{n}", std::process::id()));
            fs::create_dir_all(&d).unwrap();
            Self(d)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap_or_default();
        }
    }

    fn ago(mins: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(mins * 60)
    }

    fn write_at(file: &Path, text: &str, when: SystemTime) {
        fs::write(file, text).unwrap();
        fs::File::options()
            .write(true)
            .open(file)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(when))
            .unwrap();
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut n: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        n.sort();
        n
    }

    #[test]
    fn a_write_replaces_the_file_whole_and_leaves_no_temp() {
        // Given: an existing file in a folder that also holds a sub-folder to create.
        let t = Tree::new("atomic");
        let file = t.0.join("sub").join("a.json");
        write_atomic(&file, b"old").unwrap();
        // When: it is written again.
        write_atomic(&file, b"new").unwrap();
        // Then: the new bytes, and nothing beside them.
        assert_eq!(fs::read(&file).unwrap(), b"new");
        assert_eq!(names(&t.0.join("sub")), ["a.json"]);
    }

    #[test]
    fn a_failed_rename_keeps_the_target_and_removes_the_temp() {
        // Given: the target is a folder, so the rename cannot replace it.
        let t = Tree::new("atomic-fail");
        let target = t.0.join("a.json");
        fs::create_dir_all(&target).unwrap();
        // When / Then: the write fails, the folder stays, no temp is left.
        assert!(write_atomic(&target, b"x").is_err());
        assert!(target.is_dir());
        assert_eq!(names(&t.0), ["a.json"]);
    }

    #[test]
    fn temp_names_never_repeat_within_a_process() {
        let p = Path::new(r"C:\x\a.json");
        let mut seen: Vec<PathBuf> = (0..2000).map(|_| temp_path(p)).collect();
        assert!(seen.iter().all(|t| is_temp_of(&t.file_name().unwrap().to_string_lossy(), "a.json")));
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 2000, "two writes in one process shared a temp");
    }

    #[test]
    fn a_write_sweeps_only_day_old_temps_of_its_own_target() {
        // Given: temps of the target (old, young), an old temp of another file, an odd name.
        let t = Tree::new("atomic-sweep");
        let (old, young) = (ago(25 * 60), ago(60));
        write_at(&t.0.join("a.json.42-0f0f0f.tmp"), "{}", old);
        write_at(&t.0.join("a.json.43-0f0f0f.tmp"), "{}", young);
        write_at(&t.0.join("b.json.42-0f0f0f.tmp"), "{}", old);
        write_at(&t.0.join("a.json.42-zz.tmp"), "{}", old);
        // When: the target is written.
        write_atomic(&t.0.join("a.json"), b"{}").unwrap();
        // Then: only the day-old temp of `a.json` is gone.
        assert_eq!(
            names(&t.0),
            ["a.json", "a.json.42-zz.tmp", "a.json.43-0f0f0f.tmp", "b.json.42-0f0f0f.tmp"]
        );
    }
}
