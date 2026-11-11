//! The update check as the launch screen sees it (`UpdateWatch`): started at process start, read
//! without blocking while the screen is up, waited for after Enter (and before maintenance) so
//! claude never starts mid-update and two updates never run at once. A quit drops a check that
//! is only talking to the network, never a running install (`InstallGate`).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

use crate::background::{Background, Left, Waited};
use crate::env::Note;
use crate::types::{LabelDefaults, VersionInfo};

/// What the check found: the header's versions, the catalog labels and `default (X)` labels
/// re-read after an install (`None`: no install ran), the lines it prints once the screen is
/// gone, and its own span.
#[derive(Debug, Clone)]
pub(crate) struct UpdateFound {
    pub(crate) version: VersionInfo,
    pub(crate) labels: Option<BTreeMap<String, String>>,
    pub(crate) defaults: Option<LabelDefaults>,
    pub(crate) lines: Vec<Note>,
    pub(crate) span: (Instant, Instant),
}

const OPEN: u8 = 0;
const INSTALLING: u8 = 1;
const CLOSED: u8 = 2;

/// Who goes first, the check's install or the user's quit: whichever flips it from open.
#[derive(Debug, Clone, Default)]
pub(crate) struct InstallGate(Arc<AtomicU8>);

impl InstallGate {
    /// The check is about to run `claude update`: `false` = the launcher is quitting, skip it.
    pub(crate) fn begin_install(&self) -> bool {
        self.0
            .compare_exchange(OPEN, INSTALLING, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// The launcher quits: `true` = no install runs or will run, so nothing needs waiting for.
    fn close(&self) -> bool {
        match self.0.compare_exchange(OPEN, CLOSED, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => true,
            Err(now) => now == CLOSED,
        }
    }
}

/// The check in flight, or none (preview, `CLAUDE_AUTO_NO_UPDATE=1`).
#[derive(Debug)]
pub(crate) struct UpdateWatch {
    job: Option<Background<UpdateFound>>,
    gate: InstallGate,
    /// The versions the screen drew first.
    shown: VersionInfo,
    /// The answer was handed to a screen once already.
    told: bool,
}

impl UpdateWatch {
    /// Starts `check` on its own thread; it asks the gate before installing.
    pub(crate) fn start(shown: VersionInfo, check: impl FnOnce(&InstallGate) -> UpdateFound + Send + Clone + 'static) -> Self {
        let gate = InstallGate::default();
        let theirs = gate.clone();
        Self {
            job: Some(Background::start("update", move || check(&theirs))),
            gate,
            shown,
            told: false,
        }
    }

    pub(crate) fn off(shown: VersionInfo) -> Self {
        Self {
            job: None,
            gate: InstallGate::default(),
            shown,
            told: false,
        }
    }

    fn found(&self) -> Option<&UpdateFound> {
        self.job.as_ref().and_then(Background::done).and_then(|a| a.as_ref().ok())
    }

    /// The answer, once, the first time it is in and changes what the screen shows.
    pub(crate) fn news(&mut self) -> Option<&UpdateFound> {
        if self.told || !matches!(self.job.as_mut().and_then(Background::poll), Some(Ok(_))) {
            return None;
        }
        self.told = true;
        let shown = &self.shown;
        self.found().filter(|f| f.version != *shown || f.labels.is_some())
    }

    /// The answer a screen was already handed: every later screen draws from it too.
    pub(crate) fn applied(&self) -> Option<&UpdateFound> {
        self.found().filter(|_| self.told)
    }

    /// Waits for a check still running (after Enter, before maintenance); `on_slow` shows a status
    /// line when that takes a while. The wait, if this call waited; `left` (Esc, Ctrl+C) ends it
    /// and the check runs on, unread.
    pub(crate) fn finish(&mut self, on_slow: impl FnOnce(), left: impl FnMut() -> bool) -> Result<Option<Waited>, Left> {
        self.job.as_mut().map_or(Ok(None), |j| j.wait_or_leave(on_slow, left))
    }

    /// Quit without a launch: a check only talking to the network is left behind (its cache write
    /// is temp + rename); a running install is waited for.
    pub(crate) fn quit(&mut self, on_slow: impl FnOnce()) -> Option<Waited> {
        self.quit_or_leave(on_slow, || false).unwrap_or_default()
    }

    /// `quit` whose wait for a running install the user can leave (`left`).
    pub(crate) fn quit_or_leave(&mut self, on_slow: impl FnOnce(), left: impl FnMut() -> bool) -> Result<Option<Waited>, Left> {
        if self.gate.close() {
            self.abandon();
            return Ok(None);
        }
        self.finish(on_slow, left)
    }

    /// The process exits without the answer: nothing waits for the check, now or on drop.
    pub(crate) fn abandon(&mut self) {
        if let Some(j) = self.job.as_mut() {
            j.detach();
        }
    }

    /// What the check had to say: its lines, or the panic that ended it.
    pub(crate) fn outcome(&self) -> Option<Result<&UpdateFound, &str>> {
        self.job
            .as_ref()
            .and_then(Background::done)
            .map(|a| a.as_ref().map_err(String::as_str))
    }
}

impl Drop for UpdateWatch {
    /// An error exit quits the same way, without a status line.
    fn drop(&mut self) {
        self.quit(|| {});
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::Duration;

    fn v(installed: &str) -> VersionInfo {
        VersionInfo {
            installed: Some(installed.into()),
            newest: None,
        }
    }

    pub(crate) fn found(version: VersionInfo, labels: Option<BTreeMap<String, String>>) -> UpdateFound {
        let now = Instant::now();
        UpdateFound {
            version,
            labels,
            defaults: None,
            lines: Vec::new(),
            span: (now, now),
        }
    }

    /// A fake job's longest run: a test that never lets it go fails instead of hanging.
    pub(crate) const BOUND: Duration = Duration::from_secs(3);

    /// A fake slow checker that only talks to the network: it answers `answer` once the test
    /// sends on the returned gate (or after `BOUND`).
    pub(crate) fn slow_check(shown: VersionInfo, answer: UpdateFound) -> (Sender<()>, UpdateWatch) {
        let (go, gate) = channel::<()>();
        let gate = Arc::new(Mutex::new(gate));
        let watch = UpdateWatch::start(shown, move |_| {
            gate.lock().unwrap().recv_timeout(BOUND).unwrap_or_default();
            answer
        });
        (go, watch)
    }

    /// A fake checker that starts an install: `started` fires once it holds the gate, then it
    /// runs until the test sends on `go`.
    pub(crate) fn slow_install(answer: UpdateFound) -> (Receiver<bool>, Sender<()>, UpdateWatch) {
        let (go, gate) = channel::<()>();
        let (began, started) = channel::<bool>();
        let gate = Arc::new(Mutex::new(gate));
        let began = Arc::new(Mutex::new(began));
        let watch = UpdateWatch::start(VersionInfo::default(), move |g| {
            let installing = g.begin_install();
            began.lock().unwrap().send(installing).unwrap();
            if installing {
                gate.lock().unwrap().recv_timeout(BOUND).unwrap_or_default();
            }
            answer
        });
        (started, go, watch)
    }

    #[test]
    fn news_comes_once_when_the_check_changed_something() {
        let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.2"), None));
        assert!(w.news().is_none(), "still running");
        assert!(w.applied().is_none());
        go.send(()).unwrap();
        w.finish(|| {}, || false).unwrap();
        assert_eq!(w.news().map(|f| f.version.clone()), Some(v("2.1.2")));
        assert!(w.news().is_none(), "told once");
        assert_eq!(w.applied().map(|f| f.version.clone()), Some(v("2.1.2")));
    }

    #[test]
    fn an_unchanged_answer_is_no_news() {
        let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.1"), None));
        go.send(()).unwrap();
        w.finish(|| {}, || false).unwrap();
        assert!(w.news().is_none(), "nothing to redraw");
    }

    #[test]
    fn finish_waits_for_an_unfinished_check() {
        let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.2"), None));
        let release = std::thread::spawn(move || go.send(()).unwrap());
        assert!(w.finish(|| {}, || false).unwrap().is_some(), "Enter waited for the check");
        assert!(matches!(w.outcome(), Some(Ok(f)) if f.version == v("2.1.2")));
        release.join().unwrap();
    }

    #[test]
    fn a_quit_leaves_a_check_that_only_talks_to_the_network() {
        let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.2"), None));
        let release = std::thread::spawn(move || go.send(()).unwrap());
        assert_eq!(w.quit(|| {}), None, "no wait");
        assert!(w.outcome().is_none(), "the answer was never read");
        release.join().unwrap();
    }

    #[test]
    fn a_quit_waits_for_a_running_install() {
        let (started, go, mut w) = slow_install(found(v("2.1.2"), None));
        assert!(started.recv().unwrap(), "the install began before the quit");
        let release = std::thread::spawn(move || go.send(()).unwrap());
        assert!(w.quit(|| {}).is_some(), "the quit waited for the install");
        assert!(matches!(w.outcome(), Some(Ok(_))));
        release.join().unwrap();
    }

    #[test]
    fn an_install_after_a_quit_never_starts() {
        let gate = InstallGate::default();
        assert!(gate.close());
        assert!(!gate.begin_install(), "the check skips the install");
        let running = InstallGate::default();
        assert!(running.begin_install());
        assert!(!running.close(), "a quit must wait");
    }

    #[test]
    fn a_quit_and_an_install_never_both_win_the_gate() {
        // Given: fresh gates, one install and one quit racing for each, released together by a
        // spin barrier (a blocking barrier wakes the two too far apart to overlap).
        const ROUNDS: usize = 20_000;
        let gates: Arc<Vec<InstallGate>> = Arc::new((0..ROUNDS).map(|_| InstallGate::default()).collect());
        let arrived = Arc::new(AtomicUsize::new(0));
        let race = |gates: &[InstallGate], arrived: &AtomicUsize, win: &dyn Fn(&InstallGate) -> bool| -> Vec<bool> {
            gates
                .iter()
                .enumerate()
                .map(|(i, g)| {
                    arrived.fetch_add(1, Ordering::SeqCst);
                    while arrived.load(Ordering::SeqCst) < 2 * (i + 1) {
                        std::hint::spin_loop();
                    }
                    win(g)
                })
                .collect()
        };
        let (theirs, their_count) = (Arc::clone(&gates), Arc::clone(&arrived));
        // When: the check begins its install while the launcher quits.
        let installer = std::thread::spawn(move || race(&theirs, &their_count, &InstallGate::begin_install));
        let quits = race(&gates, &arrived, &InstallGate::close);
        let installs = installer.join().unwrap();
        // Then: never both: a quit that waits for nothing never leaves an install running.
        let both = quits.iter().zip(&installs).filter(|(q, i)| **q && **i).count();
        assert_eq!(both, 0, "rounds where the quit and the install both won");
    }

    #[test]
    fn off_never_waits_and_has_no_news() {
        let mut w = UpdateWatch::off(v("2.1.1"));
        assert!(w.news().is_none());
        assert_eq!(w.finish(|| panic!("no status"), || false), Ok(None));
        assert!(w.outcome().is_none());
    }
}
