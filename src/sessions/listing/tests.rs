use super::*;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

fn standalone(p: &Path) -> Session {
    let pp = p.parent().and_then(project_path_from_transcript);
    summarize(p, pp.as_deref(), true).unwrap()
}

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}
fn tmp(tag: &str) -> Tmp {
    let d = std::env::temp_dir().join(format!("cars-w1-l-{tag}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    Tmp(d)
}

fn put(dir: &Path, name: &str, text: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

fn set_mtime(p: &Path, minutes: u64) {
    let t = UNIX_EPOCH + Duration::from_secs(1_790_000_000 + minutes * 60);
    std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
}

fn prompt(text: &str, cwd: &str) -> String {
    format!(
        r#"{{"type":"user","cwd":"{}","message":{{"role":"user","content":"{text}"}}}}"#,
        cwd.replace('\\', "\\\\")
    ) + "\n"
}

#[test]
fn titles_skip_noise_and_sidechains() {
    assert_eq!(standalone(&fixture("noisy-head.jsonl")).title, "rewrite the launcher menu");
    let s = standalone(&fixture("with-sidechain.jsonl"));
    assert_eq!(s.title, "real user prompt");
    assert_eq!(s.last_assistant, "real assistant reply");
    let t = standalone(&fixture("truncated-tail.jsonl"));
    assert_eq!((t.title.as_str(), t.last_assistant.as_str()), ("survives truncation", "fine"));
    let c = standalone(&fixture("slash-command.jsonl"));
    assert_eq!(c.title, "/doctor установка зависла на 40%");
    assert_eq!(c.last_assistant, "Checking the installer.");
    let n = standalone(&fixture("task-notification.jsonl"));
    assert_eq!((n.title.as_str(), n.last_user.as_str()), ("the real question", "the real question"));
}

#[test]
fn recent_messages_are_real_exchanges_oldest_first() {
    let s = standalone(&fixture("counted.jsonl"));
    assert_eq!(s.prompt_count, 3);
    assert_eq!(s.recent.len(), 5);
    assert_eq!(
        s.recent[0],
        Message {
            speaker: Speaker::User,
            text: "first prompt".into()
        }
    );
    assert_eq!(
        s.recent[1],
        Message {
            speaker: Speaker::Assistant,
            text: "ok".into()
        }
    );
    assert_eq!(
        s.recent[4],
        Message {
            speaker: Speaker::Assistant,
            text: "done".into()
        }
    );
    let f = standalone(&fixture("code-fence.jsonl"));
    assert_eq!(f.recent[1].text, "Here: [code: 3 lines] Done.");
}

#[test]
fn subagent_transcripts_are_not_sessions() {
    let t = tmp("tree");
    let root = fixture("tree");
    let rows = SessionCache::default().list(&root, Scope::All, 0, 40, &t.0.join("c.json"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].project, "Demo");
}

#[test]
fn hostile_text_is_cleaned_in_every_field() {
    let t = tmp("esc");
    let lines = [
        "{\"type\":\"user\",\"message\":{\"content\":\"\\u001b]0;pwned\\u0007hello \\u001b[2Jworld\"}}",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"\\u001b[31mred\\u001b[0m reply\\u0007\"}]}}",
        "{\"type\":\"user\",\"cwd\":\"C:\\\\src\\\\pwn\\u001bed\",\"message\":{\"content\":\"second \\u001b]52;c;cGF5bG9hZA==\\u0007prompt\"}}",
    ];
    let p = put(
        &t.0.join("C--Users-someone-Desktop-Projects-EscTest"),
        "cccc3333.jsonl",
        &(lines.join("\n") + "\n"),
    );
    let s = standalone(&p);
    assert_eq!(s.title, "]0;pwned hello [2Jworld");
    let ctl = |x: &str| x.chars().any(char::is_control);
    assert!(!ctl(&s.title) && !ctl(&s.last_user) && !ctl(&s.last_assistant) && !ctl(&s.project));
    assert!(s.recent.iter().all(|m| !ctl(&m.text)));
    assert!(s.last_assistant.contains("red"));
}

#[test]
#[expect(clippy::many_single_char_names, reason = "short fixture names")]
fn project_name_comes_from_the_cwd_and_is_refreshed_on_a_cache_hit() {
    let t = tmp("cwd");
    let dir = t.0.join("C--src-my-cool-app");
    let p = put(&dir, "x.jsonl", &prompt("hello", r"C:\src\my-cool-app"));
    assert_eq!(standalone(&p).project, "my-cool-app");
    assert_eq!(standalone(&p).slug, "C--src-my-cool-app");
    let mut c = SessionCache::default();
    let cache = t.0.join("c.json");
    assert_eq!(c.list(&t.0, Scope::Slug("C--src-my-cool-app"), 0, 40, &cache).len(), 1);
    assert_eq!(c.list(&t.0, Scope::Slug("C--other"), 0, 40, &cache).len(), 0);

    let st = t.0.join("C--src-stale-app");
    let a = put(&st, "aaa.jsonl", r#"{"type":"user","message":{"role":"user","content":"first"}}"#);
    set_mtime(&a, 1);
    let cache2 = t.0.join("c2.json");
    assert_eq!(c.list(&t.0, Scope::Slug("C--src-stale-app"), 0, 40, &cache2)[0].project, "app");
    let b = put(&st, "bbb.jsonl", &prompt("second", r"C:\src\new-name"));
    set_mtime(&b, 2);
    let rows = c.list(&t.0, Scope::Slug("C--src-stale-app"), 0, 40, &cache2);
    assert!(rows.iter().all(|r| r.project == "new-name"), "a cached row refreshes its project");
}

#[test]
fn scoped_and_paged_calls_merge_only_a_survey_prunes() {
    let t = tmp("merge");
    for n in 1..=5 {
        set_mtime(
            &put(
                &t.0.join("C--src-page-a"),
                &format!("a{n}.jsonl"),
                &prompt(&format!("prompt a{n}"), r"C:\src\page-a"),
            ),
            n,
        );
    }
    for n in 1..=3 {
        set_mtime(
            &put(
                &t.0.join("C--src-page-b"),
                &format!("b{n}.jsonl"),
                &prompt(&format!("prompt b{n}"), r"C:\src\page-b"),
            ),
            10 + n,
        );
    }
    let cache = t.0.join("c.json");
    let mut c = SessionCache::default();
    let all = c.list(&t.0, Scope::Slug("C--src-page-a"), 0, 100, &cache);
    assert_eq!(all.len(), 5);
    let pages: Vec<Vec<Session>> = (0..4)
        .map(|i| c.list(&t.0, Scope::Slug("C--src-page-a"), i * 2, 2, &cache))
        .collect();
    assert_eq!(pages.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2, 1, 0]);
    let ids = |v: &[Session]| v.iter().map(|s| s.session_id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&all), ids(&pages.concat()));
    c.list(&t.0, Scope::All, 0, 100, &cache);
    assert_eq!(cache_keys(&cache), 8, "a survey caches every session");
    c.list(&t.0, Scope::All, 4, 2, &cache);
    c.list(&t.0, Scope::All, 0, 2, &cache);
    assert_eq!(cache_keys(&cache), 8, "a paged or limited call merges");
    std::fs::remove_file(t.0.join("C--src-page-a").join("a1.jsonl")).unwrap();
    c.list(&t.0, Scope::All, 0, 100, &cache);
    assert_eq!(cache_keys(&cache), 7, "a survey still prunes");
    let names: Vec<String> = std::fs::read_dir(&t.0)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("c.json"))
        .collect();
    assert_eq!(names, ["c.json"], "no temp left beside the cache");
}

#[test]
fn snapshot_pages_compose_although_a_transcript_is_appended() {
    let t = tmp("snap");
    let dir = t.0.join("C--src-shift");
    for n in 1..=6 {
        set_mtime(
            &put(&dir, &format!("s{n}.jsonl"), &prompt(&format!("prompt s{n}"), r"C:\src\shift")),
            n,
        );
    }
    let cache = t.0.join("c.json");
    let mut c = SessionCache::default();
    let ids = |v: Vec<Session>| v.into_iter().map(|s| s.session_id).collect::<Vec<_>>().join(",");
    assert_eq!(ids(c.list(&t.0, Scope::All, 0, 100, &cache)), "s6,s5,s4,s3,s2,s1");
    let snap = session_files(&t.0, &[]);
    assert_eq!(snap.len(), 6);
    assert_eq!(session_files(&t.0, &["C--src-absent".into()]).len(), 0);
    assert_eq!(session_files(&t.0.join("no-such-root"), &[]).len(), 0);
    assert_eq!(session_files(&t.0, &["C--src-shift".into(), "C--src-absent".into()]).len(), 6);
    let mut walk = ids(c.list(&t.0, Scope::Files(&snap), 0, 2, &cache));
    set_mtime(&dir.join("s1.jsonl"), 99);
    walk = format!("{walk},{}", ids(c.list(&t.0, Scope::Files(&snap), 2, 2, &cache)));
    walk = format!("{walk},{}", ids(c.list(&t.0, Scope::Files(&snap), 4, 2, &cache)));
    assert_eq!(walk, "s6,s5,s4,s3,s2,s1");
    let mut gone = snap.clone();
    gone.push(dir.join("never-existed.jsonl"));
    assert_eq!(c.list(&t.0, Scope::Files(&gone), 0, 100, &cache).len(), 6);
    assert_eq!(
        c.list(&t.0, Scope::Files(&[]), 0, 100, &cache).len(),
        0,
        "an empty snapshot is an empty scope"
    );
}

#[test]
fn a_busy_cache_is_served_from_this_process_copy() {
    let t = tmp("busy");
    for n in 1..=3 {
        put(
            &t.0.join("C--src-share"),
            &format!("h{n}.jsonl"),
            &prompt(&format!("prompt h{n}"), r"C:\src\share"),
        );
    }
    let cache = t.0.join("c.json");
    let mut c = SessionCache::default();
    c.list(&t.0, Scope::All, 0, 100, &cache);
    let warm = c.read(&cache);
    assert_eq!(warm.len(), 3);
    let held = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::File::options().read(true).share_mode(0).open(&cache)
    };
    assert!(held.is_ok(), "the exclusive handle opened");
    assert_eq!(c.read(&cache), warm, "a busy file answers from the memo, not empty");
    drop(held);
}

#[test]
fn junctioned_roots_share_one_cache_file() {
    let t = tmp("junc");
    let phys = t.0.join("physical").join("projects");
    std::fs::create_dir_all(&phys).unwrap();
    let link = t.0.join("accountA");
    std::fs::create_dir_all(&link).unwrap();
    crate::env::win_fs::create_junction(&link.join("projects"), &phys).unwrap();
    let expect = physical_dir(&t.0.join("physical")).join("claude-auto-rs-sessions.json");
    assert_eq!(cache_path(&link.join("projects")), expect);
    assert_eq!(cache_path(&phys), expect);
    let plain = t.0.join("plain").join("projects");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(
        cache_path(&plain),
        physical_dir(&t.0.join("plain")).join("claude-auto-rs-sessions.json")
    );
    std::fs::remove_dir(link.join("projects")).unwrap();
}

#[test]
fn pre_filter_changes_how_many_lines_are_parsed_never_the_summary() {
    let dir = fixture("");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jsonl")))
        .collect();
    files.push(fixture("tree/C--Users-someone-Projects-Demo/aaaa1111.jsonl"));
    assert!(files.len() >= 9);
    for f in &files {
        assert_eq!(summarize(f, None, false), summarize(f, None, true), "{}", f.display());
    }
    let t = tmp("pf");
    let spaced = put(
        &t.0.join("C--src-pf"),
        "s.jsonl",
        "{\"type\": \"user\", \"message\": {\"content\": \"spaced key prompt\"}}\n{\"type\": \"assistant\", \"message\": {\"content\": [{\"type\": \"text\", \"text\": \"spaced key reply\"}]}}\n",
    );
    let s = summarize(&spaced, Some(r"C:\src\pf"), true).unwrap();
    assert_eq!(
        (s.title.as_str(), s.last_assistant.as_str()),
        ("spaced key prompt", "spaced key reply")
    );
    let esc = put(
        &t.0.join("C--src-pf"),
        "e.jsonl",
        "{\"\\u0074ype\":\"user\",\"message\":{\"content\":\"escaped key prompt\"}}\n",
    );
    assert_eq!(summarize(&esc, Some(r"C:\src\pf"), true).unwrap().title, "escaped key prompt");
}

#[test]
fn headless_summaries_are_marked() {
    let t = tmp("hl");
    for (e, want) in [("sdk-cli", true), ("sdk-ts", true), ("cli", false), ("", false)] {
        let entry = if e.is_empty() {
            String::new()
        } else {
            format!(r#","entrypoint":"{e}""#)
        };
        let p = put(
            &t.0.join("C--x"),
            &format!("hs-{e}.jsonl"),
            &format!("{{\"type\":\"user\"{entry},\"message\":{{\"content\":\"hello there\"}}}}\n"),
        );
        assert_eq!(summarize(&p, None, true).unwrap().headless, want, "entrypoint '{e}'");
    }
}

#[test]
fn capped_count_is_a_flag_beside_an_int() {
    let t = tmp("cap");
    let head = (["one", "two", "three"].map(|w| format!(r#"{{"type":"user","message":{{"content":"prompt {w}"}}}}"#))).join("\n");
    let pad = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{}"}}]}}}}"#,
        "p".repeat(1 << 20)
    );
    let tail = r#"{"type":"user","message":{"content":"prompt four"}}"#;
    let big = put(
        &t.0.join("C--src-big"),
        "g.jsonl",
        &format!("{head}\n{}{tail}\n", format!("{pad}\n").repeat(5)),
    );
    let s = summarize(&big, None, true).unwrap();
    assert_eq!((s.prompt_count, s.prompt_count_capped), (3, true));
    assert_eq!(prompt_detail(&big, 16 << 20), (4, false));
}
