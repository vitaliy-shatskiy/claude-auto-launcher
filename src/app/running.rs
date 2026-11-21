//! The launch page's running panel, read off the UI thread: one read at a time on a `Background`,
//! the next one `EVERY_POLLS` idle polls after the last answer. Exports `RunningWatch`.

use crate::background::Background;
use crate::launch::frame::RunningSession;

/// Idle polls (~60 ms each) between two reads: about two seconds.
const EVERY_POLLS: u32 = 32;

/// The running sessions as last read, and the read in flight.
pub(super) struct RunningWatch<F> {
    read: F,
    job: Option<Background<Vec<RunningSession>>>,
    polls: u32,
}

impl<F: Fn() -> Vec<RunningSession> + Send + Clone + 'static> RunningWatch<F> {
    /// Starts the first read at once.
    pub(super) fn start(read: F) -> Self {
        let mut w = Self { read, job: None, polls: 0 };
        w.begin();
        w
    }

    fn begin(&mut self) {
        // A read only lists session records: an exit never needs to wait for it.
        self.job = Some(Background::start("running", self.read.clone()).unjoined());
    }

    /// One idle poll: a read's answer once it lands (a failed read is no answer); with none in
    /// flight, the next read starts every `EVERY_POLLS` polls.
    pub(super) fn poll(&mut self) -> Option<Vec<RunningSession>> {
        if let Some(job) = self.job.as_mut() {
            let answer = job.poll()?.as_ref().ok().cloned();
            self.job = None;
            self.polls = 0;
            return answer;
        }
        self.polls += 1;
        if self.polls >= EVERY_POLLS {
            self.begin();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Polls until the read in flight answers.
    fn answer<F: Fn() -> Vec<RunningSession> + Send + Clone + 'static>(w: &mut RunningWatch<F>) -> Vec<RunningSession> {
        loop {
            if let Some(a) = w.poll() {
                return a;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn the_panel_is_read_off_the_ui_thread_one_read_at_a_time_and_again_after_its_interval() {
        // Given: a read that counts itself and names its count.
        let reads = Arc::new(AtomicUsize::new(0));
        let r = Arc::clone(&reads);
        let mut w = RunningWatch::start(move || {
            let n = r.fetch_add(1, Ordering::SeqCst) + 1;
            vec![RunningSession {
                name: format!("read {n}"),
                ..RunningSession::default()
            }]
        });
        // When the first read lands, then it is the answer.
        assert_eq!(answer(&mut w)[0].name, "read 1");
        // And: the polls before the interval start no read.
        for _ in 1..EVERY_POLLS {
            assert!(w.poll().is_none());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1, "no read per idle poll");
        // When the interval is up, then one new read runs and lands.
        assert!(w.poll().is_none());
        assert_eq!(answer(&mut w)[0].name, "read 2");
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}
