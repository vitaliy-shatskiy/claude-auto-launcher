use super::*;

fn rec(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn slug_parse_worktree_projects_and_single_segment() {
    assert_eq!(
        slug_names("C--Users-someone-Desktop-Projects-Demo-Service--claude-worktrees-feature-1"),
        ("Demo-Service".into(), "feature-1".into())
    );
    assert_eq!(slug_names("C--Users-someone-Desktop-Projects-Demo"), ("Demo".into(), String::new()));
    assert_eq!(slug_names("subagents").0, "subagents", "a single segment is the whole segment");
    assert_eq!(slug_names("C--Users-someone-Desktop-Scratch").0, "Scratch");
    assert_eq!(
        slug_names("C--Users-someone-Desktop-Pro\u{AD}jects-Demo").0,
        "Demo",
        "Projects- is matched ordinally"
    );
    assert_eq!(
        slug_names("C--Users-someone-Desktop-Projects-Demo-\u{AD}-claude-worktrees-feature").1,
        ""
    );
}

#[test]
fn fences_collapse_with_real_line_counts() {
    let plain = "plain text with `inline` backticks, no fence at all";
    assert_eq!(collapse_fences(plain), plain);
    let fenced = "Here:\n```powershell\nWrite-Host 'a'\nWrite-Host 'b'\nWrite-Host 'c'\n```\nDone.";
    assert_eq!(collapse_fences(fenced), "Here:\n[code: 3 lines]\nDone.");
    assert_eq!(
        collapse_fences("```\none\n``` and ```\nx\ny\n```"),
        "[code: 1 lines] and [code: 2 lines]"
    );
}

#[test]
fn clean_text_turns_markers_and_escapes_into_spaces() {
    assert_eq!(clean_text("a\u{4}b\u{5}c\u{1}d\u{2}e"), "a b c d e");
    assert_eq!(clean_text("x\u{1F}y"), "x y");
    assert_eq!(clean_text("\u{1b}]0;pwned\u{7}hello \u{1b}[2Jworld"), "]0;pwned hello [2Jworld");
}

#[test]
fn user_prompt_rules() {
    assert_eq!(
        user_prompt(&rec(r#"{"type":"user","message":{"content":"hi"}}"#)).as_deref(),
        Some("hi")
    );
    assert_eq!(
        user_prompt(&rec(r#"{"type":"user","isSidechain":true,"message":{"content":"x"}}"#)),
        None
    );
    assert_eq!(
        user_prompt(&rec(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]}}"#
        )),
        None
    );
    let cmd = r#"{"type":"user","message":{"content":"<command-name>/doctor</command-name><command-args>fix it</command-args>"}}"#;
    assert_eq!(user_prompt(&rec(cmd)).as_deref(), Some("/doctor fix it"));
    let bare = r#"{"type":"user","message":{"content":"<command-name>/clear</command-name><command-args></command-args>"}}"#;
    assert_eq!(user_prompt(&rec(bare)), None);
    assert_eq!(
        user_prompt(&rec(r#"{"type":"user","message":{"content":"<bash-stdout>x</bash-stdout>"}}"#)),
        None
    );
    assert_eq!(
        user_prompt(&rec(r#"{"type":"user","message":{"content":"<task-notification>x"}}"#)),
        None
    );
}

fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

fn temp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cars-w1-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn tail_walk_is_bounded_by_bytes() {
    let d = temp("tail");
    let p = write(&d, "t.txt", &format!("{}\ntail line\n", "x".repeat(1 << 20)));
    let lines = tail_lines(&p, 5, 65_536);
    assert_eq!(lines.last().map(String::as_str), Some("tail line"));
    let longest = lines.iter().map(String::len).max().unwrap_or(0);
    assert!(longest > 0 && longest <= 70_000, "longest {longest}");
    std::fs::remove_dir_all(&d).unwrap();
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

#[test]
fn prompt_counts_agree_with_the_fixtures() {
    assert_eq!(prompt_detail(&fixture("counted.jsonl"), MAX_BYTES).0, 3);
    assert_eq!(prompt_detail(&fixture("bare-clear.jsonl"), MAX_BYTES).0, 0);
    assert_eq!(prompt_detail(&fixture("slash-command.jsonl"), MAX_BYTES).0, 1);
    assert_eq!(prompt_detail(&fixture("task-notification.jsonl"), MAX_BYTES).0, 1);
    assert_eq!(prompt_detail(&fixture("embedded-tag.jsonl"), MAX_BYTES).0, 1);
}

#[test]
fn cap_boundary_both_ends_and_bytes_not_chars() {
    let d = temp("bound");
    let line = |i: usize| format!(r#"{{"type":"user","message":{{"content":"prompt-{i:02}"}}}}"#);
    let lines = |n: usize| (1..=n).map(line).collect::<Vec<_>>().join("\n") + "\n";
    let exact = write(&d, "exact.jsonl", &lines(4));
    assert_eq!(std::fs::metadata(&exact).unwrap().len(), 200);
    assert_eq!(prompt_detail(&exact, 200), (4, false), "exactly the budget is read whole");
    let over = write(&d, "over.jsonl", &lines(5));
    assert_eq!(prompt_detail(&over, 200), (4, true), "one byte over engages the bound");
    let straddle = format!(
        "{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}]}}}}\n{{\"type\":\"user\",\"message\":{{\"content\":\"{}\"}}}}\n",
        "p".repeat(84),
        "q".repeat(60)
    );
    assert_eq!(prompt_detail(&write(&d, "s.jsonl", &straddle), 200), (1, true));
    let cyr = (1..=40)
        .map(|_| format!(r#"{{"type":"user","message":{{"content":"{}"}}}}"#, "\u{43F}".repeat(50)))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert_eq!(prompt_detail(&write(&d, "c.jsonl", &cyr), 1000).0, 8, "the budget is BYTES");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn count_agrees_with_its_authority() {
    let d = temp("auth");
    let cases = [
        r#"{"type":"user","message":{"role":"user","content":"fix the build"}}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
        r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"subagent chatter"}}"#,
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}"#,
        r#"{"type":"user","message":{"role":"user","content":"<system-reminder>noise</system-reminder>"}}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<system-reminder>noise</system-reminder>"}]}}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<local-command-stdout>build output</local-command-stdout>"}]}}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<command-name>/clear</command-name><command-args></command-args>"}]}}"#,
        r#"{"type": "user", "message": {"role": "user", "content": "a real prompt"}}"#,
        r#"{"type":"user","message":{"role":"user","content":[]}}"#,
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]}}"#,
        r#"{"type":"user","message":{"role":"user","content":"   "}}"#,
        r#"{"type":"user","toolUseResult":{"content":"tool output here"},"message":{"role":"user","content":[{"type":"text","text":"<system-reminder>noise</system-reminder>"}]}}"#,
        r#"{"type":"user","toolUseResult":{"content":"tool output here"},"message":{"role":"user","content":"fix the build"}}"#,
        r#"{"type":"user","message":{"role":"user","content":"why is \"tool_result\" in the docs"}}"#,
        // A key whose own text opens with an escaped quote is content, not the isMeta flag.
        r#"{"type":"user","message":{"role":"user","content":"fix it"},"x":{"\"isMeta": true}}"#,
    ];
    for (i, c) in cases.iter().enumerate() {
        let authority = i64::from(parse_line(c).and_then(|r| user_prompt(&r)).is_some());
        let p = write(&d, &format!("case{i}.jsonl"), &format!("{c}\n"));
        assert_eq!(prompt_detail(&p, MAX_BYTES).0, authority, "case {i}: {c}");
    }
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn headless_and_continue_candidates() {
    let d = temp("cont");
    let rec = |e: &str| format!(r#"{{"type":"user","entrypoint":"{e}","message":{{"content":"x"}}}}"#);
    assert!(is_headless(&write(&d, "a.jsonl", &rec("sdk-cli"))));
    assert!(!is_headless(&write(&d, "b.jsonl", &rec("cli"))));
    assert!(
        !is_headless(&write(&d, "c.jsonl", r#"{"type":"user"}"#)),
        "no entrypoint is interactive"
    );
    let pad = format!(r#"{{"type":"progress","data":"{}"}}"#, "p".repeat(1000));
    let sdktail = format!(
        "{{\"type\":\"user\",\"message\":{{\"content\":\"x\"}}}}\n{}{{\"type\":\"last-prompt\",\"entrypoint\":\"sdk-py\"}}",
        format!("{pad}\n").repeat(80)
    );
    let rejected = [
            rec("sdk-cli"),
            sdktail,
            r#"{"type":"user","isSidechain":true,"message":{"content":"x"}}"#.to_owned(),
            r#"{"type":"user","teamName":"alpha","message":{"content":"x"}}"#.to_owned(),
            "{\"type\":\"file-history-snapshot\"}\n{\"parentUuid\":null,\"type\":\"user\",\"sessionKind\":\"daemon\",\"message\":{\"content\":\"x\"}}".to_owned(),
        ];
    for (i, text) in rejected.iter().enumerate() {
        assert!(!continue_candidate(&write(&d, &format!("r{i}.jsonl"), text), 65_536), "case {i}");
    }
    assert!(continue_candidate(&write(&d, "ok.jsonl", &rec("cli")), 65_536));
    std::fs::remove_dir_all(&d).unwrap();
}
