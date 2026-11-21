//! Palette, glyphs, span markers and the small pure painters. Colour is applied
//! AFTER a line is laid out and truncated, never during: escapes are invisible characters.

use std::sync::LazyLock;

use regex::Regex;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const REVERSE: &str = "\x1b[7m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const MAGENTA: &str = "\x1b[35m";
pub const CYAN: &str = "\x1b[36m";
pub const BRIGHT_CYAN: &str = "\x1b[96m";
pub const BRIGHT_YELLOW: &str = "\x1b[93m";
pub const BRIGHT_WHITE: &str = "\x1b[97m";
/// Claude Code's warm accent as a 256-colour index.
pub const ACCENT: &str = "\x1b[38;5;209m";
/// The hover fill (#1f1f1f; 256-colour 235 sits one step above the focused panel's tint).
pub const BUTTON_BG: &str = "\x1b[48;5;235m";
pub const BUTTON_FG: &str = "\x1b[38;5;250m";
pub const ACCENT_BG: &str = "\x1b[48;5;209m";
pub const ACCENT_FG: &str = "\x1b[38;5;232m";

/// A palette colour by its config name (an account tint), or `None`.
pub fn palette(name: &str) -> Option<&'static str> {
    Some(match name {
        "Red" => RED,
        "Green" => GREEN,
        "Yellow" => YELLOW,
        "Blue" => BLUE,
        "Magenta" => MAGENTA,
        "Cyan" => CYAN,
        "BrightCyan" => BRIGHT_CYAN,
        "BrightYellow" => BRIGHT_YELLOW,
        "BrightWhite" => BRIGHT_WHITE,
        "Accent" => ACCENT,
        "Bold" => BOLD,
        "Dim" => DIM,
        "Reset" => RESET,
        "Reverse" => REVERSE,
        _ => return None,
    })
}

/// Dim-span markers: C0 controls, zero cells wide, resolved after layout.
pub const DIM_OPEN: char = '\u{1}';
pub const DIM_CLOSE: char = '\u{2}';
/// Hover-band markers, resolved LAST so the band can repaint over every inner Reset.
pub const HOVER_OPEN: char = '\u{4}';
pub const HOVER_CLOSE: char = '\u{5}';

/// Tone-span markers (zero cells, resolved after layout by `add_tone_color`): an open marker per
/// tone, one shared close. Bold, the owner's text (cyan family), Claude's label (the accent), live.
pub const TONE_BOLD: char = '\u{f}';
pub const TONE_USER: char = '\u{e}';
pub const TONE_CLAUDE: char = '\u{10}';
pub const TONE_LIVE: char = '\u{11}';
pub const TONE_CLOSE: char = '\u{12}';
/// Selected-row band markers: a fill plus bold that survives every inner Reset, like the hover
/// band, and stronger than it (`SELECT_BG` is lighter than `BUTTON_BG` on a dark terminal).
pub const SELECT_OPEN: char = '\u{13}';
pub const SELECT_CLOSE: char = '\u{14}';
/// The selected row's fill (#262626; 256-colour 236 keeps it a step above the hover fill).
pub const SELECT_BG: &str = "\x1b[48;5;236m";
/// Inline code in a chat message: a muted tan (256-colour 180), set apart from dim text.
pub const TONE_CODE: char = '\u{15}';
const CODE_FG: &str = "\x1b[38;5;180m";

/// Cockpit tones: the faint second line of an empty state, the accent (#d77757), strong text,
/// a bold warning and a bold error (a limit at 70 % and at 90 %).
pub const TONE_FAINT: char = '\u{16}';
pub const TONE_ACCENT: char = '\u{18}';
pub const TONE_STRONG: char = '\u{19}';
pub const TONE_WARN: char = '\u{1a}';
pub const TONE_ERROR: char = '\u{1c}';
/// The cockpit's accent, faint and strong colours as their nearest 256 colours (`to_truecolor`
/// makes them exact).
pub const COCKPIT_ACCENT: &str = "\x1b[38;5;173m";
pub const COCKPIT_FAINT: &str = "\x1b[38;5;237m";
pub const COCKPIT_STRONG: &str = "\x1b[38;5;255m";
const STRONG_BOLD: &str = "\x1b[1m\x1b[38;5;255m";
const WARN_BOLD: &str = "\x1b[1m\x1b[93m";
const ERROR_BOLD: &str = "\x1b[1m\x1b[31m";

const TONES: [(char, &str); 10] = [
    (TONE_BOLD, BOLD),
    (TONE_USER, BRIGHT_CYAN),
    (TONE_CLAUDE, ACCENT),
    (TONE_LIVE, GREEN),
    (TONE_CODE, CODE_FG),
    (TONE_FAINT, COCKPIT_FAINT),
    (TONE_ACCENT, COCKPIT_ACCENT),
    (TONE_STRONG, STRONG_BOLD),
    (TONE_WARN, WARN_BOLD),
    (TONE_ERROR, ERROR_BOLD),
];

/// `text` between `tone` and the shared close.
pub fn tone(tone: char, text: &str) -> String {
    if text.is_empty() {
        String::new()
    } else {
        format!("{tone}{text}{TONE_CLOSE}")
    }
}

/// Tone markers to their colour..Reset with colour on, to nothing with it off. An open marker
/// whose close a truncation cut off is closed at the end of the line.
pub fn add_tone_color(line: &str, enabled: bool) -> String {
    if !line.contains(|c| TONES.iter().any(|(m, _)| *m == c)) && !line.contains(TONE_CLOSE) {
        return line.to_owned();
    }
    let mut out = String::with_capacity(line.len() + 16);
    let mut open = false;
    for c in line.chars() {
        if let Some((_, sgr)) = TONES.iter().find(|(m, _)| *m == c) {
            if enabled {
                out.push_str(sgr);
            }
            open = true;
        } else if c == TONE_CLOSE {
            if enabled && open {
                out.push_str(RESET);
            }
            open = false;
        } else {
            out.push(c);
        }
    }
    if enabled && open {
        out.push_str(RESET);
    }
    out
}

/// The selected-row band: fill and bold over the marked span, re-opened after every inner Reset.
pub fn add_select_band_color(line: &str, enabled: bool) -> String {
    let (Some(a), b) = (line.find(SELECT_OPEN), line.find(SELECT_CLOSE)) else {
        return line.replace(SELECT_CLOSE, "");
    };
    if !enabled {
        return line.replace([SELECT_OPEN, SELECT_CLOSE], "");
    }
    let end = b.filter(|b| *b > a).unwrap_or(line.len());
    let band = format!("{SELECT_BG}{BOLD}");
    let inner = line.get(a + SELECT_OPEN.len_utf8()..end).unwrap_or_default();
    let rest = line.get(end..).unwrap_or_default().trim_start_matches(SELECT_CLOSE);
    format!(
        "{}{band}{}{RESET}{rest}",
        line.get(..a).unwrap_or_default(),
        inner.replace(RESET, &format!("{RESET}{band}"))
    )
}

static ANSI_COLOR: LazyLock<Regex> = LazyLock::new(|| re(r"\x1b\[[0-9;]*m"));
static CTL: LazyLock<Regex> = LazyLock::new(|| re(r"[\x00-\x1F\x7F-\x9F]"));
static DIM_PAIR: LazyLock<Regex> = LazyLock::new(|| re("\u{1}([^\u{1}\u{2}]*)\u{2}"));
static HOVER_PAIR: LazyLock<Regex> = LazyLock::new(|| re("\u{4}([^\u{4}\u{5}]*)\u{5}"));

/// Compiles a pattern that is a literal in this crate.
#[expect(clippy::expect_used, reason = "every caller passes a pattern literal checked by the tests")]
pub(crate) fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("pattern literals compile")
}

/// Strips every control character (C0, DEL, C1) from a data field, so a field can never carry a
/// marker or move the cursor.
pub fn strip_ctl(s: &str) -> String {
    CTL.replace_all(s, "").into_owned()
}

/// Markers to Dim..Reset with colour on, to nothing with it off. An unpaired
/// open marker is closed at the end of the line.
pub fn add_dim_span_color(line: &str, enabled: bool) -> String {
    if !enabled {
        return line.replace([DIM_OPEN, DIM_CLOSE], "");
    }
    let mut out = DIM_PAIR.replace_all(line, format!("{DIM}${{1}}{RESET}").as_str()).into_owned();
    if out.contains(DIM_OPEN) {
        out = out.replace(DIM_OPEN, DIM) + RESET;
    }
    out.replace(DIM_CLOSE, RESET)
}

/// Marks a finished piece of plain text as hovered.
pub fn add_hover_span(text: &str) -> String {
    format!("{HOVER_OPEN}{text}{HOVER_CLOSE}")
}

/// The band as a background that survives every inner Reset.
pub fn add_hover_span_color(line: &str, enabled: bool) -> String {
    if !line.contains(HOVER_OPEN) && !line.contains(HOVER_CLOSE) {
        return line.to_owned();
    }
    if !enabled {
        return line.replace([HOVER_OPEN, HOVER_CLOSE], "");
    }
    let paint = |caps: &regex::Captures<'_>| {
        let inner = caps.get(1).map_or("", |m| m.as_str());
        format!("{BUTTON_BG}{}{RESET}", inner.replace(RESET, &format!("{RESET}{BUTTON_BG}")))
    };
    let mut out = HOVER_PAIR.replace_all(line, paint).into_owned();
    if out.contains(HOVER_OPEN) {
        out.push(HOVER_CLOSE);
        out = HOVER_PAIR.replace_all(&out, paint).into_owned();
    }
    out.replace(HOVER_CLOSE, "")
}

/// `NO_COLOR` and a dumb terminal opt out.
pub fn color_supported(no_color: Option<&str>, term: Option<&str>) -> bool {
    !(no_color.is_some_and(|v| !v.is_empty()) || term == Some("dumb"))
}

pub fn remove_ansi_color(text: &str) -> String {
    ANSI_COLOR.replace_all(text, "").into_owned()
}

pub fn percent_color(p: i64) -> &'static str {
    if p >= 85 {
        RED
    } else if p >= 60 {
        YELLOW
    } else {
        GREEN
    }
}

/// The 256-colour codes the cockpit uses and their exact truecolor forms.
const TRUECOLOR: [(&str, &str); 8] = [
    ("\x1b[48;5;233m", "\x1b[48;2;20;20;20m"),
    ("\x1b[48;5;234m", "\x1b[48;2;27;27;27m"),
    ("\x1b[48;5;235m", "\x1b[48;2;31;31;31m"),
    ("\x1b[48;5;236m", "\x1b[48;2;38;38;38m"),
    ("\x1b[38;5;173m", "\x1b[38;2;215;119;87m"),
    ("\x1b[38;5;237m", "\x1b[38;2;58;58;58m"),
    ("\x1b[38;5;243m", "\x1b[38;2;118;118;118m"),
    ("\x1b[38;5;255m", "\x1b[38;2;242;242;242m"),
];

/// A painted line with the cockpit's 256-colour codes swapped for their exact 24-bit colours.
pub fn to_truecolor(line: &str) -> String {
    if !line.contains("\x1b[") {
        return line.to_owned();
    }
    TRUECOLOR.iter().fold(line.to_owned(), |l, (from, to)| l.replace(from, to))
}

/// The terminal says it draws 24-bit colour: `COLORTERM` truecolor or 24bit, or Windows Terminal.
pub fn truecolor_supported(colorterm: Option<&str>, wt_session: Option<&str>) -> bool {
    colorterm.is_some_and(|v| v.eq_ignore_ascii_case("truecolor") || v.eq_ignore_ascii_case("24bit"))
        || wt_session.is_some_and(|v| !v.is_empty())
}

/// `CLAUDE_AUTO_ASCII=1`, or a console not on UTF-8 (code page 65001).
pub fn ascii_required(env_ascii: Option<&str>, code_page: u32) -> bool {
    env_ascii == Some("1") || code_page != 65001
}

/// Every glyph is exactly one cell wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyphs {
    pub cursor: char,
    pub on: char,
    pub off: char,
    pub bullet: char,
    pub sparkle: char,
    pub worktree: char,
    pub up: char,
    pub bar_full: char,
    pub bar_empty: char,
    pub bar_line_full: char,
    pub bar_line_empty: char,
    pub prompt: char,
    pub tl: char,
    pub tr: char,
    pub bl: char,
    pub br: char,
    pub h: char,
    pub v: char,
    pub l_angle: char,
    pub r_angle: char,
    /// Deliberately not `v`: a footer of V glyphs made one column look like a pane divider.
    pub hint_sep: char,
    /// The warning badge on a footer hint.
    pub warn: char,
    /// Between the facts of a title (`5h 4% · 7d 96%`).
    pub dot: char,
}

pub fn glyphs(ascii: bool) -> Glyphs {
    if ascii {
        Glyphs {
            cursor: '>',
            on: '*',
            off: '-',
            bullet: '+',
            sparkle: '*',
            worktree: '@',
            up: '^',
            bar_full: '#',
            bar_empty: '.',
            bar_line_full: '=',
            bar_line_empty: '-',
            prompt: '>',
            tl: '+',
            tr: '+',
            bl: '+',
            br: '+',
            h: '-',
            v: '|',
            l_angle: '<',
            r_angle: '>',
            hint_sep: ':',
            warn: '!',
            dot: '-',
        }
    } else {
        Glyphs {
            cursor: '\u{276F}',
            on: '\u{25CF}',
            off: '\u{25CB}',
            bullet: '+',
            sparkle: '\u{273B}',
            worktree: '\u{2302}',
            up: '\u{2191}',
            bar_full: '\u{2593}',
            bar_empty: '\u{2591}',
            bar_line_full: '\u{2501}',
            bar_line_empty: '\u{2500}',
            prompt: '\u{276F}',
            tl: '\u{256D}',
            tr: '\u{256E}',
            bl: '\u{2570}',
            br: '\u{256F}',
            h: '\u{2500}',
            v: '\u{2502}',
            l_angle: '\u{2039}',
            r_angle: '\u{203A}',
            hint_sep: '\u{250A}',
            warn: '\u{26A0}',
            dot: '\u{B7}',
        }
    }
}

/// Which bar `new_bar` draws: the shade blocks or the thin box-drawing line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarStyle {
    Blocks,
    Line,
}

/// Plain text of exactly `width` characters. `[Math]::Round` is banker's rounding.
pub fn new_bar(percent: i64, width: usize, g: &Glyphs, style: BarStyle) -> String {
    if width == 0 {
        return String::new();
    }
    let p = percent.clamp(0, 100);
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "width and percent are small"
    )]
    let filled = ((width as f64 * p as f64 / 100.0).round_ties_even() as usize).min(width);
    let (full, empty) = match style {
        BarStyle::Line => (g.bar_line_full, g.bar_line_empty),
        BarStyle::Blocks => (g.bar_full, g.bar_empty),
    };
    std::iter::repeat_n(full, filled)
        .chain(std::iter::repeat_n(empty, width - filled))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ctl_drops_every_control_c0_del_and_c1() {
        // U+009B is a one-character CSI on terminals that take 8-bit controls.
        assert_eq!(strip_ctl("a\u{0}b\u{1b}[2Jc\u{7f}d\u{9b}2Je\u{85}f"), "ab[2Jcd2Jef");
        assert_eq!(strip_ctl("plain   text ünï"), "plain   text ünï", "spaces and letters stay");
    }

    #[test]
    fn dim_markers_resolve_or_vanish() {
        let l = format!("a{DIM_OPEN}b{DIM_CLOSE}c");
        assert_eq!(add_dim_span_color(&l, false), "abc");
        assert_eq!(add_dim_span_color(&l, true), format!("a{DIM}b{RESET}c"));
        assert_eq!(add_dim_span_color(&format!("a{DIM_OPEN}b"), true), format!("a{DIM}b{RESET}"));
    }

    #[test]
    fn hover_band_repaints_over_inner_resets() {
        let l = format!("x{HOVER_OPEN}a{RESET}b{HOVER_CLOSE}y");
        assert_eq!(add_hover_span_color(&l, true), format!("x{BUTTON_BG}a{RESET}{BUTTON_BG}b{RESET}y"));
        assert_eq!(
            add_hover_span_color(&format!("{HOVER_OPEN}ab"), true),
            format!("{BUTTON_BG}ab{RESET}")
        );
        assert_eq!(add_hover_span_color(&l, false), format!("xa{RESET}by"));
    }

    #[test]
    fn tone_markers_resolve_close_a_cut_span_or_vanish() {
        let l = format!("{TONE_USER}hi{TONE_CLOSE} {TONE_CLAUDE}cl");
        assert_eq!(add_tone_color(&l, true), format!("{BRIGHT_CYAN}hi{RESET} {ACCENT}cl{RESET}"));
        assert_eq!(add_tone_color(&l, false), "hi cl");
    }

    #[test]
    fn the_select_band_survives_inner_resets() {
        let l = format!("|{SELECT_OPEN}a{DIM}b{RESET}c{SELECT_CLOSE}|");
        let band = format!("{SELECT_BG}{BOLD}");
        assert_eq!(add_select_band_color(&l, true), format!("|{band}a{DIM}b{RESET}{band}c{RESET}|"));
        assert_eq!(add_select_band_color(&l, false), format!("|a{DIM}b{RESET}c|"));
    }

    #[test]
    fn bar_uses_bankers_rounding() {
        let g = glyphs(true);
        assert_eq!(new_bar(41, 8, &g, BarStyle::Blocks), "###.....");
        assert_eq!(new_bar(25, 2, &g, BarStyle::Blocks), "..", "0.5 rounds to even 0");
        assert_eq!(new_bar(75, 2, &g, BarStyle::Blocks), "##", "1.5 rounds to even 2");
        assert_eq!(new_bar(150, 3, &g, BarStyle::Line), "===");
    }

    #[test]
    fn truecolor_swaps_only_the_cockpit_codes() {
        let l = format!("\x1b[48;5;234ma{RESET}\x1b[38;5;173mb\x1b[38;5;209mc");
        assert_eq!(
            to_truecolor(&l),
            format!("\x1b[48;2;27;27;27ma{RESET}\x1b[38;2;215;119;87mb\x1b[38;5;209mc")
        );
        assert!(truecolor_supported(Some("truecolor"), None));
        assert!(truecolor_supported(None, Some("{guid}")));
        assert!(!truecolor_supported(Some("256"), Some("")));
    }

    #[test]
    fn color_and_ascii_detection() {
        assert!(!color_supported(Some("1"), None));
        assert!(!color_supported(None, Some("dumb")));
        assert!(color_supported(Some(""), Some("xterm")));
        assert!(ascii_required(Some("1"), 65001));
        assert!(ascii_required(None, 437));
        assert!(!ascii_required(None, 65001));
    }
}
