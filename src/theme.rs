//! Palette, glyphs, span markers and the small pure painters (`Theme.ps1`). Colour is applied
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
pub const BUTTON_BG: &str = "\x1b[48;5;238m";
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

static ANSI_COLOR: LazyLock<Regex> = LazyLock::new(|| re(r"\x1b\[[0-9;]*m"));
static CTL: LazyLock<Regex> = LazyLock::new(|| re(r"[\x00-\x1F]"));
static DIM_PAIR: LazyLock<Regex> = LazyLock::new(|| re("\u{1}([^\u{1}\u{2}]*)\u{2}"));
static HOVER_PAIR: LazyLock<Regex> = LazyLock::new(|| re("\u{4}([^\u{4}\u{5}]*)\u{5}"));

/// Compiles a pattern that is a literal in this crate.
#[expect(clippy::expect_used, reason = "every caller passes a pattern literal checked by the tests")]
pub(crate) fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("pattern literals compile")
}

/// Strips every C0 control from a data field, so a field can never carry a marker.
pub fn strip_ctl(s: &str) -> String {
    CTL.replace_all(s, "").into_owned()
}

/// `Add-DimSpanColor`: markers to Dim..Reset with colour on, to nothing with it off. An unpaired
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

/// `Add-HoverSpan`: marks a finished piece of plain text as hovered.
pub fn add_hover_span(text: &str) -> String {
    format!("{HOVER_OPEN}{text}{HOVER_CLOSE}")
}

/// `Add-HoverSpanColor`: the band as a background that survives every inner Reset.
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

/// `Test-ColorSupported`: `NO_COLOR` and a dumb terminal opt out.
pub fn color_supported(no_color: Option<&str>, term: Option<&str>) -> bool {
    !(no_color.is_some_and(|v| !v.is_empty()) || term == Some("dumb"))
}

/// `Remove-AnsiColor`.
pub fn remove_ansi_color(text: &str) -> String {
    ANSI_COLOR.replace_all(text, "").into_owned()
}

/// `Get-PercentColor`.
pub fn percent_color(p: i64) -> &'static str {
    if p >= 85 {
        RED
    } else if p >= 60 {
        YELLOW
    } else {
        GREEN
    }
}

/// `Test-AsciiRequired`: `CLAUDE_AUTO_ASCII=1`, or a console not on UTF-8 (code page 65001).
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
}

/// `Get-Glyphs`.
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
        }
    }
}

/// Which bar `new_bar` draws: the shade blocks or the thin box-drawing line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarStyle {
    Blocks,
    Line,
}

/// `New-Bar`: plain text of exactly `width` characters. `[Math]::Round` is banker's rounding.
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
    fn bar_uses_bankers_rounding() {
        let g = glyphs(true);
        assert_eq!(new_bar(41, 8, &g, BarStyle::Blocks), "###.....");
        assert_eq!(new_bar(25, 2, &g, BarStyle::Blocks), "..", "0.5 rounds to even 0");
        assert_eq!(new_bar(75, 2, &g, BarStyle::Blocks), "##", "1.5 rounds to even 2");
        assert_eq!(new_bar(150, 3, &g, BarStyle::Line), "===");
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
