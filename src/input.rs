//! Input events and their pure decoding (`Input.ps1`): keys, console mouse records, VT mouse
//! reports (SGR and X10), VT cursor keys, and hotkeys on any keyboard layout.

pub mod console;

/// Virtual-key codes (the values of .NET `ConsoleKey`).
pub mod vk {
    pub const BACKSPACE: u16 = 8;
    pub const TAB: u16 = 9;
    pub const ENTER: u16 = 13;
    pub const SHIFT: u16 = 16;
    pub const CONTROL: u16 = 17;
    pub const MENU: u16 = 18;
    pub const ESCAPE: u16 = 27;
    pub const SPACE: u16 = 32;
    pub const PAGE_UP: u16 = 33;
    pub const PAGE_DOWN: u16 = 34;
    pub const END: u16 = 35;
    pub const HOME: u16 = 36;
    pub const LEFT: u16 = 37;
    pub const UP: u16 = 38;
    pub const RIGHT: u16 = 39;
    pub const DOWN: u16 = 40;
    pub const INSERT: u16 = 45;
    pub const DELETE: u16 = 46;
    pub const F12: u16 = 123;
    pub const C: u16 = 0x43;
    pub const R: u16 = 0x52;
    pub const OEM2: u16 = 191;
}

/// One key press, with the modifiers and Caps Lock state its record carried.
#[expect(clippy::struct_excessive_bools, reason = "the record's own flags")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyPress {
    pub ch: char,
    pub vk: u16,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    /// A STATE, not a modifier: with it on, `u` arrives as 'U' with no Shift.
    pub caps_lock: bool,
}

impl KeyPress {
    pub fn named(vk: u16) -> Self {
        Self { vk, ..Self::default() }
    }

    pub fn char(ch: char) -> Self {
        Self { ch, ..Self::default() }
    }

    pub fn is(&self, code: u16) -> bool {
        self.vk == code
    }

    pub fn is_ctrl(&self, code: u16) -> bool {
        self.vk == code && self.ctrl
    }
}

/// A mouse event in 0-based buffer coordinates, from a console record or a VT report alike.
#[expect(clippy::struct_excessive_bools, reason = "the record's own flags")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MouseEvent {
    pub x: i32,
    pub y: i32,
    pub buttons: u32,
    pub left: bool,
    pub right: bool,
    pub is_move: bool,
    pub is_double_click: bool,
    /// Only the SIGN is meaningful: Windows Terminal reports +-128, not +-120.
    pub wheel: i32,
}

impl MouseEvent {
    pub fn wheel_up(&self) -> bool {
        self.wheel > 0
    }

    pub fn wheel_down(&self) -> bool {
        self.wheel < 0
    }
}

/// What the screen loop consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Key(KeyPress),
    Mouse(MouseEvent),
    Resize,
}

const KEY_NAMES: &[(&str, u16)] = &[
    ("Backspace", vk::BACKSPACE),
    ("Tab", vk::TAB),
    ("Enter", vk::ENTER),
    ("Escape", vk::ESCAPE),
    ("Spacebar", vk::SPACE),
    ("PageUp", vk::PAGE_UP),
    ("PageDown", vk::PAGE_DOWN),
    ("End", vk::END),
    ("Home", vk::HOME),
    ("LeftArrow", vk::LEFT),
    ("UpArrow", vk::UP),
    ("RightArrow", vk::RIGHT),
    ("DownArrow", vk::DOWN),
    ("Insert", vk::INSERT),
    ("Delete", vk::DELETE),
    ("Oem1", 186),
    ("OemPlus", 187),
    ("OemComma", 188),
    ("OemMinus", 189),
    ("OemPeriod", 190),
    ("Oem2", vk::OEM2),
    ("Oem3", 192),
    ("Oem4", 219),
    ("Oem5", 220),
    ("Oem6", 221),
    ("Oem7", 222),
];

/// A .NET `ConsoleKey` name (case-insensitive, or a number) to its virtual-key code.
pub fn parse_key_name(name: &str) -> Option<u16> {
    if let Ok(n) = name.parse::<u16>() {
        return Some(n);
    }
    if let Some(&(_, code)) = KEY_NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Some(code);
    }
    let up = name.to_ascii_uppercase();
    let mut chars = up.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(c), None, None) if c.is_ascii_uppercase() => Some(u16::from(c as u8)),
        (Some('D'), Some(d), None) if d.is_ascii_digit() => Some(u16::from(d as u8)),
        _ => {
            if let Some(n) = up.strip_prefix("NUMPAD").and_then(|d| d.parse::<u16>().ok()).filter(|n| *n <= 9) {
                return Some(96 + n);
            }
            up.strip_prefix('F')
                .and_then(|d| d.parse::<u16>().ok())
                .filter(|n| (1..=24).contains(n))
                .map(|n| 111 + n)
        }
    }
}

/// The `ConsoleKey` name of a code, for log records.
pub fn key_name(code: u16) -> String {
    if let Some(&(n, _)) = KEY_NAMES.iter().find(|(_, c)| *c == code) {
        return n.to_owned();
    }
    match code {
        0x41..=0x5A => char::from_u32(u32::from(code)).map_or_else(|| code.to_string(), String::from),
        0x30..=0x39 => format!("D{}", code - 0x30),
        112..=135 => format!("F{}", code - 111),
        _ => code.to_string(),
    }
}

/// `New-ScriptedKeyReader`'s rule: one character is a typed character, anything longer a key name.
pub fn scripted_key(entry: &str) -> Option<KeyPress> {
    let mut chars = entry.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(KeyPress::char(c)),
        _ => parse_key_name(entry).map(KeyPress::named),
    }
}

/// `New-SyntheticKey`: what a footer click stands in for.
pub fn synthetic_key(key: &str, ch: Option<char>) -> KeyPress {
    KeyPress {
        ch: ch.unwrap_or('\0'),
        vk: if key.is_empty() { 0 } else { parse_key_name(key).unwrap_or(0) },
        ..KeyPress::default()
    }
}

pub const SHIFT_PRESSED: u32 = 0x0010;
pub const CAPSLOCK_ON: u32 = 0x0080;
const ALT_PRESSED: u32 = 0x0001 | 0x0002;
const CTRL_PRESSED: u32 = 0x0004 | 0x0008;

/// `ConvertTo-ClaudeInputEvent` for a `KEY_EVENT` record: key-ups and bare modifiers are nothing.
/// A char-only record (vk 0) of a control key gets that key's vk: psmux writes a key that shares
/// one input batch with SGR mouse text that way, and the screens match Enter, Tab, Esc by vk.
pub fn key_from_record(down: bool, code: u16, unicode: u16, control_state: u32) -> Option<KeyPress> {
    if !down || matches!(code, vk::SHIFT | vk::CONTROL | vk::MENU) {
        return None;
    }
    let code = match (code, unicode) {
        (0, 13) => vk::ENTER,
        (0, 9) => vk::TAB,
        (0, 8) => vk::BACKSPACE,
        (0, 27) => vk::ESCAPE,
        _ => code,
    };
    Some(KeyPress {
        ch: char::from_u32(u32::from(unicode)).unwrap_or('\0'),
        vk: code,
        shift: control_state & SHIFT_PRESSED != 0,
        alt: control_state & ALT_PRESSED != 0,
        ctrl: control_state & CTRL_PRESSED != 0,
        caps_lock: control_state & CAPSLOCK_ON != 0,
    })
}

const MOUSE_MOVED: u32 = 0x1;
const DOUBLE_CLICK: u32 = 0x2;
const MOUSE_WHEELED: u32 = 0x4;
const MOUSE_HWHEELED: u32 = 0x8;

/// `ConvertTo-ClaudeInputEvent` for a `MOUSE_EVENT` record. The wheel amount is a SIGNED 16-bit
/// value in the high word of the button state.
pub fn mouse_from_record(x: i32, y: i32, buttons: u32, flags: u32) -> MouseEvent {
    let wheel = if flags & (MOUSE_WHEELED | MOUSE_HWHEELED) != 0 {
        let high = u16::try_from(buttons >> 16).unwrap_or_default();
        i32::from(high.cast_signed())
    } else {
        0
    };
    MouseEvent {
        x,
        y,
        buttons,
        left: buttons & 0x1 != 0,
        right: buttons & 0x2 != 0,
        is_move: flags & MOUSE_MOVED != 0,
        is_double_click: flags & DOUBLE_CLICK != 0,
        wheel,
    }
}

/// `ConvertFrom-ClaudeMouseReport`: the protocol's button byte (low bits button, 32 motion, 64
/// wheel) and 1-based coordinates, to the same shape a console record decodes to.
pub fn mouse_from_report(button: u32, x: i32, y: i32, released: bool) -> MouseEvent {
    let wheel = if button & 64 != 0 {
        if button & 1 != 0 { -1 } else { 1 }
    } else {
        0
    };
    let low = button & 3;
    let down = !released && wheel == 0;
    MouseEvent {
        x: x - 1,
        y: y - 1,
        buttons: button,
        left: down && low == 0,
        right: down && low == 2,
        is_move: button & 32 != 0,
        is_double_click: false,
        wheel,
    }
}

/// What one escape sequence turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VtOutcome {
    Mouse(MouseEvent),
    Key(KeyPress),
    Swallow,
}

/// `ConvertFrom-ClaudeVtKey`: a cursor key spelled as VT text (`[A`, `OA`, `[1~`, `[4~`).
pub fn vt_key(seq: &str) -> Option<KeyPress> {
    let code = match seq {
        "[1~" => vk::HOME,
        "[4~" => vk::END,
        _ => {
            let mut c = seq.chars();
            let (Some('[' | 'O'), Some(f), None) = (c.next(), c.next(), c.next()) else {
                return None;
            };
            match f {
                'A' => vk::UP,
                'B' => vk::DOWN,
                'C' => vk::RIGHT,
                'D' => vk::LEFT,
                'H' => vk::HOME,
                'F' => vk::END,
                _ => return None,
            }
        }
    };
    Some(KeyPress::named(code))
}

/// A CSI sequence scanned to its final byte (introducer included): an SGR mouse report
/// `[<b;x;yM` / `[<b;x;ym`, a VT key, or something swallowed (focus, device report).
pub fn decode_csi(seq: &str) -> VtOutcome {
    if let Some(m) = parse_sgr(seq) {
        return VtOutcome::Mouse(m);
    }
    vt_key(seq).map_or(VtOutcome::Swallow, VtOutcome::Key)
}

fn parse_sgr(seq: &str) -> Option<MouseEvent> {
    let body = seq.strip_prefix("[<")?;
    let released = match body.chars().last()? {
        'M' => false,
        'm' => true,
        _ => return None,
    };
    let nums = body.get(..body.len() - 1)?;
    let mut parts = nums.split(';');
    let mut next = || {
        parts
            .next()
            .filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|p| p.parse::<i64>().ok())
    };
    let (b, x, y) = (next()?, next()?, next()?);
    if parts.next().is_some() {
        return None;
    }
    Some(mouse_from_report(
        u32::try_from(b).ok()?,
        i32::try_from(x).ok()?,
        i32::try_from(y).ok()?,
        released,
    ))
}

/// An X10 report `M b x y` whose three bytes each carry 32 + value.
pub fn decode_x10(bytes: &[u32]) -> Option<MouseEvent> {
    match bytes {
        [_, b, x, y] => Some(mouse_from_report(
            b.checked_sub(32)?,
            i32::try_from(x.checked_sub(32)?).ok()?,
            i32::try_from(y.checked_sub(32)?).ok()?,
            false,
        )),
        _ => None,
    }
}

/// The lowercase letter the same physical key produces on the Cyrillic (Russian and Ukrainian)
/// layouts, per Latin hotkey. Code points, so the source stays ASCII.
const HOTKEY_LAYOUT: &[(char, char)] = &[
    ('u', '\u{0433}'),
    ('r', '\u{043A}'),
    ('d', '\u{0432}'),
    ('m', '\u{044C}'),
    ('p', '\u{0437}'),
    ('i', '\u{0448}'),
    ('f', '\u{0430}'),
    ('/', '\u{002E}'),
    ('w', '\u{0446}'),
    ('a', '\u{0444}'),
    ('s', '\u{044B}'),
    ('c', '\u{0441}'),
    ('t', '\u{0435}'),
];

/// The Cyrillic-layout letter for a Latin hotkey, if it has one.
pub fn layout_char(hotkey: char) -> Option<char> {
    HOTKEY_LAYOUT.iter().find(|(k, _)| *k == hotkey).map(|(_, c)| *c)
}

/// `Test-ClaudeHotkey`: does this press mean the Latin hotkey on ANY layout? The character
/// itself, the virtual key, or the Cyrillic letter on that key. A modifier never is; an uppercase
/// character is not unless Caps Lock is on (a mouse report's byte carries no Caps Lock).
pub fn is_hotkey(key: &KeyPress, hotkey: char) -> bool {
    if key.ch == hotkey {
        return true;
    }
    if key.shift || key.ctrl || key.alt {
        return false;
    }
    if !key.caps_lock && key.ch.is_uppercase() {
        return false;
    }
    let code = if hotkey == '/' {
        Some(vk::OEM2)
    } else if hotkey.is_ascii_lowercase() || hotkey.is_ascii_digit() {
        Some(u16::from(hotkey.to_ascii_uppercase() as u8))
    } else {
        None
    };
    if code.is_some_and(|c| key.vk == c) {
        return true;
    }
    layout_char(hotkey).is_some_and(|c| key.ch == c)
}

#[cfg(test)]
mod tests;
