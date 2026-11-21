//! The cockpit frame every screen shares: rounded panels with the title in the top border, a
//! tinted interior, the focused panel's accent, panels side by side or stacked, and the footer
//! line under them. Pure; a panel's lines are laid out, then painted, then framed.

use crate::frame::{BodyPainter, Style, format_body_line};
use crate::layout::{display_width, limit_line};
use crate::theme::{self, Glyphs};

/// The left column's width at two columns (maintenance and the session picker narrower).
pub const LEFT_WIDTH: usize = 82;
pub const MAINTENANCE_LEFT_WIDTH: usize = 50;
pub const SESSIONS_LEFT_WIDTH: usize = 62;
/// Under this many columns the columns stack.
pub const TWO_COLUMNS: usize = 120;

/// The accent (#d77757 as its nearest 256 colour; the truecolor pass makes it exact).
pub const ACCENT: &str = theme::COCKPIT_ACCENT;
/// An unfocused panel's border (#3a3a3a) and the faint second line of an empty state.
pub const FAINT: &str = theme::COCKPIT_FAINT;
/// A panel's interior (#141414) and the focused panel's (#1b1b1b).
pub const TINT: &str = "\x1b[48;5;233m";
pub const TINT_FOCUSED: &str = "\x1b[48;5;234m";
/// Strong text (#f2f2f2): a selected row's label, a version.
pub const STRONG: &str = theme::COCKPIT_STRONG;

/// Whether a panel is the one w/s moves in: accent border and bold title, the lighter tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Focused,
    Plain,
}

/// One panel: `lines` are its interior, column 0 the first cell inside the left border, marked
/// but not coloured; `painter` colours each after truncation.
pub struct Panel<'a> {
    /// Accent; bold when focused.
    pub title: String,
    /// Strong and bold after ` ─ ` (the page word on a focused panel).
    pub page: String,
    /// Dim, right-aligned in the top border; left out when the border has no room for it.
    pub right: String,
    pub focus: Focus,
    pub lines: Vec<String>,
    /// Outer size, borders included.
    pub width: usize,
    pub height: usize,
    pub painter: BodyPainter<'a>,
    /// The line drawn as the selected row: the fill across the interior, bold.
    pub selected: Option<usize>,
}

impl std::fmt::Debug for Panel<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Panel")
            .field("title", &self.title)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl Panel<'_> {
    /// An unfocused panel with no painter, no right text and no selected row.
    pub fn plain(title: &str, lines: Vec<String>, width: usize, height: usize) -> Self {
        Panel {
            title: title.to_owned(),
            page: String::new(),
            right: String::new(),
            focus: Focus::Plain,
            lines,
            width,
            height,
            painter: &no_paint,
            selected: None,
        }
    }
}

/// The painter of a panel whose lines carry only dim spans and hover marks.
pub fn no_paint(line: &str, _enabled: bool) -> String {
    line.to_owned()
}

/// `✻ claude-auto`, the identity the focused panel's border carries.
pub fn identity(g: &Glyphs) -> String {
    format!("{} claude-auto", g.sparkle)
}

fn paint(text: &str, sgr: &str, color: bool) -> String {
    if color && !text.is_empty() {
        format!("{sgr}{text}{}", theme::RESET)
    } else {
        text.to_owned()
    }
}

fn rule(g: &Glyphs, n: usize) -> String {
    std::iter::repeat_n(g.h, n).collect()
}

/// The panel drawn: exactly `height` lines of exactly `width` cells. Lines past the interior are
/// cut; missing ones are blank.
pub fn render(p: &Panel<'_>, style: Style) -> Vec<String> {
    let g = theme::glyphs(style.ascii);
    if p.width < 4 || p.height < 2 {
        return Vec::new();
    }
    let inner = p.width - 2;
    let focused = p.focus == Focus::Focused;
    let border = if focused { ACCENT } else { FAINT };
    // ASCII mode is a plain terminal: the body keeps its colours, the frame and the tints go.
    let c = style.color && !style.ascii;
    let edge = |s: &str| paint(s, border, c);

    let mut title = p.title.clone();
    if !p.page.is_empty() {
        title = format!("{title} {} {}", g.h, p.page);
    }
    // One rule cell always stays before the corner, so a cut title still reads as a border.
    let title = limit_line(&title, inner.saturating_sub(4), style.ascii);
    let used = 1 + display_width(&title) + 2;
    let right = format!(" {} ", p.right);
    let fits_right = !p.right.is_empty() && used + display_width(&right) < inner;
    let fill = inner.saturating_sub(used + if fits_right { display_width(&right) + 1 } else { 0 });
    let title_sgr = if focused {
        format!("{ACCENT}{}", theme::BOLD)
    } else {
        ACCENT.to_owned()
    };
    let title_drawn = match title.split_once(&format!(" {} ", g.h)) {
        Some((head, page)) if !p.page.is_empty() && c => format!(
            "{}{}{}",
            paint(head, &title_sgr, c),
            paint(&format!(" {} ", g.h), ACCENT, c),
            paint(page, &format!("{STRONG}{}", theme::BOLD), c)
        ),
        _ => paint(&title, &title_sgr, c),
    };
    let mut top = edge(&format!("{}{} ", g.tl, g.h));
    top.push_str(&title_drawn);
    top.push_str(&edge(&format!(" {}", rule(&g, fill))));
    if fits_right {
        top.push_str(&paint(&right, theme::DIM, c));
        top.push_str(&edge(&g.h.to_string()));
    }
    top.push_str(&edge(&g.tr.to_string()));

    let mut out = vec![top];
    for i in 0..p.height - 2 {
        let raw = p.lines.get(i).map_or("", String::as_str);
        out.push(line(p, raw, p.selected == Some(i), style));
    }
    out.push(edge(&format!("{}{}{}", g.bl, rule(&g, inner), g.br)));
    out
}

/// One interior line of `p` drawn with its side borders: `raw` cut to the interior, painted,
/// banded when `selected`, tinted. A screen that repaints one line (a hover) calls it alone.
pub fn line(p: &Panel<'_>, raw: &str, selected: bool, style: Style) -> String {
    let g = theme::glyphs(style.ascii);
    let inner = p.width.saturating_sub(2);
    let focused = p.focus == Focus::Focused;
    let c = style.color && !style.ascii;
    let side = paint(&g.v.to_string(), if focused { ACCENT } else { FAINT }, c);
    let tint = if focused { TINT_FOCUSED } else { TINT };
    let raw = if selected && style.color {
        let pad = " ".repeat(inner.saturating_sub(display_width(raw)));
        format!("{}{raw}{pad}{}", theme::SELECT_OPEN, theme::SELECT_CLOSE)
    } else {
        raw.to_owned()
    };
    let body = theme::add_tone_color(&format_body_line(&raw, inner, style, p.painter), style.color);
    let body = theme::add_select_band_color(&body, style.color);
    let pad = " ".repeat(inner.saturating_sub(display_width(&theme::remove_ansi_color(&body))));
    let interior = if c {
        format!(
            "{tint}{}{pad}{}",
            body.replace(theme::RESET, &format!("{}{tint}", theme::RESET)),
            theme::RESET
        )
    } else {
        format!("{body}{pad}")
    };
    format!("{side}{interior}{side}")
}

/// Two columns of rendered panels side by side; the shorter is padded with blank cells.
pub fn beside(left: Vec<String>, left_width: usize, right: &[String]) -> Vec<String> {
    let n = left.len().max(right.len());
    let mut left = left;
    left.resize(n, " ".repeat(left_width));
    left.into_iter()
        .enumerate()
        .map(|(i, l)| format!("{l}{}", right.get(i).map_or("", String::as_str)))
        .collect()
}

/// `left` then `right` ending at `width` cells; `right` is left out without room for both.
pub fn spread(left: &str, right: &str, width: usize) -> String {
    let used = display_width(left) + display_width(right);
    if right.is_empty() || used + 1 > width {
        return left.to_owned();
    }
    format!("{left}{}{right}", " ".repeat(width - used))
}

/// A dim span.
pub fn dim(text: &str) -> String {
    if text.is_empty() {
        String::new()
    } else {
        format!("{}{text}{}", theme::DIM_OPEN, theme::DIM_CLOSE)
    }
}

#[cfg(test)]
mod tests;
