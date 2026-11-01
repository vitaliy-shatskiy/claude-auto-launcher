//! Debug timings (`--debug`, `CLAUDE_AUTO_DEBUG=1`): named spans with parent/child nesting, held
//! in memory for the footer line, the F12 screen and the run log. Off is `None`: every call
//! returns before reading the clock, so the off path costs no syscall and no I/O.

pub mod summary;
pub mod view;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// One measured phase. Times are microseconds from the run's origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub name: String,
    /// Index of the enclosing span in [`Timings::spans`].
    pub parent: Option<usize>,
    pub start_us: u64,
    /// `None` while the span is open.
    pub dur_us: Option<u64>,
    /// Closed by the end of an enclosing span, not by its own end (an early return).
    pub aborted: bool,
    /// Dropped by [`Timings::discard`]: never shown, never logged.
    discarded: bool,
    logged: bool,
}

#[derive(Debug)]
struct Inner {
    origin: Instant,
    spans: Vec<Span>,
    /// The open spans, innermost last: a new span's parent is the last one.
    open: Vec<usize>,
    color: bool,
    ascii: bool,
}

/// The run's span recorder, shared by handle (cheap to clone). `Default` is off.
#[derive(Debug, Clone, Default)]
pub struct Timings(Option<Rc<RefCell<Inner>>>);

/// A span handle; the off recorder hands out one that ends nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanId(usize);

const NONE: SpanId = SpanId(usize::MAX);

fn micros(origin: Instant, at: Instant) -> u64 {
    u64::try_from(at.saturating_duration_since(origin).as_micros()).unwrap_or(u64::MAX)
}

/// Microseconds as milliseconds rounded to two decimals, for the log and the F12 screen.
pub fn ms(us: u64) -> f64 {
    f64::from(u32::try_from(us.saturating_add(5) / 10).unwrap_or(u32::MAX)) / 100.0
}

impl Timings {
    /// A recording run whose clock starts at `origin`.
    pub fn on(origin: Instant) -> Self {
        Self(Some(Rc::new(RefCell::new(Inner {
            origin,
            spans: Vec::new(),
            open: Vec::new(),
            color: false,
            ascii: false,
        }))))
    }

    pub fn is_on(&self) -> bool {
        self.0.is_some()
    }

    /// The footer's colour and glyph decisions, known once the UI has a style.
    pub fn set_style(&self, color: bool, ascii: bool) {
        if let Some(i) = &self.0 {
            let mut i = i.borrow_mut();
            i.color = color;
            i.ascii = ascii;
        }
    }

    pub(crate) fn style(&self) -> (bool, bool) {
        self.0.as_ref().map_or((false, false), |i| {
            let i = i.borrow();
            (i.color, i.ascii)
        })
    }

    /// Opens a span named by `parts` joined with `:`; the name is built only when on.
    pub fn begin(&self, parts: &[&str]) -> SpanId {
        if self.0.is_none() {
            return NONE;
        }
        self.begin_at(parts, Instant::now())
    }

    pub fn begin_at(&self, parts: &[&str], at: Instant) -> SpanId {
        let Some(i) = &self.0 else { return NONE };
        let mut i = i.borrow_mut();
        let start_us = micros(i.origin, at);
        let parent = i.open.last().copied();
        let id = i.spans.len();
        i.spans.push(Span {
            name: parts.join(":"),
            parent,
            start_us,
            dur_us: None,
            aborted: false,
            discarded: false,
            logged: false,
        });
        i.open.push(id);
        SpanId(id)
    }

    pub fn end(&self, id: SpanId) {
        if self.0.is_none() || id == NONE {
            return;
        }
        self.end_at(id, Instant::now());
    }

    /// Closes a span and every span still open inside it (those marked aborted), so an early
    /// return never leaves a span open to adopt later ones. A second end keeps the first.
    pub fn end_at(&self, id: SpanId, at: Instant) {
        let Some(i) = &self.0 else { return };
        let mut i = i.borrow_mut();
        let Some(pos) = i.open.iter().position(|o| *o == id.0) else {
            return;
        };
        let now = micros(i.origin, at);
        let closing: Vec<usize> = i.open.drain(pos..).collect();
        for idx in closing {
            if let Some(s) = i.spans.get_mut(idx) {
                s.dur_us = Some(now.saturating_sub(s.start_us));
                s.aborted = idx != id.0;
            }
        }
    }

    /// Drops a span that measured nothing meaningful: it is never shown or logged, and its
    /// children move up to its parent.
    pub fn discard(&self, id: SpanId) {
        let Some(i) = &self.0 else { return };
        let mut i = i.borrow_mut();
        let Some(parent) = i.spans.get(id.0).map(|s| s.parent) else {
            return;
        };
        for s in i.spans.iter_mut().filter(|s| s.parent == Some(id.0)) {
            s.parent = parent;
        }
        if let Some(s) = i.spans.get_mut(id.0) {
            s.discarded = true;
            s.logged = true;
        }
        i.open.retain(|o| *o != id.0);
    }

    /// Ends a key span at its screen's next frame, or discards it when a nested screen ran in
    /// between: that frame then measures the nested screen, not the key.
    pub fn end_key(&self, id: SpanId) {
        if self.0.is_none() || id == NONE {
            return;
        }
        let nested = self
            .0
            .as_ref()
            .is_some_and(|i| i.borrow().spans.iter().skip(id.0 + 1).any(|s| s.name.starts_with("screen:")));
        if nested { self.discard(id) } else { self.end(id) }
    }

    /// A span that ends when the guard drops.
    pub fn scope(&self, parts: &[&str]) -> Scope<'_> {
        Scope {
            t: self,
            id: self.begin(parts),
        }
    }

    /// Every span of the run, in start order.
    pub fn spans(&self) -> Vec<Span> {
        self.0.as_ref().map_or_else(Vec::new, |i| i.borrow().spans.clone())
    }

    /// Microseconds since the origin, for the open spans of the F12 screen.
    pub fn now_us(&self) -> u64 {
        self.0.as_ref().map_or(0, |i| micros(i.borrow().origin, Instant::now()))
    }

    /// The duration of the last span named `name` that ended by itself, in whole ms rounded up,
    /// so a phase that ran reads as at least 1.
    pub fn latest_ms(&self, name: &str) -> Option<u64> {
        let i = self.0.as_ref()?.borrow();
        i.spans
            .iter()
            .filter(|s| s.name == name && !s.aborted && !s.discarded)
            .filter_map(|s| s.dur_us.map(|d| (s.start_us.saturating_add(d), d)))
            .max_by_key(|(end, _)| *end)
            .map(|(_, d)| d.div_ceil(1000))
    }

    /// The ended spans not yet handed to the log; marks them handed.
    pub fn take_unlogged(&self) -> Vec<TimingRecord> {
        let Some(i) = &self.0 else { return Vec::new() };
        let mut i = i.borrow_mut();
        let names: Vec<String> = i.spans.iter().map(|s| s.name.clone()).collect();
        let mut out = Vec::new();
        for s in i.spans.iter_mut().filter(|s| !s.logged) {
            if let Some(d) = s.dur_us {
                s.logged = true;
                out.push(TimingRecord {
                    name: s.name.clone(),
                    parent: s.parent.and_then(|p| names.get(p).cloned()),
                    ms: ms(d),
                    start_ms: ms(s.start_us),
                    aborted: s.aborted,
                });
            }
        }
        out
    }
}

/// One ended span as the run log's `timing` record carries it.
#[derive(Debug, Clone, PartialEq)]
pub struct TimingRecord {
    pub name: String,
    /// The enclosing span's name; `None` for a root span.
    pub parent: Option<String>,
    pub ms: f64,
    /// Start, in ms from the process start.
    pub start_ms: f64,
    /// Ended by an enclosing span (an early return), not by itself.
    pub aborted: bool,
}

/// Ends its span on drop.
#[derive(Debug)]
pub struct Scope<'a> {
    t: &'a Timings,
    id: SpanId,
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        self.t.end(self.id);
    }
}

/// The launcher's own switch, only as the sole argument (like `--launcher-version`).
pub const DEBUG_FLAG: &str = "--launcher-debug";

/// `--launcher-debug` as the only argument, or `CLAUDE_AUTO_DEBUG=1`. claude's own `--debug`
/// is never read here.
pub fn debug_requested(args: &[String], env: Option<&str>) -> bool {
    env == Some("1") || is_debug_flag(args)
}

/// `args` is exactly `--launcher-debug`: the one argument the launcher takes away.
pub fn is_debug_flag(args: &[String]) -> bool {
    args.len() == 1 && args.first().is_some_and(|a| a == DEBUG_FLAG)
}

#[cfg(test)]
mod tests;
