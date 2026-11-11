use std::os::windows::fs::OpenOptionsExt as _;
use std::path::PathBuf;
use std::sync::mpsc;

use super::*;
use crate::launch::tests::four;

/// `FILE_SHARE_DELETE`: a handle that lets the file be replaced but not read.
const SHARE_DELETE_ONLY: u32 = 0x4;

/// The other accounts' profiles a save must never lose.
const OTHERS: &str = r#"{"Version":2,"Account":"personal","Profiles":{"personal":{"Model":"fable"},"low":{"Model":"haiku"}}}"#;

/// A throwaway directory of this test: its prefs path, and through it its own mutex name.
struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let d = std::env::temp_dir().join(format!("claude-auto-rs-prefs-{tag}-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&d).unwrap();
        Self(d)
    }

    fn prefs(&self) -> String {
        let p = self.0.join("p.json").to_string_lossy().into_owned();
        std::fs::write(&p, OTHERS).unwrap();
        p
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

fn work_save(path: &str) {
    let r = four();
    let mut s = r.new_state();
    s.effort = "max".into();
    save_prefs(path, &s, 60_000, &r);
}

#[test]
fn v1_file_migrates_to_its_own_account() {
    let p = parse_prefs(r#"{"Account":"personal","Model":"fable","Effort":"max","SavedAtMs":1000}"#);
    assert_eq!(p.account.as_deref(), Some("personal"));
    let e = p.profiles.get("personal").and_then(Value::as_object).unwrap();
    assert_eq!(e.get("Model"), Some(&json!("fable")));
    assert_eq!(e.get("SavedAtMs"), Some(&json!(1000)));
}

#[test]
fn v1_without_account_is_work() {
    let p = parse_prefs(r#"{"Effort":"high"}"#);
    assert!(p.profiles.contains_key("work"));
}

#[test]
fn future_version_and_corrupt_files_read_empty() {
    assert_eq!(parse_prefs(r#"{"Version":3,"Profiles":{"work":{"Model":"x"}}}"#), Prefs::default());
    assert_eq!(parse_prefs("{not json"), Prefs::default());
    assert_eq!(parse_prefs(r#"{"Version":"two"}"#), Prefs::default());
}

#[test]
fn save_keeps_other_profiles_and_previous_answer_for_default() {
    let r = four();
    let prev =
        parse_prefs(r#"{"Version":2,"Account":"work","Profiles":{"work":{"Model":"fable","Effort":"default"},"low":{"Model":"haiku"}}}"#);
    let mut s = r.new_state();
    s.remote = "stop server".into();
    let d = build_prefs_data(&prev, &s, 5, &r);
    assert_eq!(d["Profiles"]["low"]["Model"], "haiku");
    assert_eq!(d["Profiles"]["work"]["Model"], "fable", "default says nothing");
    assert!(d["Profiles"]["work"].get("Effort").is_none(), "a literal default is dropped");
    assert_eq!(d["Profiles"]["work"]["Remote"], "off");
    assert_eq!(d["Account"], "work");
}

#[test]
fn save_round_trips_through_the_file_atomically() {
    let r = four();
    let dir = std::env::temp_dir().join(format!("claude-auto-rs-prefs-{}", crate::log::new_run_id()));
    let path = dir.join("p.json").to_string_lossy().into_owned();
    let mut s = r.new_state();
    s.effort = "max".into();
    save_prefs(&path, &s, 60_000, &r);
    let bytes = std::fs::read(&path).unwrap();
    assert_ne!(bytes.get(..3), Some(&[0xEF, 0xBB, 0xBF][..]), "no BOM");
    let back = read_prefs(&path);
    let mut fresh = r.new_state();
    let m = merge_prefs(&mut fresh, &back, &r, 120_000);
    assert_eq!(fresh.effort, "max");
    assert_eq!(m.age_text, "1 min");
    assert!(std::fs::read_dir(&dir).unwrap().count() == 1, "no temp left behind");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn save_writes_nothing_while_another_handle_denies_reading_the_file() {
    let t = Tmp::new("unreadable");
    let path = t.prefs();
    // Share DELETE only: the read fails, yet a rename could still replace the file, so only the
    // guard stands between this save and the loss of the other profiles.
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_DELETE_ONLY)
        .open(&path)
        .unwrap();
    assert!(read_prefs(&path).unreadable, "the held file reads as unreadable");
    work_save(&path);
    drop(held);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), OTHERS, "nothing was written");
    work_save(&path);
    let after = read_prefs(&path);
    assert_eq!(
        (after.profiles.len(), after.account.as_deref()),
        (3, Some("work")),
        "once readable, the same save lands and keeps both other profiles"
    );
}

#[test]
fn an_exclusively_held_file_reads_unreadable_and_keeps_every_profile() {
    let t = Tmp::new("exclusive");
    let path = t.prefs();
    let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
    assert!(read_prefs(&path).unreadable, "the held file reads as unreadable");
    work_save(&path);
    drop(held);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), OTHERS, "other accounts' profiles survive");
    assert_eq!(std::fs::read_dir(&t.0).unwrap().count(), 1, "no temp left behind");
}

#[test]
fn save_writes_nothing_while_another_holder_owns_the_mutex() {
    let t = Tmp::new("mutex");
    let path = t.prefs();
    let name = mutex_name(&path);
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    // Its own thread: a Win32 mutex is re-entrant for the thread that owns it.
    let holder = std::thread::spawn(move || {
        let m = sys::NamedMutex::acquire(&name, 0);
        held_tx.send(m.as_ref().is_some_and(sys::NamedMutex::held)).unwrap();
        release_rx.recv().unwrap();
        drop(m);
    });
    assert!(held_rx.recv().unwrap(), "the second handle owns the mutex");
    work_save(&path);
    release_tx.send(()).unwrap();
    holder.join().unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), OTHERS, "nothing was written");
    work_save(&path);
    assert_eq!(
        read_prefs(&path).account.as_deref(),
        Some("work"),
        "once the mutex is free the same save lands"
    );
}

#[test]
fn one_prefs_file_maps_to_one_pinned_mutex_name() {
    // Get-PrefsMutexName (claude-auto\Prefs.ps1 on main) for the same paths: both launchers must
    // take one mutex for one file.
    assert_eq!(
        mutex_name(r"C:\Fixed\Claude-Auto\Prefs.json"),
        r"Local\claude-auto-prefs-e564250d8bfce5959d68fa4d17724e54"
    );
    assert_eq!(
        mutex_name("C:\\Users\\Тест Юзер\\.claude\\claude-auto-prefs.json"),
        r"Local\claude-auto-prefs-107079c7ae40b054e974bd204c42214c"
    );
}

#[test]
fn merge_ignores_values_not_on_the_row_and_hidden_accounts() {
    let r = four();
    let p = parse_prefs(r#"{"Version":2,"Account":"shared","Profiles":{"work":{"Model":"gpt","Effort":"HIGH"}}}"#);
    let mut s = r.new_state();
    let m = merge_prefs(&mut s, &p, &r, 0);
    assert_eq!(s.account, "work", "a hidden account is not resurrected");
    assert_eq!(s.model, "default");
    assert_eq!(s.effort, "high");
    assert_eq!(m.restored, ["Effort"]);
}

#[test]
fn switch_parks_and_restores_per_tab() {
    let r = four();
    let p = parse_prefs(r#"{"Version":2,"Profiles":{"low":{"Model":"haiku","SavedAtMs":0}}}"#);
    let mut s = r.new_state();
    s.effort = "max".into();
    switch_account(&mut s, "low", &p, &r, 3 * 3_600_000);
    assert_eq!(
        (s.account.as_str(), s.model.as_str(), s.effort.as_str()),
        ("low", "haiku", "default")
    );
    assert_eq!(s.restored_age, "3 h");
    switch_account(&mut s, "work", &p, &r, 0);
    assert_eq!(s.effort, "max", "the stash wins on the way back");
    reset_tab(&mut s, &r);
    assert_eq!(s.effort, "default");
    assert!(!s.profiles.contains_key("work"));
}

#[test]
fn age_text_buckets() {
    assert_eq!(age_text(None, 0), "");
    assert_eq!(age_text(Some(0), 30_000), "just now");
    assert_eq!(age_text(Some(0), 90_000), "2 min");
    assert_eq!(age_text(Some(0), 49 * 3_600_000), "2 d");
}

#[test]
fn mutex_name_is_stable_and_path_free() {
    let a = mutex_name(r"C:\X\p.json");
    assert_eq!(a, mutex_name(r"c:\x\P.JSON"));
    assert!(a.starts_with("Local\\claude-auto-prefs-"));
    assert_eq!(a.len(), "Local\\claude-auto-prefs-".len() + 32);
}
