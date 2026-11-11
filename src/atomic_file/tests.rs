use std::cell::Cell;

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
    write_durable(&file, b"new").unwrap();
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
fn temp_names_differ_when_the_clock_repeats() {
    // Given: two writes in one process that read the same clock tick.
    let p = Path::new(r"C:\x\a.json");
    let (a, b) = (temp_name(p, 7, 0, 0), temp_name(p, 7, 0, 16));
    // Then: the counter alone keeps them apart, and both still read as temps of the target.
    assert_ne!(a, b);
    assert!(
        [&a, &b]
            .iter()
            .all(|t| is_temp_of(&t.file_name().unwrap().to_string_lossy(), "a.json"))
    );
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

#[test]
fn the_flush_runs_before_the_rename_and_its_failure_keeps_the_target() {
    // Given: a target and a flush that fails, noting what the target held when it ran.
    let t = Tree::new("flush");
    let file = t.0.join("a.json");
    fs::write(&file, "old").unwrap();
    let saw = Cell::new(None);
    // When: the write runs with that flush.
    let r = write_with(&file, b"new", |_| {
        saw.set(Some(fs::read(&file).unwrap()));
        Err(io::Error::other("disk gone"))
    });
    // Then: the flush ran while the target was untouched, and its error stops the rename.
    assert_eq!(saw.take().as_deref(), Some(&b"old"[..]), "the flush ran before the rename");
    assert!(r.is_err());
    assert_eq!(fs::read(&file).unwrap(), b"old");
    assert_eq!(names(&t.0), ["a.json"], "no temp left behind");
}

#[test]
fn a_write_through_a_symlink_keeps_the_link_and_writes_its_target() {
    // Given: a link to a file in another folder.
    let t = Tree::new("symlink");
    fs::create_dir_all(t.0.join("real")).unwrap();
    let (real, link) = (t.0.join("real").join("a.json"), t.0.join("a.json"));
    fs::write(&real, "old").unwrap();
    std::os::windows::fs::symlink_file(&real, &link).expect("a symlink needs Developer Mode or elevation");
    // When: the link is written.
    write_atomic(&link, b"new").unwrap();
    // Then: the link is still a link, its target took the bytes, and no temp is left by the target.
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link survives");
    assert_eq!(fs::read(&real).unwrap(), b"new");
    assert_eq!(names(&t.0.join("real")), ["a.json"]);
}

#[test]
fn a_durable_write_to_a_hardlinked_file_reaches_every_name() {
    // Given: one file under two names.
    let t = Tree::new("hardlink");
    let (a, b) = (t.0.join("a.json"), t.0.join("b.json"));
    fs::write(&a, "a longer old text").unwrap();
    fs::hard_link(&a, &b).unwrap();
    // When: one name is written with shorter bytes.
    write_durable(&a, b"new").unwrap();
    // Then: both names read the new bytes whole, and no temp is left.
    assert_eq!(fs::read(&a).unwrap(), b"new");
    assert_eq!(fs::read(&b).unwrap(), b"new", "the other name still shares the file");
    assert_eq!(names(&t.0), ["a.json", "b.json"]);
}

#[test]
fn a_replace_keeps_hidden_and_system_attributes() {
    // Given: a hidden, system file.
    let t = Tree::new("attrs");
    let file = t.0.join("a.json");
    let kept = 0x2 | 0x4;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .attributes(kept)
        .open(&file)
        .unwrap();
    // When: it is replaced.
    write_durable(&file, b"new").unwrap();
    // Then: the new file carries both attributes.
    assert_eq!(fs::read(&file).unwrap(), b"new");
    assert_eq!(fs::metadata(&file).unwrap().file_attributes() & kept, kept);
}

#[test]
fn access_denied_and_sharing_violations_are_retried_three_times() {
    for code in [5, 32] {
        // Given: an operation that fails with `code` three times, then succeeds.
        let calls = Cell::new(0);
        // When: it runs through the retry.
        let r = retry_transient(|| {
            calls.set(calls.get() + 1);
            if calls.get() <= 3 {
                Err(io::Error::from_raw_os_error(code))
            } else {
                Ok(calls.get())
            }
        });
        // Then: the fourth attempt is the answer.
        assert_eq!(r.unwrap(), 4, "os error {code}");
    }
}

#[test]
fn other_errors_are_answered_at_once_and_a_lasting_lock_after_three_retries() {
    // Given / When: a missing file, then a lock that never lifts.
    let calls = Cell::new(0);
    let missing = retry_transient(|| -> io::Result<()> {
        calls.set(calls.get() + 1);
        Err(io::Error::from_raw_os_error(2))
    });
    assert!(missing.is_err());
    assert_eq!(calls.replace(0), 1, "a missing file is answered at once");
    let held = retry_transient(|| -> io::Result<()> {
        calls.set(calls.get() + 1);
        Err(io::Error::from_raw_os_error(32))
    });
    // Then: the lock is reported after the first try and three retries.
    assert_eq!(held.unwrap_err().raw_os_error(), Some(32));
    assert_eq!(calls.get(), 4);
}

#[test]
fn a_missing_file_with_a_fresh_temp_beside_it_is_being_replaced_not_missing() {
    // Given: no target, but another writer's temp written a moment ago.
    let t = Tree::new("settle");
    let file = t.0.join("a.json");
    fs::write(t.0.join("a.json.42-0f0f0f.tmp"), "{}").unwrap();
    // When / Then: the absence is an error, never "missing".
    assert!(exists_settled(&file).is_err());
}

#[test]
fn a_missing_file_beside_a_day_old_temp_is_missing() {
    let t = Tree::new("settle-old");
    let file = t.0.join("a.json");
    write_at(&t.0.join("a.json.42-0f0f0f.tmp"), "{}", ago(25 * 60));
    assert!(!exists_settled(&file).unwrap());
    fs::write(&file, "{}").unwrap();
    assert!(exists_settled(&file).unwrap());
}
