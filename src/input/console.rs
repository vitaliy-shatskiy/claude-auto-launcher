//! The armed console: `CONIN$`, mode arm and bit-exact restore, one record queue for keys, mouse
//! and resize, ESC-sequence peeking, and the switch to VT text mouse (`Input.ps1`, CS-1653).

use std::io::Write as _;
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Console::{KEY_EVENT, MOUSE_EVENT, WINDOW_BUFFER_SIZE_EVENT};

use super::{AfterEsc, EscSource, Event, KeyPress, VtOutcome, after_esc, esc_wait_ms, key_from_record, mouse_from_record};
use crate::sys::{self, Handle, RawRecord};

const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
/// conhost suspends output on ctrl+s and drops it while line input is on; echo needs line input.
const LINE_AND_ECHO_INPUT: u32 = 0x0002 | 0x0004;
const ENABLE_WINDOW_INPUT: u32 = 0x0008;
const ENABLE_MOUSE_INPUT: u32 = 0x0010;
const ENABLE_QUICK_EDIT_MODE: u32 = 0x0040;
const ENABLE_EXTENDED_FLAGS: u32 = 0x0080;

/// Any-motion tracking with SGR encoding: hover needs motion, SGR reads columns past 95.
pub const TEXT_MOUSE_ON: &str = "\x1b[?1003h\x1b[?1006h";
/// Every tracking mode reset on exit, including the two the old launcher requested.
pub const TEXT_MOUSE_OFF: &str = "\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l";

/// psmux forwards bare motion to a pane only when the pane program itself enabled DECSET 1003;
/// console mouse input alone gets clicks, never motion. So inside a tmux-like host (`TMUX` set,
/// psmux sets it in every pane) with stdout on the console, any-motion SGR is requested at arm
/// time, `ENABLE_MOUSE_INPUT` kept. Never elsewhere: in a plain Windows Terminal tab 1003 turns
/// console mouse records into a per-pixel text flood.
pub fn wants_vt_motion(tmux: Option<&str>, stdout_console: bool, mouse: bool) -> bool {
    mouse && stdout_console && tmux.is_some_and(|t| !t.is_empty())
}

/// The arm: the original mode plus window, mouse and extended flags, minus `QuickEdit` and
/// processed, line and echo input (Ctrl+C and Ctrl+S become keys). Keyboard-only keeps `QuickEdit` and asks for no mouse.
pub fn armed_mode(original: u32, mouse: bool) -> u32 {
    if mouse {
        (original | ENABLE_WINDOW_INPUT | ENABLE_MOUSE_INPUT | ENABLE_EXTENDED_FLAGS)
            & !(ENABLE_QUICK_EDIT_MODE | ENABLE_PROCESSED_INPUT | LINE_AND_ECHO_INPUT)
    } else {
        (original | ENABLE_WINDOW_INPUT) & !(ENABLE_PROCESSED_INPUT | LINE_AND_ECHO_INPUT)
    }
}

/// `CLAUDE_AUTO_INPUT_TRACE=1`: every raw record, buffered, flushed only while idle.
#[derive(Debug, Default)]
struct Trace {
    path: Option<std::path::PathBuf>,
    buf: Vec<String>,
    count: usize,
}

const TRACE_MAX: usize = 20_000;

impl Trace {
    fn add(&mut self, line: String) {
        if self.path.is_none() || self.count >= TRACE_MAX {
            return;
        }
        self.buf.push(line);
        self.count += 1;
        if self.count >= TRACE_MAX {
            self.buf.push("-- trace cap reached --".into());
        }
        if self.buf.len() >= 256 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        let Some(p) = &self.path else { return };
        if self.buf.is_empty() {
            return;
        }
        let mut text = self.buf.join("\r\n");
        text.push_str("\r\n");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p)
            && f.write_all(text.as_bytes()).is_err()
        {
            // Fail-open: a trace that cannot be written is not worth a broken menu.
        }
        self.buf.clear();
    }
}

/// The armed console input. Dropping it restores the mode.
#[expect(clippy::struct_excessive_bools, reason = "independent one-way latches of the console")]
#[derive(Debug)]
pub struct ConsoleInput {
    handle: Handle,
    original_mode: u32,
    armed_mode: u32,
    mouse: bool,
    closed: bool,
    saw_console_mouse: bool,
    text_mouse: bool,
    /// Any-motion SGR was requested at arm time (inside psmux/tmux): reset on close.
    vt_motion: bool,
    output_redirected: bool,
    epoch: Instant,
    last_record_ms: Option<u64>,
    trace: Trace,
}

impl ConsoleInput {
    /// Arms the console. `None` means no console here: the caller keeps its keyboard-free path.
    /// `mouse` false is `CLAUDE_AUTO_NO_MOUSE=1`: keys only, `QuickEdit` untouched.
    pub fn open(mouse: bool, output_redirected: bool, trace_dir: Option<std::path::PathBuf>) -> Option<Self> {
        let handle = Handle::open_console("CONIN$")?;
        let original = sys::console_mode(&handle)?;
        let armed = armed_mode(original, mouse);
        if !sys::set_console_mode(&handle, armed) {
            return None;
        }
        let vt_motion = wants_vt_motion(std::env::var("TMUX").ok().as_deref(), !output_redirected, mouse);
        if vt_motion {
            write_out(TEXT_MOUSE_ON);
        }
        Some(Self {
            handle,
            original_mode: original,
            armed_mode: armed,
            mouse,
            closed: false,
            saw_console_mouse: false,
            text_mouse: false,
            vt_motion,
            output_redirected,
            epoch: Instant::now(),
            last_record_ms: None,
            trace: Trace {
                path: trace_dir,
                ..Trace::default()
            },
        })
    }

    /// Restores the mode and verifies it came back bit-exact. Idempotent; closed only on success.
    pub fn close(&mut self) -> bool {
        if self.closed {
            return true;
        }
        if self.text_mouse || self.vt_motion {
            write_out(TEXT_MOUSE_OFF);
        }
        self.trace.flush();
        let ok = sys::set_console_mode(&self.handle, self.original_mode) && sys::console_mode(&self.handle) == Some(self.original_mode);
        if ok {
            self.closed = true;
        } else {
            crate::out::err_line("claude-auto: the console mode could not be restored; QuickEdit may still be off in this terminal");
        }
        ok
    }

    /// What `open` was called with, to re-arm after a cooked-mode read.
    pub fn reopen_params(&self) -> (bool, bool, Option<std::path::PathBuf>) {
        (self.mouse, self.output_redirected, self.trace.path.clone())
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Milliseconds (monotonic) at which the terminal delivered the last record.
    pub fn last_record_ms(&self) -> Option<u64> {
        self.last_record_ms
    }

    /// One non-blocking look at the queue depth; never consumes.
    pub fn pending(&self) -> bool {
        !self.closed && sys::pending_events(&self.handle) > 0
    }

    /// Drops everything queued while a child owned the screen.
    pub fn flush(&self) -> bool {
        !self.closed && sys::flush_input(&self.handle)
    }

    pub fn flush_trace(&mut self) {
        self.trace.flush();
    }

    fn read_raw(&mut self, timeout_ms: u32) -> Option<RawRecord> {
        if !sys::wait_signalled(&self.handle, timeout_ms) {
            return None;
        }
        let rec = sys::read_record(&self.handle)?;
        if self.trace.path.is_some() {
            self.trace.add(format_record(&rec));
        }
        self.last_record_ms = u64::try_from(self.epoch.elapsed().as_millis()).ok();
        if u32::from(rec.EventType) == MOUSE_EVENT {
            self.saw_console_mouse = true;
        }
        Some(rec)
    }

    fn key_down_char(rec: &RawRecord) -> Option<u16> {
        if u32::from(rec.EventType) != KEY_EVENT {
            return None;
        }
        let (down, _, ch, _) = sys::key_fields(rec);
        down.then_some(ch)
    }

    /// The char of the next pending key-down, polling ~`timeout_ms` (a report can arrive byte by
    /// byte). Peeks sixteen records and skips releases, so the ESC's own key-up hides nothing.
    fn peek_key_char(&self, timeout_ms: u64) -> Option<u16> {
        let start = Instant::now();
        loop {
            if let Some(c) = sys::peek_records(&self.handle, 16).iter().find_map(Self::key_down_char) {
                return Some(c);
            }
            if start.elapsed() >= Duration::from_millis(timeout_ms) {
                return None;
            }
            // Sleep, not a wait on the handle: the ESC's own key-up keeps it signalled, so a wait
            // would return at once and the loop would spin a core for the whole timeout.
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Takes the mouse off the console and asks the terminal for any-motion SGR reports. One-way,
    /// and never when real `MOUSE_EVENT` records arrived or the output is redirected.
    fn switch_to_text(&mut self) {
        if self.closed || self.text_mouse || self.vt_motion || !self.mouse || self.output_redirected || self.saw_console_mouse {
            return;
        }
        let mode = self.armed_mode & !ENABLE_MOUSE_INPUT;
        if !sys::set_console_mode(&self.handle, mode) {
            return;
        }
        self.text_mouse = true;
        write_out(TEXT_MOUSE_ON);
        sys::flush_input(&self.handle);
        if self.trace.path.is_some() {
            self.trace
                .add("MOUSE via TEXT - console mouse off, SGR any-motion requested".into());
        }
    }

    /// `Read-ClaudeInputEvent`: one event within `timeout_ms`, or `None`.
    pub fn read_event(&mut self, timeout_ms: u32) -> Option<Event> {
        if self.closed {
            return None;
        }
        let rec = self.read_raw(timeout_ms)?;
        let t = u32::from(rec.EventType);
        if t == KEY_EVENT {
            let (down, code, ch, cks) = sys::key_fields(&rec);
            if down && ch == 27 {
                let wait = esc_wait_ms(self.text_mouse || self.vt_motion);
                match after_esc(self, wait) {
                    AfterEsc::Sequence(VtOutcome::Swallow) => return None,
                    AfterEsc::Sequence(VtOutcome::Mouse(m)) => return Some(Event::Mouse(m)),
                    AfterEsc::Sequence(VtOutcome::Key(k)) => return Some(Event::Key(k)),
                    AfterEsc::Alone if self.trace.path.is_some() => {
                        let at = self.epoch.elapsed().as_millis();
                        self.trace
                            .add(format!("ESC alone at {at} ms: no key-down within {wait} ms, Escape"));
                    }
                    AfterEsc::Alone | AfterEsc::Followed => {}
                }
            }
            return key_from_record(down, code, ch, cks).map(Event::Key);
        }
        if t == MOUSE_EVENT {
            let (mx, my, buttons, flags) = sys::mouse_fields(&rec);
            return Some(Event::Mouse(mouse_from_record(mx, my, buttons, flags)));
        }
        if t == WINDOW_BUFFER_SIZE_EVENT {
            return Some(Event::Resize);
        }
        None
    }
}

impl EscSource for ConsoleInput {
    fn peek_down(&mut self, timeout_ms: u64) -> Option<u16> {
        self.peek_key_char(timeout_ms)
    }

    /// Reads past releases (the queue interleaves them), at most sixteen records.
    fn take_down(&mut self, timeout_ms: u32) -> Option<u16> {
        for _ in 0..16 {
            let rec = self.read_raw(timeout_ms)?;
            if let Some(c) = Self::key_down_char(&rec) {
                return Some(c);
            }
        }
        None
    }

    fn mouse_report(&mut self) {
        self.switch_to_text();
    }
}

impl Drop for ConsoleInput {
    fn drop(&mut self) {
        self.close();
    }
}

fn write_out(s: &str) {
    let mut o = std::io::stdout();
    if o.write_all(s.as_bytes()).and_then(|()| o.flush()).is_err() {
        // A console that refuses the mode string has no tracking to switch either.
    }
}

/// One greppable trace line per record.
fn format_record(rec: &RawRecord) -> String {
    let t = u32::from(rec.EventType);
    if t == KEY_EVENT {
        let (down, code, ch, cks) = sys::key_fields(rec);
        let glyph = match ch {
            33..=126 => char::from_u32(u32::from(ch)).map_or_else(|| ".".into(), String::from),
            27 => "ESC".into(),
            32 => "SP".into(),
            _ => ".".into(),
        };
        return format!("KEY down={} vk={code:<3} ch=U+{ch:04X} [{glyph}] cks=0x{cks:04X}", i32::from(down));
    }
    if t == MOUSE_EVENT {
        let (mx, my, buttons, flags) = sys::mouse_fields(rec);
        return format!("MOU x={mx:<3} y={my:<3} btn=0x{buttons:08X} fl=0x{flags:04X}");
    }
    if t == WINDOW_BUFFER_SIZE_EVENT {
        let (cols, rows) = sys::size_fields(rec);
        return format!("SIZE {cols}x{rows}");
    }
    format!("OTHER type={t}")
}

/// Keeps the decoded key type reachable for callers that only import this module.
pub type Key = KeyPress;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_clears_quickedit_and_processed_input_and_keeps_the_rest() {
        let original = 0x01F7;
        let armed = armed_mode(original, true);
        assert_eq!(armed & ENABLE_QUICK_EDIT_MODE, 0);
        assert_eq!(armed & ENABLE_PROCESSED_INPUT, 0);
        assert_ne!(armed & ENABLE_MOUSE_INPUT, 0);
        assert_eq!(armed & 0x0100, 0x0100, "unrelated bits survive");
    }

    #[test]
    fn arm_clears_line_and_echo_input_so_ctrl_s_is_a_key_and_close_restores_them() {
        let original = 0x01F7;
        for mouse in [true, false] {
            assert_eq!(armed_mode(original, mouse) & 0x6, 0, "conhost eats ctrl+s under line input");
        }
        assert_eq!(original & 0x6, 0x6, "close sets the untouched original back");
    }

    #[test]
    fn keyboard_only_arm_keeps_quickedit_and_asks_no_mouse() {
        let armed = armed_mode(0x0047, false);
        assert_ne!(armed & ENABLE_QUICK_EDIT_MODE, 0);
        assert_eq!(armed & ENABLE_MOUSE_INPUT, 0);
        assert_eq!(armed & ENABLE_PROCESSED_INPUT, 0);
    }

    #[test]
    fn motion_is_requested_only_inside_tmux_on_a_console_with_mouse() {
        assert!(wants_vt_motion(Some(r"\\.\pipe\psmux,123,0"), true, true));
        assert!(!wants_vt_motion(None, true, true), "a plain Windows Terminal tab never gets 1003");
        assert!(!wants_vt_motion(Some(""), true, true));
        assert!(
            !wants_vt_motion(Some("x"), false, true),
            "redirected stdout would carry the request into a file"
        );
        assert!(!wants_vt_motion(Some("x"), true, false), "CLAUDE_AUTO_NO_MOUSE=1 asks for no mouse");
    }

    #[test]
    fn arm_keeps_mouse_input_for_psmux_clicks() {
        assert_ne!(armed_mode(0, true) & ENABLE_MOUSE_INPUT, 0);
    }

    #[test]
    fn text_mouse_requests_motion_and_exit_resets_it() {
        assert!(TEXT_MOUSE_ON.contains("?1003h"), "any-motion, not press-only");
        assert!(!TEXT_MOUSE_ON.contains("?1003l"));
        for m in ["?1003l", "?1006l", "?1002l", "?1000l"] {
            assert!(TEXT_MOUSE_OFF.contains(m));
        }
    }
}
