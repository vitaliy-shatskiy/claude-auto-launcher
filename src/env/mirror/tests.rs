//! Tests of `mirror.rs`, ported from `tests/Test-Mirror.mjs`. Every fixture lives under the temp
//! directory; concurrent writers are injected through `MirrorFs`.

use serde_json::json;

use super::*;

struct Dir(PathBuf);
impl Dir {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("claude-auto-rs-mirror-{tag}-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {
            // A leftover temp directory is litter, not a test failure.
        }
    }
}

fn write_json(p: &Path, v: &Value) {
    std::fs::write(p, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

fn read_json_file(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn mtime(p: &Path) -> SystemTime {
    std::fs::metadata(p).unwrap().modified().unwrap()
}

fn opts() -> Options {
    Options {
        exclude: Vec::new(),
        copy_secrets: true,
    }
}

fn excluding(names: &[&str]) -> Options {
    Options {
        exclude: names.iter().map(|s| (*s).to_owned()).collect(),
        copy_secrets: true,
    }
}

fn no_secrets() -> Options {
    Options {
        exclude: Vec::new(),
        copy_secrets: false,
    }
}

fn pair(tag: &str) -> (Dir, PathBuf, PathBuf) {
    let d = Dir::new(tag);
    let (w, p) = (d.file("work.claude.json"), d.file(".claude.json"));
    (d, w, p)
}

fn keys(v: &Value) -> String {
    v.as_object()
        .map(|m| m.keys().cloned().collect::<Vec<_>>().join(","))
        .unwrap_or_default()
}

/// Wraps the real file system and writes into the target at chosen moments, as another
/// launcher or a session would.
struct Racing {
    target: PathBuf,
    stats: usize,
    mutate_on_stat: Vec<usize>,
    tmp_writes: usize,
    race_tmp_writes: usize,
    fail_rename: bool,
}

impl Racing {
    fn new(target: &Path) -> Self {
        Self {
            target: target.to_path_buf(),
            stats: 0,
            mutate_on_stat: Vec::new(),
            tmp_writes: 0,
            race_tmp_writes: 0,
            fail_rename: false,
        }
    }
    fn concurrent(&self, key: &str, value: String) {
        let mut cur = read_json_file(&self.target);
        cur.as_object_mut().unwrap().insert(key.into(), Value::String(value));
        write_json(&self.target, &cur);
    }
}

impl MirrorFs for Racing {
    fn exists(&mut self, p: &Path) -> bool {
        p.exists()
    }
    fn read(&mut self, p: &Path) -> std::io::Result<String> {
        RealFs.read(p)
    }
    fn stat(&mut self, p: &Path) -> Option<(SystemTime, u64)> {
        self.stats += 1;
        if self.mutate_on_stat.contains(&self.stats) {
            let n = self.stats;
            self.concurrent(
                "someUnrelatedKeyWrittenByAnotherLauncher",
                format!("concurrent-write-{n}-{}", "x".repeat(n * 40)),
            );
        }
        RealFs.stat(p)
    }
    fn write(&mut self, p: &Path, data: &str) -> std::io::Result<()> {
        RealFs.write(p, data)?;
        if p.to_string_lossy().contains(".tmp-") && self.tmp_writes < self.race_tmp_writes {
            self.tmp_writes += 1;
            let n = self.tmp_writes;
            self.concurrent("writtenBySession", format!("session-write-{n}-{}", "y".repeat(n * 50)));
        }
        Ok(())
    }
    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        if self.fail_rename {
            return Err(std::io::Error::other("EPERM fixture"));
        }
        RealFs.rename(from, to)
    }
    fn remove(&mut self, p: &Path) {
        RealFs.remove(p);
    }
}

fn leftovers(d: &Dir) -> usize {
    std::fs::read_dir(&d.0)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        .count()
}

#[test]
fn an_additive_merge_then_a_true_no_op() {
    let (_d, w, p) = pair("additive");
    write_json(
        &w,
        &json!({"projects": {"/repo/one": {"mcpServers": {"alpha": {"command": "alpha-cmd"}}, "disabledMcpServers": ["legacy-google"]}}}),
    );
    write_json(&p, &json!({"projects": {"/repo/one": {"mcpServers": {}}}}));
    let line = run(&w, &p, &mut RealFs, &opts()).unwrap();
    let after = read_json_file(&p);
    assert_eq!(
        after["projects"]["/repo/one"]["mcpServers"]["alpha"]["command"], "alpha-cmd",
        "a server missing from the target is added"
    );
    assert_eq!(
        after["projects"]["/repo/one"]["disabledMcpServers"],
        json!(["legacy-google"]),
        "a disabled server is unioned in too"
    );
    assert!(line.contains("alpha"), "the addition is reported on the one line: {line}");
    let before = mtime(&p);
    assert_eq!(
        run(&w, &p, &mut RealFs, &opts()),
        Ok(String::new()),
        "a second run with nothing new reports nothing"
    );
    assert_eq!(before, mtime(&p), "a second run does not touch the file at all");
}

#[test]
fn malformed_json_is_one_line_and_quotes_nothing() {
    let (_d, w, p) = pair("malformed");
    write_json(&w, &json!({"projects": {"/repo/two": {"mcpServers": {"beta": {}}}}}));
    std::fs::write(&p, "{ this is not json").unwrap();
    let e = run(&w, &p, &mut RealFs, &opts()).unwrap_err();
    assert!(
        e.starts_with("claude-mirror-mcp: ") && e.contains("not valid JSON") && !e.contains('\n'),
        "{e}"
    );
    std::fs::write(&w, "not json at all").unwrap();
    write_json(&p, &json!({"projects": {}}));
    assert!(
        run(&w, &p, &mut RealFs, &opts())
            .unwrap_err()
            .contains("work file is not valid JSON")
    );

    write_json(&w, &json!({"mcpServers": {"graph": {"command": "graph-cmd"}}}));
    std::fs::write(&p, r#"{"oauthAccount":{"token":"fixture-secret-abc123","b":broken}}"#).unwrap();
    let e = run(&w, &p, &mut RealFs, &opts()).unwrap_err();
    assert!(e.contains("target file is not valid JSON"), "{e}");
    assert!(
        !e.contains("c123") && !e.contains("broken") && !e.contains("\"b\""),
        "nothing around the bad token is quoted: {e}"
    );
}

#[test]
fn a_target_changed_once_is_retried_twice_is_refused() {
    let (_d, w, p) = pair("race");
    write_json(&w, &json!({"projects": {"/repo/three": {"mcpServers": {"gamma": {}}}}}));
    write_json(&p, &json!({"projects": {}}));
    let mut once = Racing::new(&p);
    once.mutate_on_stat = vec![2];
    assert!(
        run(&w, &p, &mut once, &opts()).is_ok(),
        "a target that changes ONCE is retried and still succeeds"
    );
    let after = read_json_file(&p);
    assert!(
        after["someUnrelatedKeyWrittenByAnotherLauncher"]
            .as_str()
            .unwrap()
            .starts_with("concurrent-write-2-")
    );
    assert!(
        after["projects"]["/repo/three"]["mcpServers"]["gamma"].is_object(),
        "and still carries the merge"
    );

    write_json(&p, &json!({"projects": {}}));
    let mut twice = Racing::new(&p);
    twice.mutate_on_stat = vec![2, 4];
    let e = run(&w, &p, &mut twice, &opts()).unwrap_err();
    assert!(e.contains("changed twice"), "{e}");
    assert!(
        read_json_file(&p)["projects"].get("/repo/three").is_none(),
        "the merge was never written after the second conflict"
    );
}

#[test]
fn a_write_between_the_tmp_file_and_the_rename_is_kept() {
    let (d, w, p) = pair("tmprace");
    write_json(&w, &json!({"mcpServers": {"graph": {"command": "graph-cmd"}}}));
    write_json(&p, &json!({"userID": "fixture"}));
    let mut one = Racing::new(&p);
    one.race_tmp_writes = 1;
    assert!(run(&w, &p, &mut one, &opts()).is_ok());
    let after = read_json_file(&p);
    assert!(after["writtenBySession"].as_str().unwrap().starts_with("session-write-1-") && after["mcpServers"]["graph"].is_object());
    assert_eq!(leftovers(&d), 0, "no tmp file is left");

    write_json(&p, &json!({"userID": "fixture"}));
    let mut two = Racing::new(&p);
    two.race_tmp_writes = 2;
    assert!(run(&w, &p, &mut two, &opts()).is_err());
    let after = read_json_file(&p);
    assert!(after["writtenBySession"].as_str().unwrap().starts_with("session-write-2-") && after.get("mcpServers").is_none());
    assert_eq!(leftovers(&d), 0, "two such writes abort without renaming over either");

    write_json(&p, &json!({"userID": "fixture"}));
    let mut broken = Racing::new(&p);
    broken.fail_rename = true;
    let e = run(&w, &p, &mut broken, &opts()).unwrap_err();
    assert!(e.contains("could not write target file: EPERM fixture"), "{e}");
    assert_eq!(leftovers(&d), 0, "a failed rename removes its tmp file");
}

#[test]
fn credentials_travel_by_default_and_are_stripped_on_request() {
    let (d, w, p) = pair("secrets");
    write_json(
        &w,
        &json!({"projects": {"/repo/five": {"mcpServers": {
            "stdio": {"command": "stdio-cmd", "args": ["--x"], "env": {"API_TOKEN": "fixture-token"}},
            "web": {"type": "http", "url": "https://mcp.example.invalid", "headers": {"Authorization": "Bearer fixture-token"}},
            "skipped": {"command": "skipped-cmd"}
        }}}}),
    );
    write_json(&p, &json!({"projects": {}}));
    let line = run(&w, &p, &mut RealFs, &excluding(&["skipped"])).unwrap();
    let s = read_json_file(&p)["projects"]["/repo/five"]["mcpServers"].clone();
    assert_eq!(
        s["stdio"]["env"]["API_TOKEN"], "fixture-token",
        "a stdio server is copied WITH its env block by default"
    );
    assert_eq!(s["web"]["headers"]["Authorization"], "Bearer fixture-token");
    assert!(s.get("skipped").is_none(), "a server named in exclude is never copied");
    assert!(!line.contains("not copied"));

    let p2 = d.file("optout.claude.json");
    write_json(&p2, &json!({"projects": {}}));
    let line = run(&w, &p2, &mut RealFs, &no_secrets()).unwrap();
    let s2 = read_json_file(&p2)["projects"]["/repo/five"]["mcpServers"].clone();
    assert!(s2["stdio"].get("env").is_none() && s2["web"].get("headers").is_none());
    assert_eq!(
        s2["web"]["url"], "https://mcp.example.invalid",
        "the stripped http server keeps everything else"
    );
    assert!(
        line.contains("stdio @ five (env not copied)") && line.contains("web @ five (headers not copied)"),
        "{line}"
    );
}

#[test]
fn the_environment_switches_parse_like_the_cli() {
    let o = Options::from_vars(Some("skipped, web"), None);
    assert_eq!((o.exclude, o.copy_secrets), (vec!["skipped".to_owned(), "web".to_owned()], true));
    assert!(!Options::from_vars(Some(""), Some("0")).copy_secrets, "only \"0\" strips");
    assert!(Options::from_vars(None, Some("")).copy_secrets);
}

#[test]
fn user_scope_work_wins_target_only_is_kept_and_nothing_else_changes() {
    let (d, w, p) = pair("user");
    write_json(
        &w,
        &json!({"numStartups": 900, "oauthAccount": {"emailAddress": "work@example.invalid"}, "mcpServers": {
            "graph": {"command": "graph-cmd", "args": ["--stdio"], "env": {"GRAPH_TOKEN": "fixture-user-token"}},
            "search": {"type": "http", "url": "https://search.example.invalid", "headers": {"Authorization": "Bearer fixture-user-token"}},
            "same": {"command": "same-cmd", "args": ["a"], "env": {"K": "v"}}
        }}),
    );
    let before = json!({
        "numStartups": 7,
        "oauthAccount": {"emailAddress": "personal@example.invalid", "accountUuid": "fixture-uuid"},
        "mcpServers": {
            "mine": {"command": "only-here-cmd"},
            "search": {"type": "http", "url": "https://old.example.invalid"},
            "same": {"env": {"K": "v"}, "args": ["a"], "command": "same-cmd"}
        },
        "projects": {"/repo/six": {"mcpServers": {"local": {"command": "local-cmd"}}, "lastCost": 1.5}},
        "userID": "fixture-user-id"
    });
    write_json(&p, &before);
    let line = run(&w, &p, &mut RealFs, &opts()).unwrap();
    let after = read_json_file(&p);
    assert_eq!(after["mcpServers"]["graph"], read_json_file(&w)["mcpServers"]["graph"]);
    assert_eq!(
        after["mcpServers"]["search"]["headers"]["Authorization"],
        "Bearer fixture-user-token"
    );
    assert_eq!(after["mcpServers"]["mine"]["command"], "only-here-cmd");
    assert_eq!(
        keys(&after["mcpServers"]["same"]),
        "env,args,command",
        "a key-order twin is left as it is"
    );
    let (mut got, mut want) = (after.clone(), before.clone());
    got.as_object_mut().unwrap().shift_remove("mcpServers");
    want.as_object_mut().unwrap().shift_remove("mcpServers");
    assert_eq!(
        serde_json::to_string(&got).unwrap(),
        serde_json::to_string(&want).unwrap(),
        "nothing outside mcpServers changes"
    );
    assert_eq!(keys(&after), keys(&before), "the top-level key order is kept");
    assert_eq!(keys(&after["mcpServers"]), "mine,search,same,graph", "a new server goes last");
    assert!(
        line.contains("graph @ user")
            && line.contains("search @ user (updated)")
            && line.contains("mine @ user (only in the target, kept)")
    );
    assert!(
        !line.contains("fixture-user-token") && !line.contains("example.invalid") && !line.contains("-cmd"),
        "names only"
    );
    assert!(!line.contains("same @ user"));

    let stamp = mtime(&p);
    let again = run(&w, &p, &mut RealFs, &opts()).unwrap();
    assert_eq!(stamp, mtime(&p), "a second user-scope run writes nothing");
    assert_eq!(
        again, "mine @ user (only in the target, kept)",
        "and still reports the target-only server, alone"
    );

    let p2 = d.file("empty.claude.json");
    write_json(&p2, &json!({"userID": "fixture-user-id-2"}));
    run(&w, &p2, &mut RealFs, &excluding(&["search"])).unwrap();
    let after2 = read_json_file(&p2);
    assert_eq!(
        (keys(&after2), keys(&after2["mcpServers"])),
        ("userID,mcpServers".to_owned(), "graph,same".to_owned())
    );

    let w3 = d.file("work3.claude.json");
    write_json(&w3, &json!({"userID": "w"}));
    let stamp = mtime(&p);
    run(&w3, &p, &mut RealFs, &opts()).unwrap();
    assert_eq!(
        stamp,
        mtime(&p),
        "a work file with no user-scope servers leaves the target untouched"
    );

    let p4 = d.file("optout.claude.json");
    write_json(
        &p4,
        &json!({"mcpServers": {"graph": {"command": "stale-cmd", "env": {"GRAPH_TOKEN": "target-own-token"}}}}),
    );
    run(&w, &p4, &mut RealFs, &no_secrets()).unwrap();
    let s4 = read_json_file(&p4)["mcpServers"].clone();
    assert_eq!(
        (
            s4["graph"]["command"].clone(),
            s4["graph"]["env"]["GRAPH_TOKEN"].clone(),
            s4["search"].get("headers").is_none()
        ),
        (json!("graph-cmd"), json!("target-own-token"), true)
    );
    let stamp = mtime(&p4);
    run(&w, &p4, &mut RealFs, &no_secrets()).unwrap();
    assert_eq!(stamp, mtime(&p4), "a second copySecrets=false run writes nothing");
}

#[test]
fn names_that_would_reach_a_prototype_are_never_written() {
    let (_d, w, p) = pair("proto");
    std::fs::write(
        &w,
        r#"{"mcpServers":{"__proto__":{"polluted":true},"constructor":{"command":"c"},"graph":{"command":"graph-cmd"}},"projects":{"__proto__":{"mcpServers":{"x":{"command":"x"}}}}}"#,
    )
    .unwrap();
    write_json(&p, &json!({"userID": "fixture"}));
    let line = run(&w, &p, &mut RealFs, &opts()).unwrap();
    let raw = std::fs::read_to_string(&p).unwrap();
    assert_eq!(keys(&read_json_file(&p)["mcpServers"]), "graph");
    assert!(!raw.contains("\"__proto__\"") && !raw.contains("\"constructor\""));
    assert!(line.contains("__proto__ @ user (unsafe name, skipped)") && line.contains("constructor @ user (unsafe name, skipped)"));
}

#[test]
fn project_scope_work_definition_wins() {
    let (d, w, p) = pair("projupdate");
    write_json(
        &w,
        &json!({"projects": {"/repo/seven": {"mcpServers": {
            "tok": {"command": "tok-cmd", "env": {"TOKEN": "NEW"}},
            "same": {"command": "same-cmd", "args": ["a"], "env": {"K": "v"}},
            "skip": {"command": "skip-cmd-new"}
        }}}}),
    );
    write_json(
        &p,
        &json!({"projects": {"/repo/seven": {"mcpServers": {
            "tok": {"command": "tok-cmd", "env": {"TOKEN": "OLD"}},
            "same": {"env": {"K": "v"}, "args": ["a"], "command": "same-cmd"},
            "skip": {"command": "skip-cmd-old"},
            "mine": {"command": "only-here-cmd"}
        }}}}),
    );
    let line = run(&w, &p, &mut RealFs, &excluding(&["skip"])).unwrap();
    let s = read_json_file(&p)["projects"]["/repo/seven"]["mcpServers"].clone();
    assert_eq!(s["tok"]["env"]["TOKEN"], "NEW");
    assert!(line.contains("tok @ seven (updated)") && !line.contains("same @ seven"), "{line}");
    assert_eq!(keys(&s["same"]), "env,args,command");
    assert_eq!(
        (s["skip"]["command"].clone(), s["mine"]["command"].clone()),
        (json!("skip-cmd-old"), json!("only-here-cmd"))
    );
    let stamp = mtime(&p);
    assert_eq!(run(&w, &p, &mut RealFs, &excluding(&["skip"])), Ok(String::new()));
    assert_eq!(stamp, mtime(&p), "a project scope that agrees up to key order is not written");

    let (w3, p3) = (d.file("w3.json"), d.file("p3.json"));
    write_json(
        &w3,
        &json!({"projects": {"/repo/eight": {"mcpServers": {"srv": {"command": "srv-cmd", "args": ["--new"], "env": {"TOKEN": "WORK"}}}}}}),
    );
    write_json(
        &p3,
        &json!({"projects": {"/repo/eight": {"mcpServers": {"srv": {"command": "srv-cmd", "args": ["--old"], "env": {"TOKEN": "TARGET"}}}}}}),
    );
    let line = run(&w3, &p3, &mut RealFs, &no_secrets()).unwrap();
    let s3 = read_json_file(&p3)["projects"]["/repo/eight"]["mcpServers"]["srv"].clone();
    assert_eq!(
        (s3["args"].clone(), s3["env"]["TOKEN"].clone()),
        (json!(["--new"]), json!("TARGET"))
    );
    assert!(line == "srv @ eight (updated)", "{line}");
    let stamp = mtime(&p3);
    run(&w3, &p3, &mut RealFs, &no_secrets()).unwrap();
    assert_eq!(
        stamp,
        mtime(&p3),
        "a second copySecrets=false run on a differing env writes nothing"
    );
}

#[test]
fn basename_follows_node_win32() {
    assert_eq!(basename("/repo/seven"), "seven");
    assert_eq!(basename(r"C:\Users\x\proj\"), "proj");
    assert_eq!(basename("C:/a/b"), "b");
}
