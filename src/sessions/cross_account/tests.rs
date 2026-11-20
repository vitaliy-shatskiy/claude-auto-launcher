use std::time::Duration;

use super::*;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}
fn tmp(name: &str) -> Tmp {
    let d = std::env::temp_dir().join(format!("cars-cross-{name}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    Tmp(d)
}

/// A transcript `age_s` seconds old, on a fixed clock so no test reads the wall time for its order.
fn transcript(root: &Path, slug: &str, id: &str, age_s: u64, body: &str) -> PathBuf {
    let d = root.join(slug);
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join(format!("{id}.jsonl"));
    std::fs::write(&p, body).unwrap();
    let t = UNIX_EPOCH + Duration::from_secs(1_790_000_000 - age_s);
    std::fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
    p
}

fn baks(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| Path::new(n).extension().is_some_and(|x| x.eq_ignore_ascii_case("bak")))
        .collect()
}

#[test]
fn the_snapshot_folds_every_store_newest_first_and_the_newer_copy_wins() {
    let t = tmp("snap");
    let (a, b) = (t.0.join("a"), t.0.join("b"));
    let own = transcript(&a, "S", "own", 100, "x");
    let dup_old = transcript(&a, "S", "dup", 300, "x");
    let dup_new = transcript(&b, "S", "dup", 50, "x");
    let foreign = transcript(&b, "S", "foreign", 200, "x");
    transcript(&b, "OTHER", "skipped", 1, "x");
    let others = [OtherStore {
        account: "b".into(),
        root: b,
    }];
    let slugs = ["S".to_owned()];
    let got = joined_snapshot(session_files(&a, &slugs), &others, &slugs);
    assert_eq!(got, [dup_new, own, foreign]);
    assert!(!got.contains(&dup_old), "the superseded copy is never offered");
}

#[test]
fn with_no_other_store_the_snapshot_is_the_accounts_own() {
    let primary = vec![PathBuf::from(r"C:\a\S\x.jsonl"), PathBuf::from(r"C:\a\S\y.jsonl")];
    assert_eq!(joined_snapshot(primary.clone(), &[], &[]), primary);
}

#[test]
fn only_a_foreign_session_is_tagged_and_transferred() {
    let others = [OtherStore {
        account: "second".into(),
        root: PathBuf::from(r"C:\m\projects"),
    }];
    let mut rows = [
        Session {
            session_id: "f".into(),
            path: PathBuf::from(r"C:\m\projects\S\f.jsonl"),
            ..Session::default()
        },
        Session {
            session_id: "o".into(),
            path: PathBuf::from(r"C:\o\projects\S\o.jsonl"),
            account: "stale".into(),
            ..Session::default()
        },
    ];
    tag_sessions(&mut rows, &others);
    assert_eq!(rows[0].account, "second");
    assert_eq!(rows[1].account, "", "the launching account's row has no tag");
    assert_eq!(transfer_of(&rows[0]).unwrap().from_account, "second");
    assert_eq!(transfer_of(&rows[1]), None);
}

#[test]
fn a_copy_keeps_the_slug_and_never_overwrites_a_newer_target() {
    let t = tmp("copy");
    let (src_root, dst_root) = (t.0.join("src"), t.0.join("dst"));
    let src = transcript(&src_root, "S", "id", 100, "source");
    let dst = copy_session_to_profile(&src, &dst_root).unwrap();
    assert_eq!(dst, dst_root.join("S").join("id.jsonl"));
    assert_eq!(std::fs::read_to_string(&dst).unwrap(), "source");

    let newer = transcript(&dst_root, "S", "id", 10, "continued here");
    copy_session_to_profile(&src, &dst_root).unwrap();
    assert_eq!(
        std::fs::read_to_string(&newer).unwrap(),
        "continued here",
        "newer target: untouched"
    );
    assert!(baks(&dst_root.join("S")).is_empty(), "nothing to keep when nothing was replaced");
}

#[test]
fn an_older_target_is_kept_as_bak_before_the_copy() {
    let t = tmp("bak");
    let (src_root, dst_root) = (t.0.join("src"), t.0.join("dst"));
    let older = transcript(&dst_root, "S", "id", 100, "continued here");
    let src = transcript(&src_root, "S", "id", 1, "source v2");
    copy_session_to_profile(&src, &dst_root).unwrap();
    assert_eq!(std::fs::read_to_string(&older).unwrap(), "source v2");
    let kept = baks(&dst_root.join("S"));
    assert_eq!(kept.len(), 1, "the older target is kept as .bak");
    assert_eq!(
        std::fs::read_to_string(dst_root.join("S").join(&kept[0])).unwrap(),
        "continued here"
    );
}

fn transfer(source: PathBuf) -> Transfer {
    Transfer {
        session_id: "abcdef0123".into(),
        source,
        from_account: "second".into(),
    }
}

#[test]
fn a_failed_copy_stops_the_launch() {
    let t = tmp("fail");
    let gone = t.0.join("src").join("S").join("abcdef0123.jsonl");
    let err = transfer_before_launch(&transfer(gone), &t.0.join("dst"), "work", false).unwrap_err();
    assert!(err.contains("not launching"), "{err}");
}

#[test]
fn preview_names_the_transfer_and_copies_nothing() {
    let t = tmp("preview");
    let src = transcript(&t.0.join("src"), "S", "abcdef0123", 10, "x");
    let dst = t.0.join("dst");
    let line = transfer_before_launch(&transfer(src), &dst, "work", true).unwrap();
    assert_eq!(
        line,
        "session transfer   : abcdef0123 from second -> work (copied on a real launch)"
    );
    assert!(!dst.exists(), "preview writes nothing");
}

#[test]
fn a_real_launch_copies_and_says_so() {
    let t = tmp("real");
    let src = transcript(&t.0.join("src"), "S", "abcdef0123", 10, "x");
    let dst = t.0.join("dst");
    let line = transfer_before_launch(&transfer(src), &dst, "work", false).unwrap();
    assert_eq!(line, "  session abcdef01 copied: second -> work");
    assert!(dst.join("S").join("abcdef0123.jsonl").is_file());
}
