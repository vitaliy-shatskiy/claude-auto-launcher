//! The scripted preview term: a fixed size, scripted keys, frames captured or printed. Exports
//! `PreviewTerm`, `parse_preview_keys`, `parse_preview_size`.

use std::collections::VecDeque;

use super::{Activation, Term, UiStop};
use crate::input::{Event, KeyPress};
use crate::log::Logger;
use crate::out;

/// Preview: a fixed size, scripted keys, frames printed linearly so a harness can capture them.
#[derive(Debug)]
pub struct PreviewTerm {
    pub size: (usize, usize),
    pub keys: VecDeque<KeyPress>,
    /// Every frame painted, in order, when capturing (tests); printed otherwise.
    pub frames: Option<Vec<Vec<String>>>,
    activation: Option<Activation>,
    logger: Logger,
}

impl PreviewTerm {
    pub fn new(size: (usize, usize), keys: VecDeque<KeyPress>, capture: bool) -> Self {
        Self {
            size,
            keys,
            frames: capture.then(Vec::new),
            activation: None,
            logger: Logger {
                root: std::path::PathBuf::new(),
                run_id: String::new(),
                enabled: false,
                timings: crate::timing::Timings::default(),
            },
        }
    }

    /// The run's logger, so a preview in debug mode shares the run's timings.
    #[must_use]
    pub fn with_logger(mut self, logger: Logger) -> Self {
        self.logger = logger;
        self
    }
}

impl Term for PreviewTerm {
    fn size(&mut self) -> (usize, usize) {
        self.size
    }

    fn paint(&mut self, lines: &[String]) {
        match self.frames.as_mut() {
            Some(f) => f.push(lines.to_vec()),
            None => {
                for l in lines {
                    out::line(l, out::Fg::Plain);
                }
            }
        }
    }

    fn wait(&mut self, _size: (usize, usize)) -> Result<Event, UiStop> {
        self.keys.pop_front().map(Event::Key).ok_or(UiStop::KeysExhausted)
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
        &mut self.activation
    }

    fn drain(&mut self) {}

    fn logger(&self) -> &Logger {
        &self.logger
    }

    fn read_line(&mut self, _prompt: &str) -> String {
        String::new()
    }
}

/// `CLAUDE_AUTO_PREVIEW_KEYS`: comma separated key names or single characters.
pub fn parse_preview_keys(spec: &str) -> Result<VecDeque<KeyPress>, String> {
    spec.split(',')
        .filter(|k| !k.is_empty())
        .map(|k| crate::input::scripted_key(k).ok_or_else(|| format!("unknown key name '{k}'")))
        .collect()
}

/// `CLAUDE_AUTO_PREVIEW_SIZE` (`WxH`); 78x24 by default.
pub fn parse_preview_size(spec: Option<&str>) -> (usize, usize) {
    spec.and_then(|s| {
        let (w, h) = s.split_once(['x', 'X'])?;
        Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
    })
    .unwrap_or((78, 24))
}
