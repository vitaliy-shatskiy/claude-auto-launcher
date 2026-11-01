//! The launcher log (`Env.ps1` Write-LauncherLog): one JSON object per line in
//! `<canonical root>\launcher-logs\claude-auto-<yyyy-MM-dd>.jsonl`. Append-only, fail-open, SILENT.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{Map, Value, json};

use crate::sys;

/// Where one run logs, and as which run.
#[derive(Debug, Clone)]
pub struct Logger {
    pub root: PathBuf,
    pub run_id: String,
    /// Preview writes nothing anywhere.
    pub enabled: bool,
    /// Debug mode's spans; off by default.
    pub timings: crate::timing::Timings,
}

impl Logger {
    /// Appends one record; `false` when it could not be written. Never prints, never fails.
    pub fn write(&self, stage: &str, data: Map<String, Value>) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(now) = sys::local_now() else { return false };
        let mut rec = Map::new();
        rec.insert("ts".into(), Value::String(iso_o(&now)));
        rec.insert("stage".into(), Value::String(stage.into()));
        rec.insert("run".into(), Value::String(self.run_id.clone()));
        rec.insert("pid".into(), json!(std::process::id()));
        for (k, v) in data {
            rec.insert(k, v);
        }
        let Ok(mut line) = serde_json::to_string(&Value::Object(rec)) else {
            return false;
        };
        line.push_str("\r\n");
        if std::fs::create_dir_all(&self.root).is_err() {
            return false;
        }
        let file = self
            .root
            .join(format!("claude-auto-{:04}-{:02}-{:02}.jsonl", now.year, now.month, now.day));
        append_with_retry(&file, line.as_bytes())
    }

    /// Debug mode: one `timing` record per ended span not written yet. Off: no record, no I/O.
    pub fn write_timings(&self) {
        for r in self.timings.take_unlogged() {
            let mut data = crate::log_data! { "name" => r.name, "parent" => r.parent, "ms" => r.ms, "startMs" => r.start_ms };
            if r.aborted {
                data.insert("aborted".into(), Value::Bool(true));
            }
            if let Some(code) = r.exit {
                data.insert("exit".into(), code.into());
            }
            self.write("timing", data);
        }
    }
}

#[cfg(test)]
impl Logger {
    /// Writes nothing, records into `timings`: a screen test's logger.
    pub(crate) fn silent(timings: crate::timing::Timings) -> Self {
        Self {
            root: PathBuf::new(),
            run_id: String::new(),
            enabled: false,
            timings,
        }
    }
}

/// Six tries with a growing pause cover the sharing-violation window of a concurrent launcher.
fn append_with_retry(file: &Path, bytes: &[u8]) -> bool {
    for i in 0..6u64 {
        let r = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
            .and_then(|mut f| f.write_all(bytes));
        match r {
            Ok(()) => return true,
            Err(_) => std::thread::sleep(Duration::from_millis(15 * (i + 1))),
        }
    }
    false
}

/// .NET's round-trip `o` format with the local offset: `2026-10-31T20:50:00.1234567+02:00`.
pub(crate) fn iso_o(t: &sys::LocalNow) -> String {
    let sign = if t.offset_minutes < 0 { '-' } else { '+' };
    let off = t.offset_minutes.unsigned_abs();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:07}{sign}{:02}:{:02}",
        t.year,
        t.month,
        t.day,
        t.hour,
        t.minute,
        t.second,
        t.ticks,
        off / 60,
        off % 60
    )
}

/// `Remove-OldLauncherLogs`: files older than `days` by last write time.
pub fn prune(root: &Path, days: u64) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let cut = SystemTime::now().checked_sub(Duration::from_secs(days * 86_400));
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let jsonl = Path::new(&name).extension().is_some_and(|e| e.eq_ignore_ascii_case("jsonl"));
        if !(name.to_ascii_lowercase().starts_with("claude-auto-") && jsonl) {
            continue;
        }
        let old = e.metadata().and_then(|m| m.modified()).ok().zip(cut).is_some_and(|(m, c)| m < c);
        if old && std::fs::remove_file(e.path()).is_err() {
            // Fail-open: an undeletable old log costs disk, never a launch.
        }
    }
}

/// `Get-LauncherParentChain`: this process and up to five ancestors, pid and name. The command
/// line is not read (WMI in PowerShell); see PORTING.md.
pub fn parent_chain(depth: usize) -> Vec<Value> {
    let table = sys::process_table();
    let mut out = Vec::new();
    let mut id = std::process::id();
    for _ in 0..depth {
        let Some(p) = table.iter().find(|p| p.pid == id) else { break };
        out.push(json!({ "pid": p.pid, "name": p.name, "cmd": "" }));
        if p.parent == 0 || p.parent == id {
            break;
        }
        id = p.parent;
    }
    out
}

/// Twelve hex characters, distinct per run.
pub fn new_run_id() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    h.write_u32(std::process::id());
    format!("{:012x}", h.finish() & 0xFFFF_FFFF_FFFF)
}

/// A JSON object from `key => value` pairs, for log call sites.
#[macro_export]
macro_rules! log_data {
    ($($k:literal => $v:expr),* $(,)?) => {{
        let mut m = serde_json::Map::new();
        $( m.insert($k.to_string(), serde_json::json!($v)); )*
        m
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_matches_dotnet_round_trip_shape() {
        let t = sys::LocalNow {
            year: 2026,
            month: 10,
            day: 31,
            hour: 20,
            minute: 5,
            second: 9,
            ticks: 1_234_567,
            offset_minutes: 120,
        };
        assert_eq!(iso_o(&t), "2026-10-31T20:05:09.1234567+02:00");
        let w = sys::LocalNow { offset_minutes: -330, ..t };
        assert!(iso_o(&w).ends_with("-05:30"));
    }

    #[test]
    fn run_ids_are_twelve_hex() {
        let a = new_run_id();
        assert_eq!(a.len(), 12);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn disabled_logger_writes_nothing() {
        let l = Logger {
            root: std::env::temp_dir().join("claude-auto-rs-never"),
            run_id: "x".into(),
            enabled: false,
            timings: crate::timing::Timings::default(),
        };
        assert!(!l.write("start", Map::new()));
        assert!(!l.root.exists());
    }

    #[test]
    fn record_lands_as_one_json_line() {
        let dir = std::env::temp_dir().join(format!("claude-auto-rs-log-{}", new_run_id()));
        let l = Logger {
            root: dir.clone(),
            run_id: "abc".into(),
            enabled: true,
            timings: crate::timing::Timings::default(),
        };
        assert!(l.write("exit", log_data! { "code" => 3 }));
        let file = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        let text = std::fs::read_to_string(&file).unwrap();
        let v: Value = serde_json::from_str(text.trim_end()).unwrap();
        assert_eq!(v["stage"], "exit");
        assert_eq!(v["code"], 3);
        assert_eq!(v.as_object().unwrap().keys().next().map(String::as_str), Some("ts"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
