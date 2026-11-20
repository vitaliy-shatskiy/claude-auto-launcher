//! The terminal shell of every screen: alternate buffer, flicker-free frame
//! writes, the event wait, hit-testing a click against a row map, and the scripted preview term.

mod preview_term;
pub mod screen_loop;
#[cfg(test)]
pub(crate) mod script_term;

use crate::frame::{FooterSpan, RowMap};
use crate::input::console::ConsoleInput;
use windows_sys::Win32::System::Console::{KEY_EVENT, MOUSE_EVENT};

use crate::input::{Event, MouseEvent, VtOutcome, decode_csi, decode_x10, mouse_from_record};
use crate::log::Logger;
use crate::out;
use crate::sys::{self, Handle};

pub use preview_term::{PreviewTerm, parse_preview_keys, parse_preview_size};

/// `?1049h` alternate buffer, `?25l` hidden cursor, OSC 0 tab title.
pub const ENTER_ALT: &str = "\x1b[?1049h\x1b[?25l\x1b]0;claude-auto\x07";
pub const EXIT_ALT: &str = "\x1b[?25h\x1b[?1049l";

/// Home, each line then erase-to-end, then erase below. No clear, no flicker.
/// No line feed after the last line: on the bottom row it scrolls the buffer by one.
pub fn frame_bytes(lines: &[String]) -> String {
    let mut s = String::from("\x1b[H");
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(l);
        s.push_str("\x1b[K");
    }
    s.push_str("\x1b[J");
    s
}

/// Only the lines that differ from `prev`, each addressed by its row, then erase below when the
/// frame got shorter. Empty when nothing changed: a hover or cursor move costs its own lines, not
/// the whole screen (at 250x68 a full colour frame is tens of KB per event).
pub fn frame_diff_bytes(prev: &[String], next: &[String]) -> String {
    let mut s = String::new();
    for (i, l) in next.iter().enumerate() {
        if prev.get(i) != Some(l) {
            s.push_str("\x1b[");
            s.push_str(&(i + 1).to_string());
            s.push_str(";1H");
            s.push_str(l);
            s.push_str("\x1b[K");
        }
    }
    if next.len() < prev.len() {
        s.push_str("\x1b[");
        s.push_str(&(next.len() + 1).to_string());
        s.push_str(";1H\x1b[J");
    }
    s
}

/// What the terminal shows, so the next paint writes only what changed. A size change or an
/// `invalidate` makes the next paint a full one.
#[derive(Debug, Default)]
pub struct Shown {
    lines: Vec<String>,
    size: (usize, usize),
}

impl Shown {
    pub fn invalidate(&mut self) {
        self.lines.clear();
    }

    /// The bytes that take the screen from what it shows to `lines` at `size`; records `lines`.
    pub fn update(&mut self, size: (usize, usize), lines: &[String]) -> String {
        let bytes = if self.lines.is_empty() || size != self.size {
            frame_bytes(lines)
        } else {
            frame_diff_bytes(&self.lines, lines)
        };
        lines.clone_into(&mut self.lines);
        self.size = size;
        bytes
    }
}

/// The last row activation, carried across screens (P14): a gesture's twin record reaches the
/// next screen, which must not read it as a press of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Activation {
    pub x: i32,
    pub y: i32,
    pub at: Option<u64>,
    pub queued: bool,
}

/// One waiting input record, decoded only as far as the twin test needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peeked {
    Mouse(MouseEvent),
    KeyDown(char),
    Other,
}

fn is_press(m: &MouseEvent) -> bool {
    m.left && !m.is_move && m.wheel == 0
}

/// Is a mouse button press waiting? A console mouse record counts when it is a left press; in
/// text-mouse mode the key records spell SGR (`ESC [ <`) or X10 (`ESC [ M`) reports, and one
/// counts when it decodes to a press. Motion, wheel, releases and keys never count.
pub fn press_waiting(recs: &[Peeked]) -> bool {
    if recs.iter().any(|r| matches!(r, Peeked::Mouse(m) if is_press(m))) {
        return true;
    }
    let text: Vec<char> = recs
        .iter()
        .filter_map(|r| if let Peeked::KeyDown(c) = r { Some(*c) } else { None })
        .collect();
    text.split(|c| *c == '\x1b').any(|seq| match seq {
        ['[', 'M', b, x, y, ..] => decode_x10(&['M', *b, *x, *y].map(u32::from)).is_some_and(|m| is_press(&m)),
        ['[', '<', ..] => {
            let end = seq.iter().position(|c| *c == 'M' || *c == 'm').map_or(seq.len(), |i| i + 1);
            let s: String = seq.iter().take(end).collect();
            matches!(decode_csi(&s), VtOutcome::Mouse(m) if is_press(&m))
        }
        _ => false,
    })
}

/// Why a screen stopped without an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiStop {
    /// A scripted key source ran out: how a finite preview ends.
    KeysExhausted,
    /// The console input could not be re-armed after a typed line (`read_line`).
    ConsoleLost,
}

impl std::fmt::Display for UiStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::KeysExhausted => "scripted keys exhausted",
            Self::ConsoleLost => "the console input could not be re-armed after a typed line",
        })
    }
}

impl std::error::Error for UiStop {}

/// What a screen needs from the terminal. The console and the preview implement it.
pub trait Term {
    fn size(&mut self) -> (usize, usize);
    fn paint(&mut self, lines: &[String]);
    /// The next event; `Resize` when the size moved from `size`; `Idle` after
    /// one empty poll (~60 ms) so a screen can look at background work.
    fn wait(&mut self, size: (usize, usize)) -> Result<Event, UiStop>;
    fn window_top(&mut self) -> i32;
    /// One non-blocking look: is another record already waiting?
    fn input_pending(&mut self) -> bool;
    /// One non-blocking look: is a mouse button press among the waiting records? Only a press
    /// marks a twin: motion (psmux streams it) and keys never do.
    fn press_pending(&mut self) -> bool {
        self.input_pending()
    }
    /// When the terminal delivered the record being handled (monotonic ms), if known.
    fn record_time(&self) -> Option<u64>;
    fn last_activation(&mut self) -> &mut Option<Activation>;
    /// Drops input queued while a child owned the screen.
    fn drain(&mut self);
    fn logger(&self) -> &Logger;
    /// Hands the console back for one cooked line typed after `prompt`
    /// at the bottom row, then re-arms it. Preview answers an empty line.
    fn read_line(&mut self, prompt: &str) -> String;
    /// The screen may no longer show the last frame (a new screen, a child, a typed line): the
    /// next paint rewrites every line.
    fn invalidate(&mut self) {}
    /// Bytes the last paint wrote to the terminal; 0 where nothing is written.
    fn painted_bytes(&self) -> usize {
        0
    }
}

/// The real console: CONOUT$ for size, the armed CONIN$ for events.
#[derive(Debug)]
pub struct ConsoleTerm {
    out: Option<Handle>,
    pub input: Option<ConsoleInput>,
    alt: bool,
    activation: Option<Activation>,
    logger: Logger,
    vt_restore: Option<(Handle, u32)>,
    /// A second CONIN$ handle onto the same queue, for peeking without consuming.
    peek: Option<Handle>,
    /// The lines on screen and the size they were written at: the next paint writes only the
    /// lines that differ. Empty = rewrite everything.
    shown: Shown,
    last_bytes: usize,
}

const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

impl ConsoleTerm {
    /// Opens the console; `alt` enters the alternate buffer (skipped when output is redirected).
    pub fn open(alt: bool, input: Option<ConsoleInput>, logger: Logger) -> Self {
        let out = Handle::open_console("CONOUT$");
        // VT processing on for the frames; the original mode comes back on close.
        let vt_restore = Handle::open_console("CONOUT$").and_then(|h| {
            let m = sys::console_mode(&h)?;
            (sys::set_console_mode(&h, m | ENABLE_VIRTUAL_TERMINAL_PROCESSING)).then_some((h, m))
        });
        if alt {
            out::raw(ENTER_ALT);
        }
        Self {
            out,
            input,
            alt,
            activation: None,
            logger,
            vt_restore,
            peek: Handle::open_console("CONIN$"),
            shown: Shown::default(),
            last_bytes: 0,
        }
    }

    /// Restores everything: the input mode, the buffer, the output mode. Idempotent.
    pub fn close(&mut self) {
        if let Some(i) = self.input.as_mut() {
            i.close();
        }
        if self.alt {
            out::raw(EXIT_ALT);
            self.alt = false;
        }
        if let Some((h, m)) = self.vt_restore.take() {
            sys::set_console_mode(&h, m);
        }
    }
}

impl Drop for ConsoleTerm {
    fn drop(&mut self) {
        self.close();
    }
}

impl Term for ConsoleTerm {
    fn size(&mut self) -> (usize, usize) {
        let (w, h, _) = self.out.as_ref().and_then(sys::window_size).unwrap_or((80, 25, 0));
        (usize::try_from(w).unwrap_or(80), usize::try_from(h).unwrap_or(25))
    }

    fn paint(&mut self, lines: &[String]) {
        if self.alt {
            let size = self.size();
            let bytes = self.shown.update(size, lines);
            if !bytes.is_empty() {
                out::raw(&bytes);
            }
            self.last_bytes = bytes.len();
        } else {
            for l in lines {
                out::line(l, out::Fg::Plain);
            }
        }
    }

    fn wait(&mut self, size: (usize, usize)) -> Result<Event, UiStop> {
        if self.size() != size {
            return Ok(Event::Resize);
        }
        let Some(input) = self.input.as_mut() else {
            return Err(UiStop::ConsoleLost);
        };
        if let Some(ev) = input.read_event(60) {
            return Ok(ev);
        }
        let pending = input.pending();
        input.flush_trace();
        if let Some(a) = self.activation.as_mut()
            && a.queued
            && !pending
        {
            a.queued = false;
        }
        Ok(Event::Idle)
    }

    fn window_top(&mut self) -> i32 {
        self.out.as_ref().and_then(sys::window_size).map_or(0, |(_, _, t)| t)
    }

    fn input_pending(&mut self) -> bool {
        self.input.as_ref().is_some_and(ConsoleInput::pending)
    }

    fn press_pending(&mut self) -> bool {
        if !self.input_pending() {
            return false;
        }
        let Some(h) = self.peek.as_ref() else { return false };
        let recs: Vec<Peeked> = sys::peek_records(h, 128)
            .iter()
            .map(|r| match u32::from(r.EventType) {
                MOUSE_EVENT => {
                    let (x, y, buttons, flags) = sys::mouse_fields(r);
                    Peeked::Mouse(mouse_from_record(x, y, buttons, flags))
                }
                KEY_EVENT => match sys::key_fields(r) {
                    (true, _, ch, _) => char::from_u32(u32::from(ch)).map_or(Peeked::Other, Peeked::KeyDown),
                    _ => Peeked::Other,
                },
                _ => Peeked::Other,
            })
            .collect();
        press_waiting(&recs)
    }

    fn record_time(&self) -> Option<u64> {
        self.input.as_ref().and_then(ConsoleInput::last_record_ms)
    }

    fn last_activation(&mut self) -> &mut Option<Activation> {
        &mut self.activation
    }

    fn drain(&mut self) {
        self.shown.invalidate();
        if let Some(i) = self.input.as_ref() {
            i.flush();
        }
    }

    fn invalidate(&mut self) {
        self.shown.invalidate();
    }

    fn painted_bytes(&self) -> usize {
        self.last_bytes
    }

    fn logger(&self) -> &Logger {
        &self.logger
    }

    fn read_line(&mut self, prompt: &str) -> String {
        let params = self.input.as_ref().map(ConsoleInput::reopen_params);
        if let Some(mut i) = self.input.take() {
            i.close();
        }
        let (_, h) = self.size();
        self.shown.invalidate();
        out::raw(&format!("\x1b[?25h\x1b[{h};1H\x1b[K{prompt}"));
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            line.clear();
        }
        if self.alt {
            out::raw("\x1b[?25l");
        }
        if let Some((mouse, redirected, trace)) = params {
            self.input = ConsoleInput::open(mouse, redirected, trace);
        }
        line.trim_end_matches(['\r', '\n']).to_owned()
    }
}

/// Where a click landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    None,
    Footer {
        span: FooterSpan,
        index: usize,
    },
    /// The project screen's action row, and the value cell under the pointer if any.
    Action {
        value: Option<String>,
    },
    /// A launch-shaped row's option cell.
    Cell {
        row: usize,
        name: String,
        value: String,
    },
    /// A launch-shaped row outside its cells.
    LaunchRow {
        row: usize,
        name: String,
    },
    /// A list row (absolute index).
    ListRow(i32),
}

/// Which footer hint a click landed on.
pub fn footer_hit(map: &RowMap, x: i32, y: i32, window_top: i32) -> Option<(FooterSpan, usize)> {
    let fy = map.footer_y?;
    let line = (y - window_top) - fy;
    let line = usize::try_from(line).ok()?;
    map.footer
        .iter()
        .enumerate()
        .find(|(_, s)| s.line == line && x >= s.start && x <= s.end)
        .map(|(i, s)| (s.clone(), i))
}

/// A buffer row to a list row index, or `None` outside the rows.
pub fn mouse_row(y: i32, first_row_y: i32, row_count: i32, window_top: i32) -> Option<i32> {
    if row_count <= 0 {
        return None;
    }
    let row = y - window_top - first_row_y;
    (0..row_count).contains(&row).then_some(row)
}

/// Footer first, then the action row, then a launch row and its cell, then a list row.
pub fn hit_at(map: Option<&RowMap>, x: i32, y: i32, window_top: i32) -> Hit {
    let Some(map) = map else { return Hit::None };
    if let Some((span, index)) = footer_hit(map, x, y, window_top) {
        return Hit::Footer { span, index };
    }
    let row_y = y - window_top;
    if let Some(a) = map.action.as_ref().filter(|a| a.y == row_y) {
        return Hit::Action {
            value: a.cells.iter().find(|c| x >= c.start && x <= c.end).map(|c| c.value.clone()),
        };
    }
    if !map.rows.is_empty() {
        let Some(row) = map.rows.iter().find(|r| r.y == row_y) else {
            return Hit::None;
        };
        return match row.cells.iter().find(|c| x >= c.start && x <= c.end) {
            Some(c) => Hit::Cell {
                row: row.index,
                name: row.name.clone(),
                value: c.value.clone(),
            },
            None => Hit::LaunchRow {
                row: row.index,
                name: row.name.clone(),
            },
        };
    }
    let Some(list) = map.list.as_ref() else { return Hit::None };
    if !list.row_ys.is_empty() {
        return list
            .row_ys
            .iter()
            .position(|v| *v == row_y)
            .map_or(Hit::None, |at| Hit::ListRow(list.start + i32::try_from(at).unwrap_or(0)));
    }
    mouse_row(y, list.first_row_y, list.row_count, window_top).map_or(Hit::None, |r| Hit::ListRow(list.start + r))
}

#[cfg(test)]
mod tests;
