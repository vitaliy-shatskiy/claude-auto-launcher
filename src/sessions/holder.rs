//! Another live claude.exe on a session (`Get-ClaudeSessionHolder`, `Get-ContinueSessionId`,
//! `Format-HeldSessionWarning`). Fail open: any unreadable input answers "not held".

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::listing::session_files;
use super::transcript::{clean_text, continue_candidate};
use crate::config::ps_str;

/// One live record of `<root>\sessions\<pid>.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Holder {
    pub pid: u32,
    pub session_id: String,
    pub name: String,
    pub status: String,
    pub kind: String,
    /// Process creation, UTC FILETIME.
    pub since: Option<u64>,
}

fn record(path: &Path, session_id: &str) -> Option<Holder> {
    let text = std::fs::read_to_string(path).ok()?;
    let r: Value = serde_json::from_str(&text).ok()?;
    let o = r.as_object()?;
    let get = |k: &str| o.get(k).map(ps_str).unwrap_or_default();
    if get("sessionId") != session_id {
        return None;
    }
    let pid = get("pid").parse::<i32>().ok().filter(|p| *p > 0)?;
    let pid = u32::try_from(pid).ok()?;
    let started = super::win::process_start(pid)?;
    if started.to_string() != get("procStart") {
        return None;
    }
    Some(Holder {
        pid,
        session_id: get("sessionId"),
        name: get("name"),
        status: get("status"),
        kind: get("kind"),
        since: Some(started),
    })
}

/// The live holder of `session_id` across every profile's `sessions\` folder: the pid runs AND
/// started at the recorded `procStart` (a reused pid fails). Records in name order.
pub(crate) fn find_holder(dirs: &[PathBuf], session_id: &str) -> Option<Holder> {
    if session_id.is_empty() {
        return None;
    }
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")))
            .collect();
        files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
        if let Some(h) = files.iter().find_map(|f| record(f, session_id)) {
            return Some(h);
        }
    }
    None
}

/// `Get-ContinueSessionId`: the newest transcript of the project's own slugs that `claude -c`
/// considers and no live NON-interactive process holds. No slug = no candidate.
pub(crate) fn continue_session_id(root: &Path, slugs: &[String], dirs: &[PathBuf]) -> String {
    let slugs: Vec<String> = slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
    if root.as_os_str().is_empty() || slugs.is_empty() {
        return String::new();
    }
    for file in session_files(root, &slugs) {
        if !continue_candidate(&file, 65_536) {
            continue;
        }
        let id = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if !dirs.is_empty() && find_holder(dirs, &id).is_some_and(|h| !h.kind.is_empty() && !h.kind.eq_ignore_ascii_case("interactive")) {
            continue;
        }
        return id;
    }
    String::new()
}

/// FILETIME (UTC) to the local `HH:mm` at `offset_minutes` from UTC.
fn local_hh_mm(filetime: u64, offset_minutes: i32) -> String {
    let unix_minutes = i64::try_from(filetime / 600_000_000).unwrap_or(0) - 11_644_473_600 / 60 + i64::from(offset_minutes);
    let m = unix_minutes.rem_euclid(1440);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// `Format-HeldSessionWarning`: the name (else the id's first block), pid, status, local start.
pub(crate) fn warning(h: &Holder, offset_minutes: i32) -> String {
    let mut label = clean_text(&h.name);
    if label.is_empty() {
        label = clean_text(h.session_id.split('-').next().unwrap_or(""));
    }
    let mut facts = vec![format!("pid {}", h.pid)];
    let status = clean_text(&h.status);
    if !status.is_empty() {
        facts.push(status);
    }
    if let Some(t) = h.since {
        facts.push(format!("since {}", local_hh_mm(t, offset_minutes)));
    }
    format!(
        "Session {label} is open in another Claude process ({}). Continuing here forks its transcript.",
        facts.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            if std::fs::remove_dir_all(&self.0).is_err() {}
        }
    }

    fn put(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn rec(pid: u32, sid: &str, start: &str, name: &str, kind: &str) -> String {
        format!(
            r#"{{"pid":{pid},"sessionId":"{sid}","cwd":"C:\\src\\Held","procStart":"{start}","status":"busy","name":"{name}","kind":"{kind}"}}"#
        )
    }

    fn age(p: &Path, minutes_ago: u64) {
        let t = SystemTime::now() - Duration::from_secs(minutes_ago * 60);
        std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
    }

    #[test]
    fn live_dead_reused_and_malformed_records() {
        let t = Tmp(std::env::temp_dir().join(format!("cars-w1-held-{}", std::process::id())));
        let me = std::process::id();
        let start = super::super::win::process_start(me).unwrap().to_string();
        let reused = (start.parse::<u64>().unwrap() + 1).to_string();
        let d = t.0.join("held");
        put(
            &d.join(format!("{me}.json")),
            &rec(me, "aaaa1111-live", &start, "held-e1", "interactive"),
        );
        put(
            &d.join("99999996.json"),
            &rec(99_999_996, "bbbb2222-dead", &start, "", "interactive"),
        );
        put(&d.join("reused.json"), &rec(me, "cccc3333-reused", &reused, "", "interactive"));
        put(&d.join("bg.json"), &rec(me, "eeee5555-bg", &start, "bg-1", "bg"));
        put(&d.join("0000-broken.json"), r#"{"pid": 12, "sessionId": "#);
        let b = t.0.join("broken");
        put(&b.join("a.json"), r#"{"pid": 12, "sessionId": "#);
        put(&b.join("b.json"), "[1, 2]");
        put(&b.join("c.json"), "\"text\"");
        put(
            &b.join("d.json"),
            &format!("[{}]", rec(me, "aaaa1111-live", &start, "held-e1", "interactive")),
        );
        let dirs = vec![d.clone()];
        let h = find_holder(&dirs, "aaaa1111-live").unwrap();
        assert_eq!((h.pid, h.name.as_str()), (me, "held-e1"));
        assert!(find_holder(&dirs, "bbbb2222-dead").is_none(), "a dead pid holds nothing");
        assert!(find_holder(&dirs, "cccc3333-reused").is_none(), "a reused pid holds nothing");
        assert!(find_holder(&[b], "aaaa1111-live").is_none(), "malformed records are ignored");
        assert!(find_holder(&[t.0.join("missing")], "aaaa1111-live").is_none());
        assert!(find_holder(&dirs, "").is_none());
        let several = [t.0.join("missing"), d.clone(), d.clone()];
        assert_eq!(find_holder(&several, "aaaa1111-live").map(|h| h.pid), Some(me));

        let since = local_hh_mm(start.parse().unwrap(), 0);
        assert_eq!(
            warning(&h, 0),
            format!(
                "Session held-e1 is open in another Claude process (pid {me}, busy, since {since}). Continuing here forks its transcript."
            )
        );

        let projects = d.join("projects");
        put(&projects.join("C--src-Held").join("old1111.jsonl"), "{}");
        put(&projects.join("C--src-Held").join("new2222.jsonl"), "{}");
        put(&projects.join("C--src-Other").join("foreign3.jsonl"), "{}");
        age(&projects.join("C--src-Held").join("old1111.jsonl"), 30);
        age(&projects.join("C--src-Held").join("new2222.jsonl"), 10);
        age(&projects.join("C--src-Other").join("foreign3.jsonl"), 1);
        assert_eq!(continue_session_id(&projects, &["C--src-Held".into()], &[]), "new2222");
        assert_eq!(continue_session_id(&projects, &[], &[]), "");
        assert_eq!(continue_session_id(&d.join("nope"), &["C--src-Held".into()], &[]), "");
        put(&projects.join("C--src-Elsewhere").join("aaaa1111-live.jsonl"), "{}");
        let id = continue_session_id(&projects, &["C--src-Elsewhere".into()], &dirs);
        assert_eq!(find_holder(&dirs, &id).map(|h| h.pid), Some(me), "matched on the id alone");
        let bg = projects.join("C--f-bg");
        put(
            &bg.join("eeee5555-bg.jsonl"),
            r#"{"type":"user","entrypoint":"cli","message":{"content":"x"}}"#,
        );
        put(
            &bg.join("ok-bg.jsonl"),
            r#"{"type":"user","entrypoint":"cli","message":{"content":"ok"}}"#,
        );
        age(&bg.join("eeee5555-bg.jsonl"), 1);
        age(&bg.join("ok-bg.jsonl"), 20);
        assert_eq!(
            continue_session_id(&projects, &["C--f-bg".into()], &dirs),
            "ok-bg",
            "a background holder is passed over"
        );
    }

    #[test]
    fn warning_falls_back_to_the_short_id_and_strips_controls() {
        let anon = Holder {
            pid: 7,
            session_id: "dddd4444-5555-6666".into(),
            name: String::new(),
            status: String::new(),
            kind: String::new(),
            since: Some(116_444_736_000_000_000 + (8 * 60 + 5) * 600_000_000),
        };
        assert_eq!(
            warning(&anon, 0),
            "Session dddd4444 is open in another Claude process (pid 7, since 08:05). Continuing here forks its transcript."
        );
        let esc = Holder {
            name: "x\u{1b}[2Jy".into(),
            status: "busy\u{7}".into(),
            since: None,
            ..anon
        };
        assert!(!warning(&esc, 0).chars().any(char::is_control));
    }
}
