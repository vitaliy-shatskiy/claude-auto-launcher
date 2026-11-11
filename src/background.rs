//! One job on its own thread while a screen is up (the project registry, the update check): its
//! answer is read without blocking (`poll`) or waited for (`wait`). Dropping it joins the job, so
//! an exit never cuts a job's file write in half, unless the job is `unjoined` (atomic writes). A panic in the job is its answer as an error and
//! prints nothing: the screen may be up.

use std::any::Any;
use std::cell::Cell;
use std::sync::Once;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A wait after Enter longer than this shows a status line instead of a frozen frame.
pub const SLOW: Duration = Duration::from_millis(150);

/// How often a wait past `SLOW` asks whether the user left it.
const POLL: Duration = Duration::from_millis(60);

/// The user left a wait (Esc, Ctrl+C) before the job answered; the job runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Left;

/// The job's result, or the panic that ended it, in words.
pub type Answer<T> = Result<T, String>;

/// How long a `wait` blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waited {
    pub start: Instant,
    pub end: Instant,
}

/// A job in flight or finished.
#[derive(Debug)]
pub struct Background<T> {
    rx: Option<Receiver<T>>,
    handle: Option<JoinHandle<()>>,
    done: Option<Answer<T>>,
    /// A drop waits for the job (the default) or leaves it running (`unjoined`).
    joins: bool,
}

thread_local! {
    /// Set on a job's thread: the panic hook stays silent there.
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

/// The panic hook skips a job's thread: the message is its answer, not text over the screen.
fn quiet_job_panics() {
    // The hook takes no context, so the switch is this process-wide `Once` plus `QUIET`.
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !QUIET.with(Cell::get) {
                prev(info);
            }
        }));
    });
}

fn panic_text(p: &(dyn Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "the job panicked".into())
}

impl<T: Send + 'static> Background<T> {
    /// Starts `job` on a thread named `name`. A thread that cannot start runs the job here, at once.
    pub fn start(name: &str, job: impl FnOnce() -> T + Send + Clone + 'static) -> Self {
        quiet_job_panics();
        let (tx, rx) = channel();
        let body = job.clone();
        let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
            QUIET.with(|q| q.set(true));
            // The receiver is gone only when nobody wants the answer any more.
            tx.send(body()).unwrap_or_default();
        });
        match spawned {
            Ok(h) => Self {
                rx: Some(rx),
                handle: Some(h),
                done: None,
                joins: true,
            },
            Err(_) => Self {
                rx: None,
                handle: None,
                done: Some(Ok(job())),
                joins: true,
            },
        }
    }

    /// The channel closed without an answer: the job panicked; its message is in the join.
    fn lost(&mut self) -> Answer<T> {
        let why = match self.handle.take().map(JoinHandle::join) {
            Some(Err(p)) => panic_text(p.as_ref()),
            Some(Ok(())) | None => "the job ended without an answer".into(),
        };
        Err(why)
    }

    /// The answer once it is in, without blocking.
    pub fn poll(&mut self) -> Option<&Answer<T>> {
        if let Some(rx) = &self.rx {
            let got = match rx.try_recv() {
                Ok(a) => Ok(a),
                Err(TryRecvError::Disconnected) => self.lost(),
                Err(TryRecvError::Empty) => return None,
            };
            self.done = Some(got);
            self.rx = None;
        }
        self.done.as_ref()
    }

    /// The answer, waiting for it. A job still running after `SLOW` calls `on_slow` once (a status
    /// line), then the wait goes on. `Waited` only when this call found the answer unread.
    pub fn wait(&mut self, on_slow: impl FnOnce()) -> (&Answer<T>, Option<Waited>) {
        let waited = self.wait_or_leave(on_slow, || false).unwrap_or_default();
        let done = self.done.get_or_insert_with(|| Err("the job ended without an answer".into()));
        (done, waited)
    }

    /// `wait`, but past `SLOW` it asks `left` every `POLL`: `true` (Esc, Ctrl+C) stops waiting and
    /// leaves the job running and unread, so a later wait picks it up again.
    pub fn wait_or_leave(&mut self, on_slow: impl FnOnce(), mut left: impl FnMut() -> bool) -> Result<Option<Waited>, Left> {
        let Some(rx) = self.rx.take() else { return Ok(None) };
        let start = Instant::now();
        let got = match rx.recv_timeout(SLOW) {
            Ok(a) => Some(a),
            Err(RecvTimeoutError::Disconnected) => None,
            Err(RecvTimeoutError::Timeout) => {
                on_slow();
                loop {
                    match rx.recv_timeout(POLL) {
                        Ok(a) => break Some(a),
                        Err(RecvTimeoutError::Disconnected) => break None,
                        Err(RecvTimeoutError::Timeout) => {
                            if left() {
                                self.rx = Some(rx);
                                return Err(Left);
                            }
                        }
                    }
                }
            }
        };
        let answer = match got {
            Some(a) => Ok(a),
            None => self.lost(),
        };
        self.done = Some(answer);
        Ok(Some(Waited {
            start,
            end: Instant::now(),
        }))
    }

    /// Leaves an unread job behind: no wait, now or on drop; its answer is never read.
    pub fn detach(&mut self) {
        if self.done.is_none() {
            self.rx = None;
            self.handle = None;
        }
    }

    /// Dropping this job leaves it running instead of joining it: for a job whose writes are
    /// atomic, so an exit (cancel, error) never waits for it.
    #[must_use]
    pub fn unjoined(mut self) -> Self {
        self.joins = false;
        self
    }

    /// The answer if `poll` or `wait` already read it.
    pub fn done(&self) -> Option<&Answer<T>> {
        self.done.as_ref()
    }
}

impl<T> Drop for Background<T> {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take().filter(|_| self.joins) {
            // A panic here was already read as the answer, or nobody asked for it.
            h.join().unwrap_or_default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex};

    /// A job that runs until the test lets it go.
    fn gated() -> (Sender<()>, impl FnOnce() -> u32 + Send + Clone + 'static) {
        let (go, gate) = channel::<()>();
        let gate = Arc::new(Mutex::new(gate));
        // Bounded: a mutant that never lets the job go fails the test instead of hanging it.
        (go, move || {
            gate.lock().unwrap().recv_timeout(Duration::from_secs(3)).unwrap_or_default();
            7
        })
    }

    #[test]
    fn an_unjoined_job_is_left_running_on_drop() {
        // Given: a job that runs until the test lets it go, whose drop must not wait.
        let (go, job) = gated();
        let finished = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&finished);
        let b = Background::start("t", move || {
            let v = job();
            flag.store(true, Ordering::SeqCst);
            v
        })
        .unjoined();
        // When: it is dropped while running.
        drop(b);
        // Then: the drop returned before the job ended.
        assert!(!finished.load(Ordering::SeqCst), "the drop joined the job");
        go.send(()).unwrap_or_default();
    }

    #[test]
    fn wait_reads_once_and_never_runs_twice() {
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&runs);
        let mut b = Background::start("t", move || counter.fetch_add(1, Ordering::SeqCst) + 1);
        let (first, waited) = b.wait(|| {});
        assert_eq!(first, &Ok(1));
        assert!(waited.is_some(), "the first wait reads the job");
        let (again, waited) = b.wait(|| {});
        assert_eq!(again, &Ok(1));
        assert_eq!(waited, None, "a second wait reuses the answer");
        drop(b);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn poll_never_blocks_and_sees_the_answer_once_it_is_in() {
        let (go, job) = gated();
        let mut b = Background::start("t", job);
        assert_eq!(b.poll(), None, "still running");
        go.send(()).unwrap();
        assert_eq!(b.wait(|| {}).0, &Ok(7));
        assert_eq!(b.poll(), Some(&Ok(7)));
    }

    #[test]
    fn a_slow_job_shows_the_status_once_then_still_waits() {
        let (go, job) = gated();
        let mut b = Background::start("t", job);
        let mut shown = 0;
        let (a, waited) = b.wait(|| {
            shown += 1;
            go.send(()).unwrap();
        });
        assert_eq!(a, &Ok(7), "the wait went on after the status");
        assert!(waited.is_some());
        assert_eq!(shown, 1);
    }

    #[test]
    fn a_slow_wait_can_be_left_and_a_later_wait_still_reads_the_job() {
        // Given: a job still running past the status line.
        let (go, job) = gated();
        let mut b = Background::start("t", job);
        let mut asked = 0;
        // When: the user leaves the wait at the first poll.
        let left = b.wait_or_leave(
            || {},
            || {
                asked += 1;
                true
            },
        );
        // Then: the wait ended unread; a later wait goes on and reads the answer.
        assert_eq!((left, asked), (Err(Left), 1));
        assert_eq!(b.poll(), None, "the job runs on");
        go.send(()).unwrap();
        assert_eq!(b.wait(|| {}).0, &Ok(7));
    }

    #[test]
    fn a_job_that_panics_answers_its_message() {
        let mut b = Background::<u32>::start("t", || panic!("scan failed"));
        assert_eq!(b.wait(|| {}).0, &Err("scan failed".to_owned()));
    }

    #[test]
    fn drop_waits_for_a_slow_job() {
        let (go, job) = gated();
        let finished = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&finished);
        let b = Background::start("t", move || {
            let v = job();
            flag.store(true, Ordering::SeqCst);
            v
        });
        let release = std::thread::spawn(move || go.send(()).unwrap());
        drop(b);
        assert!(finished.load(Ordering::SeqCst), "the drop joined the job");
        release.join().unwrap();
    }
}
