//! Tests of `sharing.rs` (Test-Env.ps1 "profile roots and the shared-file links", "CLAUDE.md",
//! "a shared directory missing from the CANONICAL root", the bracket and failed-relink blocks).
//! Fake roots under the temp directory only; icacls is a recording fake.

use std::os::windows::fs::OpenOptionsExt as _;
use std::time::Duration;

use super::*;

struct Tree(PathBuf);
impl Tree {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("claude-auto-rs-share-{tag}-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn dir(&self, rel: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {
            // A leftover temp directory is litter, not a test failure.
        }
    }
}

fn write(p: &Path, text: &str) {
    std::fs::write(p, text).unwrap();
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

/// Write time moved by `hours` from now (negative = older).
fn age(p: &Path, hours: i64) {
    let now = SystemTime::now();
    let d = Duration::from_secs(hours.unsigned_abs() * 3600);
    let t = if hours < 0 { now - d } else { now + d };
    std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
}

fn ids(paths: &[PathBuf]) -> usize {
    let mut v: Vec<String> = paths.iter().map(|p| win_fs::file_id(p).unwrap()).collect();
    v.sort();
    v.dedup();
    v.len()
}

fn texts(n: &[Note]) -> Vec<String> {
    n.iter().map(|x| x.text.clone()).collect()
}

fn count(n: &[Note], needle: &str) -> usize {
    n.iter().filter(|x| x.text.contains(needle)).count()
}

#[expect(clippy::unnecessary_wraps, reason = "the icacls seam's signature")]
fn no_icacls(_: &[String]) -> std::io::Result<(i32, String)> {
    Ok((0, String::new()))
}

/// Replaces `p` by a fresh file (an atomic write elsewhere) with `text`, aged `hours`.
fn replace(p: &Path, text: &str, hours: i64) {
    std::fs::remove_file(p).unwrap();
    write(p, text);
    age(p, hours);
}

#[test]
fn profile_roots_are_created_and_share_one_inode() {
    let t = Tree::new("roots");
    let work = t.dir("work");
    write(&work.join("settings.json"), r#"{"theme":"work"}"#);
    let (a, b) = (t.0.join("personal"), t.0.join("shared"));
    let mut notes = Vec::new();
    assert!(new_profile_root(&a, &work, &["settings.json"], &mut notes).unwrap());
    assert!(new_profile_root(&b, &work, &["settings.json"], &mut notes).unwrap());
    assert!(
        !new_profile_root(&b, &work, &["settings.json"], &mut notes).unwrap(),
        "creating it again is a no-op"
    );
    assert_eq!(
        ids(&[work.join("settings.json"), a.join("settings.json"), b.join("settings.json")]),
        1
    );
    assert_eq!(count(&notes, "linked settings.json into personal"), 1);
    assert_eq!(count(&notes, "created profile root"), 2);
}

fn three_roots(t: &Tree) -> (Vec<PathBuf>, PathBuf) {
    let work = t.dir("work");
    write(&work.join("settings.json"), r#"{"theme":"work"}"#);
    let roots = vec![work.clone(), t.0.join("personal"), t.0.join("shared")];
    for r in &roots[1..] {
        new_profile_root(r, &work, &["settings.json"], &mut Vec::new()).unwrap();
    }
    let third = roots[2].join("settings.json");
    (roots, third)
}

fn files(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots.iter().map(|r| r.join("settings.json")).collect()
}

#[test]
fn a_drifted_third_copy_is_relinked_from_the_newest() {
    let t = Tree::new("drift");
    let (roots, third) = three_roots(&t);
    replace(&third, r#"{"theme":"drifted"}"#, -2);
    let n = repair_link("settings.json", &roots);
    assert_eq!(ids(&files(&roots)), 1, "a drifted third copy is re-linked");
    assert_eq!(read(&third), r#"{"theme":"work"}"#, "the newest copy won");
    assert!(with_suffix(&third, ".pre-relink").exists(), "the discarded copy is kept beside it");
    assert_eq!(count(&n, "re-linked settings.json in shared"), 1);

    replace(&third, r#"{"theme":"newest"}"#, 2);
    repair_link("settings.json", &roots);
    assert_eq!(ids(&files(&roots)), 1);
    assert_eq!(
        read(&roots[0].join("settings.json")),
        r#"{"theme":"newest"}"#,
        "the newest copy wins across every root"
    );
}

#[test]
fn a_copy_differing_only_in_volatile_keys_stays_separate() {
    let t = Tree::new("volatile");
    let (roots, third) = three_roots(&t);
    replace(&third, r#"{"theme":"work"}"#, 0);
    repair_link("settings.json", &roots);
    let work_file = roots[0].join("settings.json");
    let model_only = r#"{"effortLevel":"xhigh","model":"own","theme":"work"}"#;
    replace(&third, model_only, 3);
    repair_link("settings.json", &roots);
    assert_eq!(ids(&files(&roots)), 2, "a NEWEST model-only copy is not pushed onto the others");
    assert_eq!(read(&work_file), r#"{"theme":"work"}"#);
    assert_eq!(read(&third), model_only, "and it keeps its own model");
    age(&third, -3);
    repair_link("settings.json", &roots);
    assert_eq!(read(&third), model_only, "an OLDER model-only copy is not relinked");
    replace(&third, r#"{"advisorModel":"own","theme":"work"}"#, -3);
    repair_link("settings.json", &roots);
    assert_ne!(
        win_fs::file_id(&third),
        win_fs::file_id(&work_file),
        "an advisorModel-only copy is not relinked"
    );
    replace(&third, r#"{"model":"own","theme":"dark"}"#, -3);
    repair_link("settings.json", &roots);
    assert_eq!(ids(&files(&roots)), 1, "a model change plus another key is still relinked");
    assert_eq!(read(&third), r#"{"theme":"work"}"#);
}

#[test]
fn missing_roots_are_skipped_silently() {
    let t = Tree::new("missing");
    let (roots, _) = three_roots(&t);
    let never = t.0.join("never-created");
    assert_eq!(
        repair_link("settings.json", &[roots[0].clone(), roots[1].clone(), never.clone()]),
        Vec::<Note>::new()
    );
    assert_eq!(
        repair_junction("projects", &roots[0], &never, &mut no_icacls).unwrap(),
        Vec::<Note>::new()
    );
    assert!(
        !roots[0].join("projects").exists(),
        "a missing secondary root creates nothing in the canonical root"
    );
}

#[test]
fn a_bracket_root_relinks_and_a_second_drift_rotates_the_backup() {
    let t = Tree::new("brackets");
    let (work, sec) = (t.dir(r"br[x]dir\work"), t.dir(r"br[x]dir\personal"));
    let roots = vec![work.clone(), sec.clone()];
    write(&work.join("settings.json"), r#"{"theme":"work"}"#);
    write(&sec.join("settings.json"), r#"{"theme":"drifted"}"#);
    age(&sec.join("settings.json"), -2);
    repair_link("settings.json", &roots);
    assert_eq!(ids(&files(&roots)), 1, "a drifted copy under a [bracket] root is re-linked");
    let backup = sec.join("settings.json.pre-relink");
    let first = read(&backup);
    assert_eq!(first, r#"{"theme":"drifted"}"#);

    replace(&sec.join("settings.json"), r#"{"theme":"drifted-again"}"#, -1);
    repair_link("settings.json", &roots);
    let rotated: Vec<PathBuf> = std::fs::read_dir(&sec)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| leaf(p).starts_with("settings.json.pre-relink."))
        .collect();
    assert_eq!(rotated.len(), 1, "a second drift rotates the previous backup aside");
    assert_eq!(read(&rotated[0]), first, "so the FIRST losing copy is still readable");
    assert_eq!(read(&backup), r#"{"theme":"drifted-again"}"#);

    t.dir(r"br[x]dir\work\projects");
    t.dir(r"br[x]dir\personal\projects");
    let n = repair_junction("projects", &work, &sec, &mut no_icacls).unwrap();
    assert_eq!(
        count(&n, "not a junction"),
        1,
        "a real directory under a [bracket] root is reported"
    );
}

#[test]
fn a_relink_that_cannot_be_backed_up_leaves_the_file_alone() {
    let t = Tree::new("nobackup");
    let (work, sec) = (t.dir("a\\work"), t.dir("a\\personal"));
    write(&work.join("settings.json"), r#"{"theme":"work"}"#);
    write(&sec.join("settings.json"), r#"{"theme":"drifted"}"#);
    age(&sec.join("settings.json"), -2);
    let who = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap(), std::env::var("USERNAME").unwrap());
    let sec_s = sec.display().to_string();
    real_icacls(&[sec_s.clone(), "/deny".into(), format!("{who}:(WD)")]).unwrap();
    let n = repair_link("settings.json", &[work, sec.clone()]);
    real_icacls(&[sec_s, "/remove:d".into(), who]).unwrap();
    assert_eq!(read(&sec.join("settings.json")), r#"{"theme":"drifted"}"#);
    assert_eq!(count(&n, "could not back up"), 1, "{:?}", texts(&n));
}

#[test]
fn a_relink_refused_mid_way_leaves_the_file_in_place() {
    let t = Tree::new("locked");
    let (work, sec) = (t.dir("b\\work"), t.dir("b\\personal"));
    write(&work.join("settings.json"), r#"{"theme":"work"}"#);
    let f = sec.join("settings.json");
    write(&f, r#"{"theme":"drifted"}"#);
    age(&f, -2);
    // A handle whose share mode omits Delete: a sharing violation no privilege bypasses.
    let lock = std::fs::File::options().read(true).share_mode(1).open(&f).unwrap();
    let n = repair_link("settings.json", &[work, sec]);
    drop(lock);
    assert_eq!(read(&f), r#"{"theme":"drifted"}"#, "its content is untouched");
    assert_eq!(count(&n, "could not re-link"), 1, "{:?}", texts(&n));
    assert_eq!(count(&n, "re-linked settings.json"), 0, "nothing claims it was re-linked");
}

struct SymlinkCase {
    /// Held so the folder lives as long as the case.
    _tree: Tree,
    work: PathBuf,
    root: PathBuf,
    canon: PathBuf,
    other: PathBuf,
    link: PathBuf,
}

impl SymlinkCase {
    fn new() -> Self {
        let t = Tree::new("symlink");
        let (work, root) = (t.dir("work"), t.dir("personal"));
        let (canon, other, link) = (work.join("CLAUDE.md"), t.0.join("other.md"), root.join("CLAUDE.md"));
        write(&canon, "# global");
        write(&other, "# other");
        Self {
            _tree: t,
            work,
            root,
            canon,
            other,
            link,
        }
    }
    fn state(&self) -> String {
        match std::fs::symlink_metadata(&self.link) {
            Err(_) => "missing".into(),
            Ok(m) if m.file_type().is_symlink_file() => format!("link|{}", std::fs::read_link(&self.link).unwrap().display()),
            Ok(_) => "file".into(),
        }
    }
    fn linked(&self) -> String {
        format!("link|{}", self.canon.display())
    }
    fn backups(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| leaf(p).starts_with("CLAUDE.md.pre-symlink."))
            .collect()
    }
    fn entries(&self) -> usize {
        std::fs::read_dir(&self.root).unwrap().count()
    }
    fn run(&self) -> Vec<Note> {
        repair_symlink("CLAUDE.md", &self.work, &self.root, &real_symlink)
    }
}

#[test]
fn claude_md_symlink_is_created_repointed_and_left_alone() {
    let c = SymlinkCase::new();
    let n = repair_all(
        &c.work,
        std::slice::from_ref(&c.root),
        SharedSet {
            files: &[],
            symlinks: &["CLAUDE.md"],
            dirs: &[],
        },
        &mut Seams {
            symlink: &real_symlink,
            icacls: &mut no_icacls,
        },
    );
    assert_eq!(
        c.state(),
        c.linked(),
        "a missing CLAUDE.md is created as a symlink to the canonical file"
    );
    assert_eq!(count(&n, "symlinked CLAUDE.md in personal"), 1, "and the creation is announced");
    assert!(c.run().is_empty(), "a correct symlink is left alone, silently");

    std::fs::remove_file(&c.link).unwrap();
    std::os::windows::fs::symlink_file(&c.other, &c.link).unwrap();
    c.run();
    assert_eq!(c.state(), c.linked(), "a symlink to another file is re-pointed");
    assert_eq!(read(&c.other), "# other", "and the wrong target itself is untouched");
}

#[test]
fn an_identical_hardlink_is_saved_and_replaced_a_diverged_file_is_named() {
    let c = SymlinkCase::new();
    std::fs::hard_link(&c.canon, &c.link).unwrap();
    c.run();
    let bk = c.backups();
    assert_eq!(c.state(), c.linked(), "an identical hardlink is replaced by the symlink");
    assert_eq!(bk.len(), 1);
    let stamp = Regex::new(r"^CLAUDE\.md\.pre-symlink\.\d{8}-\d{6}$").unwrap();
    assert!(
        stamp.is_match(&leaf(&bk[0])) && read(&bk[0]) == "# global",
        "its copy is saved as .pre-symlink.<stamp>"
    );
    assert_eq!(read(&c.canon), "# global", "the canonical file is intact");
    std::fs::remove_file(&bk[0]).unwrap();

    std::fs::remove_file(&c.link).unwrap();
    write(&c.link, "# edited by hand");
    let n = c.run();
    assert_eq!(
        (c.state(), read(&c.link)),
        ("file".to_owned(), "# edited by hand".to_owned()),
        "a diverged regular file is left in place"
    );
    assert_eq!(count(&n, "CLAUDE.md in personal differs"), 1, "and named on one warning line");
    assert!(n[0].text.starts_with("  "), "a sharing note is indented");
    assert!(c.backups().is_empty(), "and no backup is taken of it");
}

#[test]
fn no_privilege_or_a_read_only_file_leaves_everything_as_it_was() {
    let c = SymlinkCase::new();
    write(&c.link, "# global");
    let denied = |l: &Path, t: &Path| -> std::io::Result<()> {
        Err(std::io::Error::other(format!(
            "A required privilege is not held by the client: {} -> {}",
            l.display(),
            t.display()
        )))
    };
    let n = repair_symlink("CLAUDE.md", &c.work, &c.root, &denied);
    assert_eq!(count(&n, "could not symlink CLAUDE.md in personal"), 1);
    assert_eq!((c.state(), read(&c.link)), ("file".to_owned(), "# global".to_owned()));
    assert_eq!(c.entries(), 1, "no privilege: no backup and no temp link left");

    let mut perm = std::fs::metadata(&c.link).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&c.link, perm.clone()).unwrap();
    let n = c.run();
    c.run();
    assert_eq!(count(&n, "could not replace CLAUDE.md in personal"), 1, "{:?}", texts(&n));
    assert_eq!(
        c.entries(),
        1,
        "read-only identical file: two runs leave no backup and no temp link"
    );
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test fixture's own file, made writable again"
    )]
    perm.set_readonly(false);
    std::fs::set_permissions(&c.link, perm).unwrap();
    c.run();
    assert_eq!(
        (c.state(), c.backups().len()),
        (c.linked(), 1),
        "an identical regular file is backed up and replaced"
    );
}

#[test]
fn a_missing_canonical_directory_is_put_back_and_junctioned() {
    let t = Tree::new("junction");
    let (work, low) = (t.dir("work"), t.dir("low"));
    let mut calls: Vec<Vec<String>> = Vec::new();
    let mut rec = |a: &[String]| -> std::io::Result<(i32, String)> {
        calls.push(a.to_vec());
        Ok((0, String::new()))
    };
    let n = repair_junction("tasks", &work, &low, &mut rec).unwrap();
    assert!(
        work.join("tasks").is_dir(),
        "a shared directory missing from the canonical root is created there"
    );
    let md = std::fs::symlink_metadata(low.join("tasks")).unwrap();
    assert_ne!(
        md.file_attributes() & super::super::FILE_ATTRIBUTE_REPARSE_POINT,
        0,
        "and the secondary root gets its junction"
    );
    assert_eq!(count(&n, "junction created and shielded: low tasks -> work"), 1);
    assert!(
        calls
            .iter()
            .any(|c| c.contains(&"/L".to_owned()) && c.iter().any(|a| a.ends_with(":(DE)"))),
        "{calls:?}"
    );

    std::fs::remove_dir(work.join("tasks")).unwrap();
    let n = repair_junction("tasks", &work, &low, &mut no_icacls).unwrap();
    assert!(work.join("tasks").is_dir(), "a dangling junction gets its target back");
    write(&low.join(r"tasks\probe.txt"), "x");
    assert!(
        work.join(r"tasks\probe.txt").exists(),
        "and a write through the junction lands in the canonical root"
    );
    assert!(n.is_empty(), "restoring the target prints nothing");

    let gone = t.0.join("no-such-work");
    repair_junction("tasks", &gone, &low, &mut no_icacls).unwrap();
    assert!(!gone.exists(), "a missing canonical ROOT is not created");
    std::fs::remove_dir(low.join("tasks")).unwrap();
}

#[test]
fn the_root_deny_is_added_once_and_a_refusal_is_one_line() {
    let who = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    let (p, r) = (Path::new(r"C:\x\low\tasks"), Path::new(r"C:\x\low"));
    let mut calls: Vec<Vec<String>> = Vec::new();
    let listing = format!(r"C:\x\low {who}:(DENY)(DC)");
    let mut shielded = |a: &[String]| -> std::io::Result<(i32, String)> {
        calls.push(a.to_vec());
        Ok((0, listing.clone()))
    };
    assert_eq!(protect_junction(p, r, &mut shielded), Vec::<Note>::new());
    assert_eq!(calls.len(), 2, "the root deny is not stacked when present: {calls:?}");
    let mut refused = |a: &[String]| -> std::io::Result<(i32, String)> { Ok((if a.len() == 1 { 0 } else { 5 }, String::new())) };
    let n = protect_junction(p, r, &mut refused);
    assert_eq!(count(&n, "shield NOT applied"), 2, "{:?}", texts(&n));
    assert_eq!(count(&n, "icacls exit 5"), 2);
}

#[test]
fn utc_stamp_formats_a_known_instant() {
    let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_445_296);
    assert_eq!(utc_stamp(t), "20261031-111456");
}
