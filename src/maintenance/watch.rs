//! The update check as the launch screen sees it (`UpdateWatch`): started at process start, read
//! without blocking while the screen is up, waited for after Enter so claude never starts
//! mid-update.

use std::collections::BTreeMap;
use std::time::Instant;

use crate::background::{Background, Waited};
use crate::env::Note;
use crate::types::VersionInfo;

/// What the check found: the header's versions, the catalog labels re-read after an install
/// (`None`: no install ran), the lines it prints once the screen is gone, and its own span.
#[derive(Debug, Clone)]
pub(crate) struct UpdateFound {
    pub(crate) version: VersionInfo,
    pub(crate) labels: Option<BTreeMap<String, String>>,
    pub(crate) lines: Vec<Note>,
    pub(crate) span: (Instant, Instant),
}

/// The check in flight, or none (preview, `CLAUDE_AUTO_NO_UPDATE=1`).
#[derive(Debug)]
pub(crate) struct UpdateWatch {
    job: Option<Background<UpdateFound>>,
    /// The versions the screen drew first.
    shown: VersionInfo,
    /// The answer was handed to a screen once already.
    told: bool,
}

impl UpdateWatch {
    pub(crate) fn start(shown: VersionInfo, check: impl FnOnce() -> UpdateFound + Send + Clone + 'static) -> Self {
        Self {
            job: Some(Background::start("update", check)),
            shown,
            told: false,
        }
    }

    pub(crate) fn off(shown: VersionInfo) -> Self {
        Self {
            job: None,
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

    /// The versions the header shows now.
    pub(crate) fn version(&self) -> &VersionInfo {
        self.found().filter(|_| self.told).map_or(&self.shown, |f| &f.version)
    }

    /// After Enter: waits for a check still running; `on_slow` shows a status line when that
    /// takes a while. The wait, if this call waited.
    pub(crate) fn finish(&mut self, on_slow: impl FnOnce()) -> Option<Waited> {
        self.job.as_mut().and_then(|j| j.wait(on_slow).1)
    }

    /// What the check had to say: its lines, or the panic that ended it.
    pub(crate) fn outcome(&self) -> Option<Result<&UpdateFound, &str>> {
        self.job
            .as_ref()
            .and_then(Background::done)
            .map(|a| a.as_ref().map_err(String::as_str))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::mpsc::{Sender, channel};
    use std::sync::{Arc, Mutex};

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
            lines: Vec::new(),
            span: (now, now),
        }
    }

    /// A fake slow checker: it answers `answer` once the test sends on the returned gate.
    pub(crate) fn slow_check(shown: VersionInfo, answer: UpdateFound) -> (Sender<()>, UpdateWatch) {
        let (go, gate) = channel::<()>();
        let gate = Arc::new(Mutex::new(gate));
        let watch = UpdateWatch::start(shown, move || {
            gate.lock().unwrap().recv().unwrap();
            answer
        });
        (go, watch)
    }

    #[test]
    fn news_comes_once_when_the_check_changed_something() {
        let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.2"), None));
        assert!(w.news().is_none(), "still running");
        assert_eq!(w.version(), &v("2.1.1"));
        go.send(()).unwrap();
        w.finish(|| {});
        assert_eq!(w.news().map(|f| f.version.clone()), Some(v("2.1.2")));
        assert!(w.news().is_none(), "told once");
        assert_eq!(w.version(), &v("2.1.2"));
    }

    #[test]
    fn an_unchanged_answer_is_no_news() {
        let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.1"), None));
        go.send(()).unwrap();
        w.finish(|| {});
        assert!(w.news().is_none(), "nothing to redraw");
        assert_eq!(w.version(), &v("2.1.1"));
    }

    #[test]
    fn finish_waits_for_an_unfinished_check() {
        let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.2"), None));
        let release = std::thread::spawn(move || go.send(()).unwrap());
        assert!(w.finish(|| {}).is_some(), "Enter waited for the check");
        assert!(matches!(w.outcome(), Some(Ok(f)) if f.version == v("2.1.2")));
        release.join().unwrap();
    }

    #[test]
    fn off_never_waits_and_has_no_news() {
        let mut w = UpdateWatch::off(v("2.1.1"));
        assert!(w.news().is_none());
        assert_eq!(w.finish(|| panic!("no status")), None);
        assert!(w.outcome().is_none());
    }
}
