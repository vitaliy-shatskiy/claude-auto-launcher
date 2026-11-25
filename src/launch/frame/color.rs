//! The launch page's colour pass over laid-out lines: row labels, the selected cell, percentages,
//! ages. Exports `add_launch_color`.

use std::sync::LazyLock;

use regex::Regex;

use super::super::Roster;
use crate::cockpit;
use crate::theme::{self, Glyphs};

pub(super) static SGR_ROW_LABEL: LazyLock<Regex> = LazyLock::new(|| theme::re(r"^.{3}account[\*\s]"));
pub(super) static COMPACT_PREFIX: LazyLock<Regex> = LazyLock::new(|| theme::re("^.{3}[a-z]+\\*? +(?:[^\\s\\[\\]]+ )*\u{4}?$"));
pub(super) static BRACKET: LazyLock<Regex> = LazyLock::new(|| theme::re(r"\[([^\[\]]+)\]"));
pub(super) static PERCENT: LazyLock<Regex> = LazyLock::new(|| theme::re(r"(\d+)%"));
pub(super) static ROW_WORDS: LazyLock<Regex> =
    LazyLock::new(|| theme::re(r"\b(account|action|model|effort|advisor|permission|remote|mode)\b"));
pub(super) static AGE: LazyLock<Regex> = LazyLock::new(|| theme::re(r"(\d+ (?:min|h|d) ago|just now)"));

/// The selected cell's bracket `[...]` with at most one nested pair, starting at `open`
/// (a char index of `[`); returns the exclusive end char index.
pub(super) fn selected_bracket_end(chars: &[char], open: usize) -> Option<usize> {
    let mut i = open + 1;
    loop {
        match chars.get(i)? {
            ']' => return Some(i + 1),
            '[' => {
                let mut j = i + 1;
                loop {
                    match chars.get(j)? {
                        ']' => break,
                        '[' => return None,
                        _ => j += 1,
                    }
                }
                i = j + 1;
            }
            _ => i += 1,
        }
    }
}

pub(super) fn is_selected_anchor(chars: &[char], open: usize, on: char) -> bool {
    if open >= 2 && chars.get(open - 2) == Some(&on) && chars.get(open - 1) == Some(&' ') {
        return true;
    }
    let prefix: String = chars.get(..open).map_or_else(String::new, |c| c.iter().collect());
    let not_account = prefix.chars().skip(3).collect::<String>();
    COMPACT_PREFIX.is_match(&prefix) && !not_account.starts_with("account")
}

/// Painted AFTER layout; the selected cell first (anchored on the On glyph or
/// the compact row's label), then the account tab, percentages, row words, cursor, sparkle, ages.
pub(super) fn add_launch_color(line: &str, enabled: bool, g: &Glyphs, roster: &Roster) -> String {
    if !enabled || line.is_empty() {
        return line.to_owned();
    }
    // The focused account row bands only its label: each account keeps its own colour.
    let label_end = |l: &str| {
        SGR_ROW_LABEL
            .find(l)
            .map(|m| m.end() + l.get(m.end()..).map_or(0, |v| v.len() - v.trim_start().len()))
    };
    let banded = line.strip_prefix(theme::SELECT_OPEN).and_then(|rest| {
        let end = label_end(rest)?;
        let (label, values) = (rest.get(..end)?, rest.get(end..)?);
        Some(format!(
            "{}{label}{}{}",
            theme::SELECT_OPEN,
            theme::SELECT_CLOSE,
            values.replace(theme::SELECT_CLOSE, "")
        ))
    });
    let is_account_row = banded.is_some() || SGR_ROW_LABEL.is_match(line);
    let line = banded.as_deref().unwrap_or(line);
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars.get(i) == Some(&'[')
            && is_selected_anchor(&chars, i, g.on)
            && let Some(end) = selected_bracket_end(&chars, i)
        {
            let whole: String = chars.get(i..end).map_or_else(String::new, |c| c.iter().collect());
            let value: String = chars.get(i + 1..end - 1).map_or_else(String::new, |c| c.iter().collect());
            let tint = if ["off", "stop server", "safe"].iter().any(|v| v.eq_ignore_ascii_case(&value)) {
                theme::YELLOW
            } else {
                roster.tint(&value)
            };
            out.push_str(theme::BOLD);
            out.push_str(tint);
            out.push_str(&whole);
            out.push_str(theme::RESET);
            i = end;
            continue;
        }
        if let Some(c) = chars.get(i) {
            out.push(*c);
        }
        i += 1;
    }
    if is_account_row {
        out = BRACKET
            .replace_all(&out, |c: &regex::Captures<'_>| {
                let whole = c.get(0).map_or("", |m| m.as_str());
                let value = c.get(1).map_or("", |m| m.as_str()).split(' ').next().unwrap_or_default();
                format!("{}{}{whole}{}", theme::BOLD, roster.tint(value), theme::RESET)
            })
            .into_owned();
    }
    out = PERCENT
        .replace_all(&out, |c: &regex::Captures<'_>| {
            let p = c.get(1).map_or(0, |m| m.as_str().parse::<i64>().unwrap_or(i64::MAX));
            format!("{}{}{}", theme::percent_color(p), c.get(0).map_or("", |m| m.as_str()), theme::RESET)
        })
        .into_owned();
    out = ROW_WORDS
        .replace_all(&out, format!("{}${{1}}{}", theme::DIM, theme::RESET).as_str())
        .into_owned();
    out = out.replace(g.cursor, &format!("{}{}{}{}", theme::BOLD, cockpit::ACCENT, g.cursor, theme::RESET));
    out = out.replace(g.sparkle, &format!("{}{}{}", theme::ACCENT, g.sparkle, theme::RESET));
    AGE.replace_all(&out, format!("{}${{1}}{}", theme::DIM, theme::RESET).as_str())
        .into_owned()
}
