//! The footer as data: hints, their click spans, the lines they wrap to, the hover and selected
//! light, and the colour pass over a footer line.

use crate::layout::display_width;
use crate::theme::{self, Glyphs};

/// A clickable or plain hint of the footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub token: String,
    pub label: String,
    pub clickable: bool,
    /// The `ConsoleKey` name a click stands in for ('' when `ch` carries it).
    pub key: String,
    pub ch: Option<char>,
    /// A plain hint that stays when a narrow footer drops the plain ones.
    pub keep: bool,
    /// Drawn after the label in yellow (`⚠ 2`): it is part of the click span.
    pub badge: String,
}

impl Hint {
    pub fn plain(token: &str, label: &str) -> Self {
        Self {
            token: token.into(),
            label: label.into(),
            clickable: false,
            key: String::new(),
            ch: None,
            keep: false,
            badge: String::new(),
        }
    }

    /// Kept on a narrow footer, beside the buttons.
    #[must_use]
    pub fn kept(self) -> Self {
        Self { keep: true, ..self }
    }

    /// With a yellow badge after the label; an empty badge draws nothing.
    #[must_use]
    pub fn badged(self, badge: String) -> Self {
        Self { badge, ..self }
    }

    pub fn key(token: &str, label: &str, key: &str) -> Self {
        Self {
            token: token.into(),
            label: label.into(),
            clickable: true,
            key: key.into(),
            ch: None,
            keep: false,
            badge: String::new(),
        }
    }

    pub fn ch(token: &str, label: &str, ch: char) -> Self {
        Self {
            token: token.into(),
            label: label.into(),
            clickable: true,
            key: String::new(),
            ch: Some(ch),
            keep: false,
            badge: String::new(),
        }
    }
}

/// Columns of one hint on its footer line. `start`/`end` cover token and label (-1 when the hint
/// is not clickable); `key_start`/`key_end` the token alone. Character columns of the plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterSpan {
    pub start: i32,
    pub end: i32,
    pub key_start: usize,
    pub key_end: usize,
    pub key: String,
    pub ch: Option<char>,
    pub line: usize,
    /// Characters at the end of the span drawn as the yellow badge.
    pub badge: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterLine {
    pub text: String,
    pub spans: Vec<FooterSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footer {
    pub lines: Vec<FooterLine>,
}

/// One list drives what is printed, where it can be clicked and which key a
/// click means. All hints on one line when they fit `width` (0 = never drop); else the plain
/// (not clickable) hints drop, except the kept ones, then the buttons by priority until the rest
/// fits. The footer is always one row: a hint is never split or wrapped.
pub fn new_hint_footer(hints: &[Hint], g: &Glyphs, width: usize, plain: bool) -> Footer {
    let sep = format!("  {}  ", g.hint_sep);
    let token_of = |h: &Hint| {
        if !h.clickable {
            h.token.clone()
        } else if plain {
            format!("[{}]", h.token)
        } else {
            format!(" {} ", h.token)
        }
    };
    let piece_of = |h: &Hint| {
        let mut piece = token_of(h);
        for part in [&h.label, &h.badge] {
            if !part.is_empty() {
                piece.push(' ');
                piece.push_str(part);
            }
        }
        piece
    };
    let one_line =
        |hs: &[&Hint]| 2 + hs.iter().map(|h| display_width(&piece_of(h))).sum::<usize>() + display_width(&sep) * hs.len().saturating_sub(1);
    let mut shown: Vec<&Hint> = hints.iter().collect();
    if width > 0 && one_line(&shown) > width && shown.iter().any(|h| h.clickable || h.keep) {
        shown.retain(|h| h.clickable || h.keep);
    }
    // Then by priority, the last listed first within a priority.
    for tier in 0..4 {
        while width > 0 && shown.len() > 1 && one_line(&shown) > width {
            let Some(i) = shown.iter().rposition(|h| priority(h) == tier) else {
                break;
            };
            shown.remove(i);
        }
    }
    let mut lines = Vec::new();
    {
        let r = 0;
        let mut text = String::from("  ");
        let mut spans = Vec::new();
        for (c, h) in shown.iter().enumerate() {
            if c > 0 {
                text.push_str(&sep);
            }
            let token = token_of(h);
            let start = text.chars().count();
            let token_end = start + token.chars().count() - 1;
            text.push_str(&piece_of(h));
            let end = text.chars().count() - 1;
            let col = |v: usize| i32::try_from(v).unwrap_or(i32::MAX);
            spans.push(FooterSpan {
                start: if h.clickable { col(start) } else { -1 },
                end: if h.clickable { col(end) } else { -1 },
                key_start: start,
                key_end: token_end,
                key: if h.clickable { h.key.clone() } else { String::new() },
                ch: if h.clickable { h.ch } else { None },
                line: r,
                badge: h.badge.chars().count(),
            });
        }
        lines.push(FooterLine { text, spans });
    }
    Footer { lines }
}

/// The order a narrow footer drops hints in: a button first (0), then a kept plain hint (the
/// move and switch keys, 1), tab, the filter and enter (2), esc last (3): the way back outlasts tab.
fn priority(h: &Hint) -> u8 {
    if !h.clickable {
        1
    } else if h.key == "Escape" {
        3
    } else if matches!(h.key.as_str(), "Enter" | "Tab") || h.ch == Some('/') {
        2
    } else {
        0
    }
}

/// Which footer button is lit: by (key, char), so a wrapped footer stays correct.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FooterLight {
    pub hover: Option<(String, Option<char>)>,
    pub selected: Vec<(String, Option<char>)>,
}

/// The (key, char) a hovered clickable-span INDEX names.
pub fn footer_hover(footer: &Footer, hover: i32) -> Option<(String, Option<char>)> {
    let i = usize::try_from(hover).ok()?;
    footer
        .lines
        .iter()
        .flat_map(|l| &l.spans)
        .filter(|s| s.start >= 0)
        .nth(i)
        .map(|s| (s.key.clone(), s.ch))
}

fn char_slice(chars: &[char], from: usize, to_inclusive: usize) -> String {
    chars.get(from..=to_inclusive).map_or_else(String::new, |s| s.iter().collect())
}

/// Paints key tokens by COLUMN, so stripping the colour returns the line exactly.
pub fn add_hint_color(line: &str, spans: &[FooterSpan], enabled: bool, light: &FooterLight) -> String {
    if !enabled || line.is_empty() || spans.is_empty() {
        return line.to_owned();
    }
    let chars: Vec<char> = line.chars().collect();
    let mut sorted: Vec<&FooterSpan> = spans.iter().collect();
    sorted.sort_by_key(|s| s.key_start);
    let mut out = String::new();
    let mut cursor = 0usize;
    for s in sorted {
        if s.key_start < cursor || s.key_end >= chars.len() {
            continue;
        }
        out.push_str(theme::DIM);
        out.push_str(
            &chars
                .get(cursor..s.key_start)
                .map_or_else(String::new, |c| c.iter().collect::<String>()),
        );
        out.push_str(theme::RESET);
        let id = (s.key.clone(), s.ch);
        let hovered = s.start >= 0 && light.hover.as_ref() == Some(&id);
        let selected = s.start >= 0 && light.selected.contains(&id);
        let cap = if hovered || selected {
            format!("{}{}", theme::ACCENT_BG, theme::ACCENT_FG)
        } else if s.start >= 0 {
            format!("{}{}", theme::BUTTON_BG, theme::BUTTON_FG)
        } else {
            theme::DIM.to_owned()
        };
        let label_tint = if hovered || selected { theme::BOLD } else { theme::DIM };
        out.push_str(&cap);
        out.push_str(&char_slice(&chars, s.key_start, s.key_end));
        out.push_str(theme::RESET);
        cursor = s.key_end + 1;
        if let Ok(end) = usize::try_from(s.end)
            && end >= cursor
            && cursor < chars.len()
        {
            let label_end = end.min(chars.len() - 1);
            let badge_at = (label_end + 1).saturating_sub(s.badge).max(cursor);
            out.push_str(label_tint);
            out.push_str(
                &chars
                    .get(cursor..badge_at)
                    .map_or_else(String::new, |c| c.iter().collect::<String>()),
            );
            out.push_str(theme::RESET);
            if badge_at <= label_end {
                out.push_str(theme::YELLOW);
                out.push_str(&char_slice(&chars, badge_at, label_end));
                out.push_str(theme::RESET);
            }
            cursor = label_end + 1;
        }
    }
    if cursor < chars.len() {
        out.push_str(theme::DIM);
        out.push_str(&chars.get(cursor..).map_or_else(String::new, |c| c.iter().collect::<String>()));
        out.push_str(theme::RESET);
    }
    out
}
