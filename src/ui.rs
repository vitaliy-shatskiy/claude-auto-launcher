//! The terminal shell of every screen (`Ui.ps1` top): alternate buffer, flicker-free frame
//! writes, the event wait, hit-testing a click against a row map, and the scripted preview term.

pub mod screen_loop;

use std::collections::VecDeque;

use crate::frame::{FooterSpan, RowMap};
use crate::input::console::ConsoleInput;
use crate::input::{Event, KeyPress};
use crate::log::Logger;
use crate::out;
use crate::sys::{self, Handle};

/// `?1049h` alternate buffer, `?25l` hidden cursor, OSC 0 tab title.
pub const ENTER_ALT: &str = "\x1b[?1049h\x1b[?25l\x1b]0;claude-auto\x07";
pub const EXIT_ALT: &str = "\x1b[?25h\x1b[?1049l";

/// `Write-Frame`: home, each line then erase-to-end, then erase below. No clear, no flicker.
pub fn frame_bytes(lines: &[String]) -> String {
    let mut s = String::from("\x1b[H");
    for l in lines {
        s.push_str(l);
        s.push_str("\x1b[K\n");
    }
    s.push_str("\x1b[J");
    s
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

/// A scripted key source ran out: how a finite preview ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exhausted;

impl std::fmt::Display for Exhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("scripted keys exhausted")
    }
}

impl std::error::Error for Exhausted {}

/// What a screen needs from the terminal. The console and the preview implement it.
pub trait Term {
    fn size(&mut self) -> (usize, usize);
    fn paint(&mut self, lines: &[String]);
    /// `Wait-KeyOrResize`: the next event; `Resize` when the size moved from `size`.
    fn wait(&mut self, size: (usize, usize)) -> Result<Event, Exhausted>;
    fn window_top(&mut self) -> i32;
    /// One non-blocking look: is another record already waiting?
    fn input_pending(&mut self) -> bool;
    /// When the terminal delivered the record being handled (monotonic ms), if known.
    fn record_time(&self) -> Option<u64>;
    fn last_activation(&mut self) -> &mut Option<Activation>;
    /// Drops input queued while a child owned the screen.
    fn drain(&mut self);
    fn logger(&self) -> &Logger;
    /// `Read-ClaudeFreePath`: hands the console back for one cooked line typed after `prompt`
    /// at the bottom row, then re-arms it. Preview answers an empty line.
    fn read_line(&mut self, prompt: &str) -> String;
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
            out::raw(&frame_bytes(lines));
        } else {
            for l in lines {
                out::line(l, out::Fg::Plain);
            }
        }
    }

    fn wait(&mut self, size: (usize, usize)) -> Result<Event, Exhausted> {
        loop {
            if self.size() != size {
                return Ok(Event::Resize);
            }
            let Some(input) = self.input.as_mut() else { return Err(Exhausted) };
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
        }
    }

    fn window_top(&mut self) -> i32 {
        self.out.as_ref().and_then(sys::window_size).map_or(0, |(_, _, t)| t)
    }

    fn input_pending(&mut self) -> bool {
        self.input.as_ref().is_some_and(ConsoleInput::pending)
    }

    fn record_time(&self) -> Option<u64> {
        self.input.as_ref().and_then(ConsoleInput::last_record_ms)
    }

    fn last_activation(&mut self) -> &mut Option<Activation> {
        &mut self.activation
    }

    fn drain(&mut self) {
        if let Some(i) = self.input.as_ref() {
            i.flush();
        }
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
            },
        }
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

    fn wait(&mut self, _size: (usize, usize)) -> Result<Event, Exhausted> {
        self.keys.pop_front().map(Event::Key).ok_or(Exhausted)
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

/// `CLAUDE_AUTO_PREVIEW_SIZE` (`WxH`, new in the Rust port); the PowerShell preview's 78x24 default.
pub fn parse_preview_size(spec: Option<&str>) -> (usize, usize) {
    spec.and_then(|s| {
        let (w, h) = s.split_once(['x', 'X'])?;
        Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
    })
    .unwrap_or((78, 24))
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

/// `Get-ClaudeFooterHit`: which footer hint a click landed on.
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

/// `Get-ClaudeMouseRow`: a buffer row to a list row index, or `None` outside the rows.
pub fn mouse_row(y: i32, first_row_y: i32, row_count: i32, window_top: i32) -> Option<i32> {
    if row_count <= 0 {
        return None;
    }
    let row = y - window_top - first_row_y;
    (0..row_count).contains(&row).then_some(row)
}

/// `Get-HitAt`: footer first, then the action row, then a launch row and its cell, then a list row.
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
mod tests {
    use super::*;
    use crate::frame::{CellSpan, ListMap, RowHit};

    fn span(start: i32, end: i32, line: usize) -> FooterSpan {
        FooterSpan {
            start,
            end,
            key_start: 0,
            key_end: 0,
            key: "Enter".into(),
            ch: None,
            line,
        }
    }

    #[test]
    fn frame_write_homes_erases_each_line_and_below() {
        assert_eq!(frame_bytes(&["a".into(), "b".into()]), "\x1b[Ha\x1b[K\nb\x1b[K\n\x1b[J");
    }

    #[test]
    fn hit_footer_then_cells_then_rows() {
        let map = RowMap {
            rows: vec![RowHit {
                index: 1,
                name: "Remote".into(),
                y: 4,
                cells: vec![CellSpan {
                    start: 15,
                    end: 20,
                    value: "on".into(),
                }],
            }],
            footer_y: Some(10),
            footer: vec![span(5, 9, 0), span(2, 6, 1)],
            ..RowMap::default()
        };
        assert!(matches!(hit_at(Some(&map), 6, 10, 0), Hit::Footer { index: 0, .. }));
        assert!(
            matches!(hit_at(Some(&map), 3, 11, 0), Hit::Footer { index: 1, .. }),
            "the wrapped line"
        );
        assert_eq!(
            hit_at(Some(&map), 16, 4, 0),
            Hit::Cell {
                row: 1,
                name: "Remote".into(),
                value: "on".into()
            }
        );
        assert_eq!(
            hit_at(Some(&map), 2, 4, 0),
            Hit::LaunchRow {
                row: 1,
                name: "Remote".into()
            }
        );
        assert_eq!(hit_at(Some(&map), 2, 5, 0), Hit::None);
        assert_eq!(
            hit_at(Some(&map), 16, 104, 100),
            hit_at(Some(&map), 16, 4, 0),
            "window top is subtracted"
        );
    }

    #[test]
    fn list_rows_by_arithmetic_or_explicit_ys() {
        let map = RowMap {
            list: Some(ListMap {
                first_row_y: 3,
                row_count: 4,
                start: 10,
                row_ys: Vec::new(),
            }),
            ..RowMap::default()
        };
        assert_eq!(hit_at(Some(&map), 0, 5, 0), Hit::ListRow(12));
        assert_eq!(hit_at(Some(&map), 0, 7, 0), Hit::None);
        let gaps = RowMap {
            list: Some(ListMap {
                row_ys: vec![3, 5],
                start: 0,
                ..ListMap::default()
            }),
            ..RowMap::default()
        };
        assert_eq!(hit_at(Some(&gaps), 0, 4, 0), Hit::None, "a click on a gap is no row");
        assert_eq!(hit_at(Some(&gaps), 0, 5, 0), Hit::ListRow(1));
    }

    #[test]
    fn preview_keys_and_size() {
        let k = parse_preview_keys("DownArrow,RightArrow,r,Enter").unwrap();
        assert_eq!(k.len(), 4);
        assert!(parse_preview_keys("Bogus").is_err());
        assert_eq!(parse_preview_size(Some("120x30")), (120, 30));
        assert_eq!(parse_preview_size(None), (78, 24));
    }
}
