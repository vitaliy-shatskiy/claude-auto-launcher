//! The registry built on a background thread while the launch screen is up (`Prefetch`): the
//! first `get` waits for that one build, later calls reuse it; nothing ever scans twice.

use std::thread::JoinHandle;
use std::time::Instant;

use super::Registry;

/// A registry build in flight or finished.
#[derive(Debug)]
pub struct Prefetch {
    pending: Option<JoinHandle<Registry>>,
    done: Option<Registry>,
}

/// What the first `get` reports: how long the caller blocked on the build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waited {
    pub start: Instant,
    pub end: Instant,
}

impl Prefetch {
    /// Starts `build` on its own thread. A thread that cannot start builds here, at once.
    pub fn start(build: impl FnOnce() -> Registry + Send + Clone + 'static) -> Self {
        match std::thread::Builder::new().name("registry".into()).spawn(build.clone()) {
            Ok(h) => Self {
                pending: Some(h),
                done: None,
            },
            Err(_) => Self {
                pending: None,
                done: Some(build()),
            },
        }
    }

    /// The registry; the first call waits for the build. A build that panicked is an empty list:
    /// the launch never fails because of an optional step.
    pub fn get(&mut self) -> (&Registry, Option<Waited>) {
        let mut waited = None;
        if let Some(h) = self.pending.take() {
            let start = Instant::now();
            self.done = Some(h.join().unwrap_or_default());
            waited = Some(Waited {
                start,
                end: Instant::now(),
            });
        }
        (self.done.get_or_insert_with(Registry::default), waited)
    }

    /// Waits for a build still running, so its cache write is never cut off by the exit.
    pub fn finish(&mut self) {
        self.get();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::select::tests::proj;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn get_waits_once_and_never_builds_twice() {
        let builds = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&builds);
        let mut p = Prefetch::start(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Registry {
                projects: vec![proj("A", r"C:\w\alpha", "alpha")],
                phases: Vec::new(),
            }
        });
        let (first, waited) = p.get();
        assert_eq!(first.projects.len(), 1);
        assert!(waited.is_some(), "the first get joins the build");
        let (again, waited) = p.get();
        assert_eq!(again.projects.len(), 1);
        assert_eq!(waited, None, "a second get reuses the result");
        p.finish();
        assert_eq!(builds.load(Ordering::SeqCst), 1, "one scan per run");
    }

    #[test]
    fn a_build_that_panics_is_an_empty_list() {
        let mut p = Prefetch::start(|| panic!("scan failed"));
        assert_eq!(p.get().0.projects, Vec::<crate::projects::Project>::new());
    }
}
