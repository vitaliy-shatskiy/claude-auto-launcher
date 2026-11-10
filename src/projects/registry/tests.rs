use super::*;
use std::fs::FileTimes;

pub(crate) struct Tree(pub PathBuf);

impl Tree {
    pub(crate) fn new(tag: &str) -> Self {
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("cap-rs-{tag}-{}-{n}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        Self(d)
    }
    pub(crate) fn dir(&self, rel: &str) -> String {
        let p = self.0.join(rel);
        fs::create_dir_all(&p).unwrap();
        p.to_string_lossy().into_owned()
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap_or_default();
    }
}

pub(crate) fn ago(mins: u64) -> SystemTime {
    SystemTime::now() - Duration::from_secs(mins * 60)
}

pub(crate) fn write_at(file: &Path, text: &str, when: SystemTime) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
    File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_times(FileTimes::new().set_modified(when))
        .unwrap();
}

fn user(cwd: &str, entry: Option<&str>) -> String {
    let mut v = json!({ "type": "user", "cwd": cwd });
    if let Some(e) = entry {
        v["entrypoint"] = json!(e);
    }
    format!("{{\"type\":\"queue-operation\"}}\n{v}\n")
}

/// `hideHeadlessProjects` false: the git-root exception these tests were written against.
fn build(root: &Path, cache: &Path, temp: &[String], profiles: &[String]) -> Vec<Project> {
    build_registry(&RegistryInput {
        projects_root: root,
        cache_path: cache,
        temp_roots: temp,
        profile_roots: profiles,
        hide_headless: false,
    })
}

#[test]
fn headless_git_root_is_hidden() {
    let t = Tree::new("hlgit");
    let root = t.0.join("projects");
    let repo = t.dir("rules-1");
    let mixed = t.dir("mixed");
    fs::create_dir_all(Path::new(&repo).join(".git")).unwrap();
    write_at(&root.join("Gr").join("g1.jsonl"), &user(&repo, Some("sdk-cli")), ago(1));
    write_at(&root.join("Gr").join("g2.jsonl"), &user(&repo, Some("sdk-ts")), ago(2));
    write_at(&root.join("Mx").join("m1.jsonl"), &user(&mixed, Some("cli")), ago(3));
    let on = build_registry(&RegistryInput {
        projects_root: &root,
        cache_path: &t.0.join("c.json"),
        temp_roots: &[],
        profile_roots: &[],
        hide_headless: true,
    });
    let hidden = |reg: &[Project], slug: &str| reg.iter().find(|p| p.slug == slug).unwrap().hidden;
    assert!(hidden(&on, "Gr"), "only sdk-* transcripts: hidden even in a git root");
    assert!(!hidden(&on, "Mx"), "an interactive session keeps it listed");
    let off = build(&root, &t.0.join("c2.json"), &[], &[]);
    assert!(!hidden(&off, "Gr"), "hideHeadlessProjects false: the git root stays listed");
}

fn slugs(r: &[Project]) -> String {
    r.iter().map(|p| p.slug.as_str()).collect::<Vec<_>>().join(",")
}

#[test]
fn cwd_comes_from_the_transcript_not_the_slug() {
    let t = Tree::new("cwd");
    let real = r"C:\Users\x\Projects\Acme.Dashboard.Web-team-reports";
    let f = t.0.join("C--Users-x-Projects-Acme-Dashboard-Web-team-reports").join("s1.jsonl");
    write_at(&f, &json!({ "type": "user", "cwd": real }).to_string(), ago(1));
    assert_eq!(path_from_transcript(&f).as_deref(), Some(real));
}

#[test]
fn only_resolvable_existing_projects_newest_first_and_cached() {
    let t = Tree::new("reg");
    let a = t.dir("real-a");
    let b = t.dir("real-b");
    let root = t.0.join("projects");
    write_at(&root.join("A").join("s.jsonl"), &user(&a, None), ago(5));
    write_at(&root.join("B").join("s.jsonl"), &user(&b, None), ago(2 * 1440));
    write_at(&root.join("C").join("s.jsonl"), &user(r"C:\no\such\dir\at\all", None), ago(0));
    fs::create_dir_all(root.join("D")).unwrap();
    let cache = t.0.join("cache.json");
    let reg = build(&root, &cache, &[], &[]);
    assert_eq!(slugs(&reg), "A,B");
    assert_eq!(reg[0].path, a);
    assert!(cache.exists(), "cache file written");
    fs::remove_file(root.join("B").join("s.jsonl")).unwrap();
    assert_eq!(slugs(&build(&root, &cache, &[], &[])), "A", "no transcript left drops out");
}

#[test]
fn a_cache_hit_answers_without_rereading() {
    let t = Tree::new("hit");
    let p1 = t.dir("hit1");
    let p2 = t.dir("hit2");
    let root = t.0.join("projects");
    let f = root.join("Hit").join("s.jsonl");
    let when = ago(1);
    write_at(&f, &user(&p1, None), when);
    let cache = t.0.join("c.json");
    assert_eq!(build(&root, &cache, &[], &[])[0].path, p1);
    write_at(&f, &user(&p2, None), when);
    assert_eq!(build(&root, &cache, &[], &[])[0].path, p1, "same mtime: the cache answers");
}

#[test]
fn a_corrupt_cache_rebuilds() {
    let t = Tree::new("corrupt");
    let x = t.dir("x");
    let root = t.0.join("projects");
    write_at(&root.join("X").join("s.jsonl"), &user(&x, None), ago(1));
    let cache = t.0.join("corrupt.json");
    fs::write(&cache, "{").unwrap();
    assert_eq!(build(&root, &cache, &[], &[]).len(), 1);
}

#[test]
fn one_directory_is_one_row_with_every_slug() {
    let t = Tree::new("merge");
    let real = t.dir("Shared");
    let root = t.0.join("projects");
    write_at(&root.join("C--tmp-Shared").join("a.jsonl"), &user(&real, None), ago(60));
    write_at(
        &root.join("C--tmp-Shared-alt").join("b.jsonl"),
        &user(&real.replace('\\', "/"), None),
        ago(120),
    );
    let reg = build(&root, &t.0.join("c.json"), &[], &[]);
    assert_eq!(reg.len(), 1);
    assert_eq!(reg[0].slug, "C--tmp-Shared");
    let mut s = reg[0].slugs.clone();
    s.sort();
    assert_eq!(s, ["C--tmp-Shared", "C--tmp-Shared-alt"]);
    let at = reg[0].last_activity_ms.unwrap();
    assert!(at <= unix_ms(ago(59)) && at >= unix_ms(ago(61)), "the newest activity of the two");
}

#[test]
fn a_drive_root_is_named_and_a_nul_cwd_dropped() {
    let t = Tree::new("drive");
    let root = t.0.join("projects");
    let drive = format!("{}\\", t.0.to_string_lossy().get(..2).unwrap());
    write_at(&root.join("C--").join("d.jsonl"), &user(&drive, None), ago(1));
    write_at(
        &root.join("C--src-nul").join("n.jsonl"),
        r#"{"type":"user","cwd":"C:\\src\\\u0000bad"}"#,
        ago(1),
    );
    let reg = build(&root, &t.0.join("c.json"), &[], &[]);
    assert_eq!(reg.len(), 1);
    assert_eq!(reg[0].path, drive);
    assert_eq!(reg[0].name, drive.trim_end_matches('\\'));
}

#[test]
fn stale_preview_caches_pruned_by_exact_name() {
    let t = Tree::new("prune");
    let now = SystemTime::now();
    let old = now - Duration::from_hours(48);
    let names = [
        ("claude-auto-projects-preview-111.json", old, false),
        ("claude-auto-projects-preview-222.json", now, true),
        ("claude-auto-projects.json", old, true),
        ("claude-auto-projects-preview-333.json.bak", old, true),
        ("claude-auto-projects-preview-notes.json", old, true),
    ];
    for (n, when, _) in names {
        write_at(&t.0.join(n), "{}", when);
    }
    remove_stale_preview_cache_at(&t.0, now);
    for (n, _, kept) in names {
        assert_eq!(t.0.join(n).exists(), kept, "{n}");
    }
}

#[test]
fn scratch_rows_stay_known_but_hidden() {
    let t = Tree::new("scratch");
    let fake_temp = t.dir("faketemp");
    let fake_profile = t.0.join(".claude-x").to_string_lossy().into_owned();
    let real = t.dir(r"work\real");
    let in_temp = t.dir(r"faketemp\claude\slug\session\rules\child");
    let job = t.dir(r".claude-x\jobs\76c3aa80\tmp\w585");
    let root = t.0.join("projects");
    write_at(&root.join("real").join("r.jsonl"), &user(&real, None), ago(1));
    write_at(
        &root.join("intemp").join("i.jsonl"),
        &user(&in_temp.to_uppercase().replace('\\', "/"), None),
        ago(2),
    );
    write_at(&root.join("job").join("j.jsonl"), &user(&job, None), ago(3));
    write_at(
        &root.join("gone").join("g.jsonl"),
        &user(&format!(r"{fake_temp}\gone"), None),
        ago(5),
    );
    let cache = t.0.join("c.json");
    let temp = [fake_temp];
    let prof = [fake_profile];
    for _ in 0..2 {
        let reg = build(&root, &cache, &temp, &prof);
        assert_eq!(slugs(&reg), "real,intemp,job");
        assert_eq!(reg.iter().map(|p| p.hidden).collect::<Vec<_>>(), [false, true, true]);
    }
}

#[test]
fn headless_only_folders_hidden_unless_a_repository_root() {
    let t = Tree::new("hl");
    let root = t.0.join("projects");
    let dirs: Vec<String> = ["hl", "mx", "old", "vs", "gr", "wt"]
        .iter()
        .map(|n| t.dir(&format!("real-{n}")))
        .collect();
    fs::create_dir_all(Path::new(&dirs[4]).join(".git")).unwrap();
    fs::write(Path::new(&dirs[5]).join(".git"), "gitdir: C:/somewhere/.git/worktrees/wt\n").unwrap();
    let w = |slug: &str, name: &str, cwd: &str, e: Option<&str>, m: u64| {
        let f = root.join(slug).join(format!("{name}.jsonl"));
        write_at(&f, &user(cwd, e), ago(m));
        f
    };
    w("Hl", "h1", &dirs[0], Some("sdk-cli"), 1);
    w("Hl", "h2", &dirs[0], Some("sdk-ts"), 2);
    w("Mx", "m1", &dirs[1], Some("cli"), 120);
    let mx_headless = w("Mx", "m2", &dirs[1], Some("sdk-cli"), 4);
    w("Old", "o1", &dirs[2], None, 180);
    w("Vs", "v1", &dirs[3], Some("claude-vscode"), 240);
    w("Gr", "g1", &dirs[4], Some("sdk-cli"), 5);
    w("Wt", "w1", &dirs[5], Some("sdk-cli"), 6);
    let cache = t.0.join("c.json");
    let reg = build(&root, &cache, &[], &[]);
    let hidden: Vec<&str> = reg.iter().filter(|p| p.hidden).map(|p| p.slug.as_str()).collect();
    let listed: Vec<&str> = reg.iter().filter(|p| !p.hidden).map(|p| p.slug.as_str()).collect();
    assert!(hidden.contains(&"Hl") && hidden.contains(&"Wt"));
    assert_eq!(listed, ["Gr", "Mx", "Old", "Vs"], "listed by activity");
    let mx = reg.iter().find(|p| p.slug == "Mx").unwrap().last_activity_ms;
    let ms = |m: u64| unix_ms(ago(m));
    assert!(
        mx.unwrap() <= ms(119),
        "dated by the interactive session, not the newer headless run"
    );
    fs::remove_dir_all(Path::new(&dirs[4]).join(".git")).unwrap();
    let again = build(&root, &cache, &[], &[]);
    assert!(
        !again.iter().find(|p| p.slug == "Gr").unwrap().hidden,
        "git-root answer from the cache"
    );
    assert!(fs::read_to_string(&cache).unwrap().contains("entrypoint|Mx|m2.jsonl|"));
    let when = fs::metadata(&mx_headless).unwrap().modified().unwrap();
    write_at(&mx_headless, &user(&dirs[1], Some("cli")), when);
    let third = build(&root, &cache, &[], &[]);
    assert_eq!(
        third.iter().find(|p| p.slug == "Mx").unwrap().last_activity_ms,
        mx,
        "verdict from the cache"
    );
}

#[test]
fn headless_verdict_reads_head_and_tail() {
    let t = Tree::new("verdict");
    let sdk = "{\"type\":\"user\",\"entrypoint\":\"sdk-cli\"}\n";
    let cli = "{\"type\":\"user\",\"entrypoint\":\"cli\"}\n";
    let pad = format!("{{\"type\":\"assistant\",\"text\":\"{}\"}}\n", "x".repeat(1000)).repeat(200);
    let f = |n: &str, s: String| {
        let p = t.0.join(n);
        fs::write(&p, s).unwrap();
        p
    };
    assert!(!is_headless_transcript(&f("small.jsonl", format!("{sdk}{cli}"))));
    assert!(!is_headless_transcript(&f("big-cli.jsonl", format!("{sdk}{pad}{cli}"))));
    assert!(is_headless_transcript(&f("big-sdk.jsonl", format!("{sdk}{pad}{sdk}"))));
    assert!(is_headless_transcript(&f("py.jsonl", "{\"entrypoint\":\"sdk-py\"}\n".into())));
    assert!(!is_headless_transcript(&f("old.jsonl", "{\"type\":\"user\"}\n".into())));
    assert!(!is_headless_transcript(&t.0.join("no-such.jsonl")));
}

#[test]
fn interactive_slug_dates_and_shows_a_merged_folder() {
    let t = Tree::new("hlmerge");
    let root = t.0.join("projects");
    let plain = t.dir("merge-plain");
    let repo = t.dir("merge-repo");
    fs::create_dir_all(Path::new(&repo).join(".git")).unwrap();
    write_at(&root.join("Pa").join("a1.jsonl"), &user(&plain, Some("cli")), ago(120));
    write_at(
        &root.join("Pb").join("b1.jsonl"),
        &user(&plain.to_uppercase(), Some("sdk-cli")),
        ago(1),
    );
    write_at(&root.join("Ra").join("a1.jsonl"), &user(&repo, Some("cli")), ago(120));
    write_at(
        &root.join("Rb").join("b1.jsonl"),
        &user(&repo.to_uppercase(), Some("sdk-cli")),
        ago(1),
    );
    let reg = build(&root, &t.0.join("c.json"), &[], &[]);
    assert_eq!(reg.len(), 2);
    for p in &reg {
        assert!(!p.hidden, "{}", p.path);
        assert!(p.last_activity_ms.unwrap() <= unix_ms(ago(119)), "dated by the interactive slug");
    }
}

#[test]
fn a_headless_share_cwd_is_never_probed_and_not_hidden() {
    let t = Tree::new("net");
    let root = t.0.join("projects");
    write_at(
        &root.join("Net").join("n1.jsonl"),
        &user("//claude-auto-no-such-host-hl/share/repo", Some("sdk-cli")),
        ago(1),
    );
    let reg = build(&root, &t.0.join("c.json"), &[], &[]);
    assert_eq!(reg.len(), 1);
    assert!(!reg[0].hidden);
}
