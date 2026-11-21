//! Another live claude.exe on a session. Fail open: any unreadable input answers "not held".

use std::collections::HashSet;
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
    live_record(path, Some(session_id), &super::win::process_start)
}

/// The record at `path` when its pid runs and started at its `procStart` (`start_of` reads a
/// pid's creation FILETIME); `want` filters on the session id first.
fn live_record(path: &Path, want: Option<&str>, start_of: &dyn Fn(u32) -> Option<u64>) -> Option<Holder> {
    let text = std::fs::read_to_string(path).ok()?;
    let r: Value = serde_json::from_str(&text).ok()?;
    let o = r.as_object()?;
    let get = |k: &str| o.get(k).map(ps_str).unwrap_or_default();
    if want.is_some_and(|w| get("sessionId") != w) || get("sessionId").is_empty() {
        return None;
    }
    let pid = get("pid").parse::<i32>().ok().filter(|p| *p > 0)?;
    let pid = u32::try_from(pid).ok()?;
    let started = start_of(pid)?;
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

/// The ids of the sessions a live Claude Code process has open, across every account's
/// `sessions\` folder: the same record twice (sharing links the folders) counts once. Stale
/// records (a dead pid, a reused one with another start) and unreadable ones never count.
pub(crate) fn live_ids(dirs: &[PathBuf], start_of: &dyn Fn(u32) -> Option<u64>) -> HashSet<String> {
    live_holders(dirs, start_of).into_iter().map(|h| h.session_id).collect()
}

/// Every live record across the `sessions\` folders, one per session, oldest process first.
pub(crate) fn live_holders(dirs: &[PathBuf], start_of: &dyn Fn(u32) -> Option<u64>) -> Vec<Holder> {
    let mut out: Vec<Holder> = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json"))
                && let Some(h) = live_record(&p, None, start_of)
                && !out.iter().any(|o| o.session_id == h.session_id)
            {
                out.push(h);
            }
        }
    }
    out.sort_by_key(|h| h.since);
    out
}

/// The launch page's running sessions: a name (the session's, else its kind and pid), what it is
/// doing, and how long it has run at `now_ms`.
pub(crate) fn running_sessions(dirs: &[PathBuf], now_ms: i64) -> Vec<crate::launch::frame::RunningSession> {
    running_from(&live_holders(dirs, &super::win::process_start), now_ms)
}

/// FILETIME (100 ns since 1601) to Unix milliseconds.
const FILETIME_UNIX_MS: i64 = 11_644_473_600_000;

fn running_from(holders: &[Holder], now_ms: i64) -> Vec<crate::launch::frame::RunningSession> {
    holders
        .iter()
        .map(|h| {
            let name = clean_text(&h.name);
            let kind = if h.kind.is_empty() {
                "claude".to_owned()
            } else {
                clean_text(&h.kind)
            };
            crate::launch::frame::RunningSession {
                name: if name.is_empty() { format!("{kind} {}", h.pid) } else { name },
                project: clean_text(&h.status),
                age: h
                    .since
                    .and_then(|ft| i64::try_from(ft / 10_000).ok())
                    .map(|ms| super::frame::relative_age(ms - FILETIME_UNIX_MS, now_ms))
                    .unwrap_or_default(),
            }
        })
        .collect()
}

/// The newest transcript of the project's own slugs that `claude -c`
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

/// The name (else the id's first block), pid, status, local start.
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
    fn live_ids_are_the_records_whose_pid_runs_with_its_recorded_start() {
        // Given: pid 10 runs since 500; pid 20 is gone; pid 30 runs since 900 (reused).
        let start_of = |pid: u32| match pid {
            10 => Some(500),
            30 => Some(900),
            _ => None,
        };
        let t = Tmp(std::env::temp_dir().join(format!("cars-live-ids-{}", std::process::id())));
        let (a, b) = (t.0.join("a").join("sessions"), t.0.join("b").join("sessions"));
        put(&a.join("10.json"), &rec(10, "live-1", "500", "", "interactive"));
        put(&a.join("20.json"), &rec(20, "dead-2", "500", "", "interactive"));
        put(&a.join("30.json"), &rec(30, "reused-3", "400", "", "interactive"));
        put(&a.join("40.json"), "{ not json");
        put(&a.join("notes.txt"), &rec(10, "not-a-record", "500", "", "interactive"));
        // And: sharing shows the same record under a second account.
        put(&b.join("10.json"), &rec(10, "live-1", "500", "", "interactive"));
        // When: the live set is read over both, plus a missing folder.
        let ids = live_ids(&[a, b, t.0.join("missing")], &start_of);
        // Then: only the live match counts, once.
        assert_eq!(ids, HashSet::from(["live-1".to_owned()]));
        assert!(live_ids(&[t.0.join("missing")], &start_of).is_empty());
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

    #[test]
    fn running_sessions_name_each_holder_and_its_age_and_fall_back_to_kind_and_pid() {
        // Given: two holders, one named, started 12 min and 3 h before now.
        let now_ms = 1_789_560_000_000;
        let ft = |mins: i64| u64::try_from((now_ms - mins * 60_000 + FILETIME_UNIX_MS) * 10_000).ok();
        let named = Holder {
            pid: 7,
            session_id: "a".into(),
            name: "claude-auto design".into(),
            status: "busy".into(),
            kind: "interactive".into(),
            since: ft(12),
        };
        let anon = Holder {
            pid: 9,
            session_id: "b".into(),
            name: String::new(),
            status: "idle\u{7}".into(),
            since: ft(180),
            ..named.clone()
        };
        // When / then
        let r = running_from(&[named, anon], now_ms);
        assert_eq!(
            (r[0].name.as_str(), r[0].project.as_str(), r[0].age.as_str()),
            ("claude-auto design", "busy", "12 min")
        );
        assert_eq!(
            (r[1].name.as_str(), r[1].project.as_str(), r[1].age.as_str()),
            ("interactive 9", "idle", "3 h")
        );
    }

    #[test]
    fn live_holders_count_a_shared_record_once_oldest_first() {
        let start_of = |pid: u32| match pid {
            10 => Some(500),
            11 => Some(300),
            _ => None,
        };
        let t = Tmp(std::env::temp_dir().join(format!("cars-live-holders-{}", std::process::id())));
        let (a, b) = (t.0.join("a").join("sessions"), t.0.join("b").join("sessions"));
        put(&a.join("10.json"), &rec(10, "s-10", "500", "", "interactive"));
        put(&b.join("10.json"), &rec(10, "s-10", "500", "", "interactive"));
        put(&b.join("11.json"), &rec(11, "s-11", "300", "", "interactive"));
        let ids: Vec<String> = live_holders(&[a, b], &start_of).into_iter().map(|h| h.session_id).collect();
        assert_eq!(ids, ["s-11", "s-10"]);
    }
}
