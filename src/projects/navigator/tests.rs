use super::*;
use crate::projects::registry::tests::Tree;
use std::sync::atomic::{AtomicUsize, Ordering};

fn nav() -> Navigator {
    Navigator::new(list_dirs, NETWORK_LIMIT)
}

fn typed(n: &mut Navigator, s: &str) {
    for c in s.chars() {
        n.type_char(c);
    }
    n.refresh();
}

fn folders(n: &Navigator) -> Vec<String> {
    n.rows
        .iter()
        .filter_map(|r| if let NavRow::Folder(f) = r { Some(f.clone()) } else { None })
        .collect()
}

/// `root\{alpha, alpine, beta}`; the root as typed, with its trailing `\`.
fn tree(name: &str) -> (Tree, String) {
    let t = Tree::new(name);
    for d in [r"root\alpha", r"root\alpine", r"root\beta"] {
        t.dir(d);
    }
    let root = format!(r"{}\root\", t.0.display());
    (t, root)
}

#[test]
fn a_large_parent_is_read_only_up_to_the_cap() {
    // Given: a parent with five folders and a file.
    let t = Tree::new("nav-cap");
    for d in ["a", "b", "c", "d", "e"] {
        t.dir(d);
    }
    std::fs::write(t.0.join("f.txt"), "").unwrap();
    let parent = t.0.to_string_lossy().into_owned();
    // When: it is read with a cap of three.
    let capped = list_dirs_capped(&parent, 3).unwrap();
    // Then: three folders (the file never counts), and the real cap reads them all.
    assert_eq!(capped.len(), 3, "{capped:?}");
    assert_eq!(list_dirs(&parent).unwrap(), ["a", "b", "c", "d", "e"]);
}

#[test]
fn an_empty_field_lists_the_drives_present() {
    let mut n = nav();
    n.refresh();
    let here = env!("CARGO_MANIFEST_DIR").chars().next().unwrap().to_ascii_uppercase();
    assert!(folders(&n).contains(&format!("{here}:")), "{:?}", n.rows);
    assert!(!n.rows.iter().any(|r| matches!(r, NavRow::Use(_) | NavRow::Up)));
}

#[test]
fn the_list_is_the_parents_folders_filtered_by_the_last_segment() {
    let (_t, root) = tree("nav-list");
    let mut n = nav();
    typed(&mut n, &root);
    assert_eq!(n.rows[..2], [NavRow::Use(root.trim_end_matches('\\').to_owned()), NavRow::Up]);
    assert_eq!(folders(&n), ["alpha", "alpine", "beta"]);
    typed(&mut n, "ET");
    assert_eq!(folders(&n), ["beta"], "case-insensitive, a held segment counts");
    let mut q = nav();
    typed(&mut q, &format!("\"{}\"", root.replace('\\', "/")));
    assert_eq!(folders(&q), ["alpha", "alpine", "beta"], "a pasted path with quotes and slashes");
}

#[test]
fn tab_completes_one_and_cycles_several() {
    let (_t, root) = tree("nav-tab");
    let mut n = nav();
    typed(&mut n, &format!("{root}b"));
    n.tab();
    assert_eq!(n.text, format!(r"{root}beta\"), "one candidate is entered");
    let mut m = nav();
    typed(&mut m, &format!("{root}alp"));
    m.tab();
    assert_eq!(m.text, format!("{root}alpha"));
    m.refresh();
    m.tab();
    assert_eq!(m.text, format!("{root}alpine"), "the second Tab cycles");
    m.tab();
    assert_eq!(m.text, format!("{root}alpha"));
}

#[test]
fn backspace_goes_up_on_an_empty_segment_and_ctrl_drops_a_segment() {
    let (_t, root) = tree("nav-bksp");
    let mut n = nav();
    typed(&mut n, &format!(r"{root}alpha\"));
    n.backspace(false);
    assert_eq!(n.text, root, "an empty segment: one level up");
    typed(&mut n, "alp");
    n.backspace(false);
    assert_eq!(n.text, format!("{root}al"));
    n.backspace(true);
    assert_eq!(n.text, root, "Ctrl drops the segment");
    let mut d = nav();
    typed(&mut d, r"C:\");
    d.backspace(false);
    assert_eq!(d.text, "", "above a drive root: the drive list");
}

#[test]
fn enter_opens_a_folder_up_goes_back_and_use_picks_an_existing_one() {
    let (_t, root) = tree("nav-enter");
    let mut n = nav();
    typed(&mut n, &root);
    let beta = n.rows.iter().position(|r| *r == NavRow::Folder("beta".into())).unwrap();
    assert_eq!(n.enter(beta), NavStep::Stay);
    assert_eq!(n.text, format!(r"{root}beta\"));
    n.refresh();
    assert_eq!(n.enter(0), NavStep::Pick(format!("{root}beta")));
    assert_eq!(n.enter(1), NavStep::Stay);
    assert_eq!(n.text, root, "the .. row");
}

#[test]
fn a_missing_folder_is_created_only_after_the_confirm_and_one_level_only() {
    let (t, root) = tree("nav-create");
    let fresh = format!("{root}fresh");
    let mut n = nav();
    typed(&mut n, &fresh);
    assert_eq!(n.enter(0), NavStep::Stay);
    assert_eq!(n.confirm.as_deref(), Some(fresh.as_str()));
    assert!(n.status().contains("enter yes / esc no"));
    assert!(!Path::new(&fresh).exists(), "nothing before the confirm");
    assert!(n.escape(), "Esc drops the question");
    assert!(!Path::new(&fresh).exists());
    assert_eq!(n.enter(0), NavStep::Stay);
    assert_eq!(n.enter(0), NavStep::Pick(fresh.clone()), "the second Enter is the yes");
    assert!(Path::new(&fresh).is_dir());
    let deep = format!(r"{root}none\deeper");
    let mut d = nav();
    typed(&mut d, &deep);
    assert_eq!(d.enter(0), NavStep::Stay);
    assert_eq!(
        (d.confirm.as_deref(), d.notice.as_str()),
        (None, "path not found"),
        "never two levels"
    );
    let mut e = nav();
    typed(&mut e, &format!("{root}other"));
    assert_eq!(e.enter(0), NavStep::Stay);
    assert!(e.confirm.is_some());
    e.type_char('x');
    assert!(e.confirm.is_none(), "typing drops the question");
    drop(t);
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

#[expect(clippy::unnecessary_wraps, reason = "the Lister signature")]
fn counting(_: &str) -> Result<Vec<String>, String> {
    CALLS.fetch_add(1, Ordering::SeqCst);
    Ok(vec!["one".into(), "two".into()])
}

static NEVER_GATE: (std::sync::Mutex<bool>, std::sync::Condvar) = (std::sync::Mutex::new(false), std::sync::Condvar::new());

/// A share that never answers while the test runs; bounded, so a Tab that waited for it would get
/// an empty answer after 3 s and no notice instead of hanging the run.
#[expect(clippy::unnecessary_wraps, reason = "the Lister signature")]
fn never_answers(_: &str) -> Result<Vec<String>, String> {
    let (lock, cv) = &NEVER_GATE;
    drop(
        cv.wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(3), |open| !*open)
            .unwrap(),
    );
    Ok(Vec::new())
}

#[test]
fn a_network_folder_is_listed_only_on_tab_and_used_unprobed() {
    let share = r"\\claude-auto-no-such-host-604\share\";
    let mut n = Navigator::new(counting, Duration::from_secs(5));
    typed(&mut n, share);
    assert_eq!(CALLS.load(Ordering::SeqCst), 0, "typing never lists a network folder");
    assert!(n.hint.contains("tab lists it"), "{}", n.hint);
    n.tab();
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(n.text, format!("{share}one"), "Tab shows the first candidate");
    n.backspace(true);
    n.refresh();
    assert_eq!(folders(&n), ["one", "two"], "the Tab listing stays for this parent");
    assert_eq!(CALLS.load(Ordering::SeqCst), 1, "and is not asked again");
    let mut u = Navigator::new(counting, Duration::from_secs(5));
    typed(&mut u, r"\\claude-auto-no-such-host-604\share\missing");
    assert_eq!(
        u.enter(0),
        NavStep::Pick(r"\\claude-auto-no-such-host-604\share\missing".into()),
        "a network path is never probed and never offered for creation"
    );
    let mut q = Navigator::new(counting, Duration::from_secs(5));
    q.classify = |_| DriveClass::Skip;
    typed(&mut q, r"Q:\src\");
    assert!(q.hint.contains("tab lists it"), "a network drive letter as well");
}

#[test]
fn an_offline_share_costs_one_time_limit_never_a_hang() {
    let mut n = Navigator::new(never_answers, Duration::from_millis(30));
    typed(&mut n, r"\\claude-auto-offline-604\share\");
    n.tab();
    assert!(n.notice.contains("no answer in 30 ms"), "Tab waited for the share: {}", n.notice);
    n.refresh();
    assert_eq!(folders(&n), Vec::<String>::new());
    assert!(n.status().contains("no answer"), "the notice outlives the next frame");
}

fn denied(_: &str) -> Result<Vec<String>, String> {
    Err("cannot list it: permission denied".into())
}

#[test]
fn a_folder_that_cannot_be_listed_says_so_and_still_offers_use_and_up() {
    let mut n = Navigator::new(denied, NETWORK_LIMIT);
    typed(&mut n, r"C:\locked\");
    assert_eq!(n.rows, [NavRow::Use(r"C:\locked".into()), NavRow::Up]);
    assert!(n.status().contains("permission denied"));
}

#[test]
fn a_listing_error_stays_on_the_next_frames() {
    let mut n = Navigator::new(denied, NETWORK_LIMIT);
    typed(&mut n, r"C:\locked\");
    n.refresh();
    n.refresh();
    assert!(n.status().contains("permission denied"), "{}", n.status());
}

static RELATIVE_CALLS: AtomicUsize = AtomicUsize::new(0);

#[expect(clippy::unnecessary_wraps, reason = "the Lister signature")]
fn counting_relative(_: &str) -> Result<Vec<String>, String> {
    RELATIVE_CALLS.fetch_add(1, Ordering::SeqCst);
    Ok(vec!["one".into()])
}

#[test]
fn only_a_full_path_is_listed() {
    for t in [r"\Windows\", r"src\", r"C:src\", r"..\"] {
        let mut n = Navigator::new(counting_relative, NETWORK_LIMIT);
        typed(&mut n, t);
        n.tab();
        assert_eq!(RELATIVE_CALLS.load(Ordering::SeqCst), 0, "{t} was listed");
        assert!(n.status().contains("type a full path"), "{t}: {}", n.status());
    }
    let mut n = Navigator::new(counting_relative, NETWORK_LIMIT);
    typed(&mut n, r"C:\x\");
    assert_eq!(RELATIVE_CALLS.load(Ordering::SeqCst), 1, "a full local path is listed");
}

#[test]
fn a_symlink_to_a_file_is_not_a_folder() {
    let (t, root) = tree("nav-link");
    let file = t.0.join(r"root\file.txt");
    std::fs::write(&file, "x").unwrap();
    std::os::windows::fs::symlink_file(&file, t.0.join(r"root\to-file")).unwrap();
    std::os::windows::fs::symlink_dir(t.0.join(r"root\alpha"), t.0.join(r"root\to-dir")).unwrap();
    assert_eq!(list_dirs(&root).unwrap(), ["alpha", "alpine", "beta", "to-dir"]);
}

static SLOW_CALLS: AtomicUsize = AtomicUsize::new(0);
/// Opened by the test: the slow share answers only then.
static SLOW_GATE: (std::sync::Mutex<bool>, std::sync::Condvar) = (std::sync::Mutex::new(false), std::sync::Condvar::new());

#[expect(clippy::unnecessary_wraps, reason = "the Lister signature")]
fn slow_share(_: &str) -> Result<Vec<String>, String> {
    SLOW_CALLS.fetch_add(1, Ordering::SeqCst);
    let (lock, cv) = &SLOW_GATE;
    let mut open = lock.lock().unwrap();
    while !*open {
        open = cv.wait(open).unwrap();
    }
    Ok(vec!["late".into()])
}

#[test]
fn a_slow_share_keeps_one_worker_and_its_late_answer_is_used() {
    let share = r"\\claude-auto-slow-604\share\";
    let mut n = Navigator::new(slow_share, Duration::from_millis(10));
    typed(&mut n, share);
    n.tab();
    assert!(n.notice.contains("no answer"), "{}", n.notice);
    n.notice.clear();
    n.refresh();
    n.tab();
    assert_eq!(SLOW_CALLS.load(Ordering::SeqCst), 1, "a second Tab waits on the same worker");
    let (lock, cv) = &SLOW_GATE;
    *lock.lock().unwrap() = true;
    cv.notify_all();
    n.limit = Duration::from_secs(5);
    n.notice.clear();
    n.tab();
    assert_eq!(n.text, format!(r"{share}late\"), "the late answer completes");
    assert_eq!(SLOW_CALLS.load(Ordering::SeqCst), 1);
}
