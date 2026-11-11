//! Test-only `ScriptTerm`: a fixed size, scripted events (mouse included) replayed in order, every
//! frame kept in `inner`, and a canned answer for `read_line` (`asked` counts the reads).

use std::collections::VecDeque;

use crate::input::Event;
use crate::ui::{Activation, PreviewTerm, Term, UiStop};

#[derive(Debug)]
pub(crate) struct ScriptTerm {
    pub(crate) inner: PreviewTerm,
    size: (usize, usize),
    q: VecDeque<Event>,
    answer: String,
    pub(crate) asked: usize,
}

impl ScriptTerm {
    /// `events` replayed in order; once they run out the screen stops with `KeysExhausted`.
    pub(crate) fn new(size: (usize, usize), events: impl IntoIterator<Item = Event>) -> Self {
        Self {
            inner: PreviewTerm::new(size, VecDeque::new(), true),
            size,
            q: events.into_iter().collect(),
            answer: String::new(),
            asked: 0,
        }
    }

    /// `read_line` answers `answer` (default: an empty line).
    pub(crate) fn answering(mut self, answer: &str) -> Self {
        answer.clone_into(&mut self.answer);
        self
    }
}

impl Term for ScriptTerm {
    fn size(&mut self) -> (usize, usize) {
        self.size
    }
    fn paint(&mut self, lines: &[String]) {
        self.inner.paint(lines);
    }
    fn wait(&mut self, _s: (usize, usize)) -> Result<Event, UiStop> {
        self.q.pop_front().ok_or(UiStop::KeysExhausted)
    }
    fn window_top(&mut self) -> i32 {
        0
    }
    fn input_pending(&mut self) -> bool {
        false
    }
    fn record_time(&self) -> Option<u64> {
        None
    }
    fn last_activation(&mut self) -> &mut Option<Activation> {
        self.inner.last_activation()
    }
    fn drain(&mut self) {}
    fn logger(&self) -> &crate::log::Logger {
        self.inner.logger()
    }
    fn read_line(&mut self, _prompt: &str) -> String {
        self.asked += 1;
        self.answer.clone()
    }
}
