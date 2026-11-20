//! What a start clears before its check: downloads a launcher left half-written when it exited,
//! and an `installed` record the exe on disk does not match. Exports `sweep_partials`,
//! `forget_stale_install`, `started_ms`, `alive_since`.

use std::path::Path;
use std::time::UNIX_EPOCH;

use super::Local;
use super::check::{State, Version};

const HOUR_MS: i64 = 60 * 60_000;
/// 1601-01-01 to 1970-01-01, in ms: FILETIME's epoch to Unix time.
const FILETIME_EPOCH_MS: i64 = 11_644_473_600_000;

fn written_ms(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    i64::try_from(t.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()
}

/// The pid in `<exe name>.<pid>-<id>.new` (`swap::download`'s name), when `name` is one.
fn download_pid(exe_name: &str, name: &str) -> Option<u32> {
    let rest = name.strip_prefix(exe_name)?.strip_prefix('.')?.strip_suffix(".new")?;
    let (pid, id) = rest.split_once('-')?;
    if id.is_empty() {
        return None;
    }
    pid.parse().ok()
}

/// Deletes this exe's downloads older than an hour whose process is gone (`alive(pid, written_ms)`
/// false): a launcher that exits mid-download leaves one, ~10 MB each.
pub(super) fn sweep_partials(exe: &Path, now_ms: i64, alive: &dyn Fn(u32, i64) -> bool) {
    let (Some(dir), Some(exe_name)) = (exe.parent(), exe.file_name().and_then(|n| n.to_str())) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| download_pid(exe_name, n)) else {
            continue;
        };
        let path = e.path();
        let Some(at) = written_ms(&path) else { continue };
        if now_ms.saturating_sub(at) >= HOUR_MS && !alive(pid, at) && std::fs::remove_file(&path).is_err() {
            // Held open by something: the next start tries again.
        }
    }
}

/// Forgets `installed` when this exe is another build (`current`) and was not replaced after this
/// process started (`started_ms`): a hand install over an updated exe. A swap by another launcher
/// after this one started stays recorded; an unknown time keeps the record.
pub(super) fn forget_stale_install(local: &Local, current: Option<Version>, started_ms: Option<i64>) {
    let mut s = State::read(&local.state);
    if s.installed.is_none() || s.installed == current {
        return;
    }
    let untouched_since_start = written_ms(&local.exe).zip(started_ms).is_some_and(|(w, st)| w <= st);
    if untouched_since_start {
        s.installed = current;
        s.write(&local.state);
    }
}

fn filetime_ms(ft: u64) -> i64 {
    i64::try_from(ft / 10_000).unwrap_or(i64::MAX).saturating_sub(FILETIME_EPOCH_MS)
}

/// When this process started, in Unix ms.
pub(super) fn started_ms() -> Option<i64> {
    crate::sessions::win::process_start(std::process::id()).map(filetime_ms)
}

/// `pid` runs a process that started no later than `since_ms`; a later one reuses a dead pid.
pub(super) fn alive_since(pid: u32, since_ms: i64) -> bool {
    crate::sessions::win::process_start(pid).is_some_and(|ft| filetime_ms(ft) <= since_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::self_update::Mode;
    use crate::self_update::tests::{local, tmp};
    use std::time::{Duration, SystemTime};

    fn age(path: &Path, by: Duration) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - by).unwrap();
    }

    #[test]
    fn an_old_download_of_a_dead_process_is_deleted_and_the_rest_stay() {
        let t = tmp("sweep");
        let l = local(&t.0, Mode::Notify);
        let names = [
            "claude-auto.exe.1-a.new",
            "claude-auto.exe.2-b.new",
            "claude-auto.exe.3-c.new",
            "claude-auto.exe.old",
            "other.exe.1-a.new",
        ];
        for n in names {
            std::fs::write(t.0.join(n), b"partial").unwrap();
        }
        for n in ["claude-auto.exe.1-a.new", "claude-auto.exe.2-b.new", "claude-auto.exe.old", "other.exe.1-a.new"] {
            age(&t.0.join(n), Duration::from_secs(2 * 3600));
        }
        let now = written_ms(&t.0.join("claude-auto.exe.3-c.new")).unwrap();
        sweep_partials(&l.exe, now, &|pid, _| pid == 2);
        let left: Vec<bool> = names.iter().map(|n| t.0.join(n).exists()).collect();
        assert_eq!(left, [false, true, true, true, true], "only the old download of a dead pid goes");
    }

    #[test]
    fn a_reused_pid_counts_as_dead() {
        assert!(!alive_since(std::process::id(), 0), "this process started after 1970");
        assert!(alive_since(std::process::id(), i64::MAX));
    }

    #[test]
    fn an_install_record_the_exe_does_not_match_is_forgotten() {
        let t = tmp("stale");
        let l = local(&t.0, Mode::Notify);
        let recorded = Some(Version([99, 0, 0]));
        let written = written_ms(&l.exe).unwrap();
        State { installed: recorded, ..State::default() }.write(&l.state);
        forget_stale_install(&l, Some(Version([1, 0, 0])), Some(written - 1));
        assert_eq!(State::read(&l.state).installed, recorded, "swapped in after this start: kept");
        forget_stale_install(&l, Some(Version([1, 0, 0])), Some(written + 1));
        assert_eq!(State::read(&l.state).installed, Some(Version([1, 0, 0])), "a hand install: forgotten");
    }
}
