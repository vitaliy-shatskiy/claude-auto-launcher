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

#[expect(clippy::unnecessary_wraps, reason = "the Lister signature")]
fn never_answers(_: &str) -> Result<Vec<String>, String> {
    std::thread::sleep(Duration::from_secs(30));
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
    let t0 = std::time::Instant::now();
    n.tab();
    assert!(t0.elapsed() < Duration::from_secs(5), "Tab waited for the share");
    assert!(n.notice.contains("no answer in 30 ms"), "{}", n.notice);
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
