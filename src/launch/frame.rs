//! The launch screen's frame (`Get-LaunchFrame`, `Add-LaunchColor`, limit lines). Pure.

use std::sync::LazyLock;

use regex::Regex;

use super::{LaunchState, OptionText, Roster, RowName, is_hidden_row_value, visible_row_values};
use crate::frame::{
    self, CellSpan, Footer, FooterLight, Frame, Hint, MIN_HEIGHT, MIN_WIDTH, RadioSpec, RowHit, RowMap, Style, TWO_PANE_WIDTH,
    measure_cell_spans, new_hint_footer, new_radio_row,
};
use crate::layout::{display_width, new_box};
use crate::theme::{self, BarStyle, Glyphs, new_bar};
use crate::types::{DefaultLabels, LimitRecord, Limits, VersionInfo};

/// Everything `Get-LaunchFrame` draws from.
#[derive(Debug, Clone, Copy)]
pub struct LaunchFrameInput<'a> {
    pub state: &'a LaunchState,
    pub width: usize,
    pub height: usize,
    pub limits: &'a Limits,
    pub version: &'a VersionInfo,
    /// Explicit restore marks and age; `None` reads them off the state (per tab).
    pub restored: Option<(&'a [String], &'a str)>,
    pub default_model_label: &'a str,
    pub default_advisor_label: &'a str,
    pub default_labels: &'a DefaultLabels,
    pub style: Style,
}

/// One limit as label, percent and an age of its own, in drawing order: five hour, seven day,
/// model bucket (aged when it came from a stale widget export).
fn limit_items(l: &LimitRecord) -> Vec<(String, i64, String)> {
    let mut items = Vec::new();
    if let Some(p) = l.five_hour {
        items.push(("5h".to_owned(), p, String::new()));
    }
    if let Some(p) = l.seven_day {
        items.push(("7d".to_owned(), p, String::new()));
    }
    if let Some(p) = l.model {
        let label = l
            .model_label
            .as_deref()
            .filter(|s| !s.is_empty())
            .map_or_else(|| "model".to_owned(), str::to_lowercase);
        let age = l.model_age.as_deref().map_or_else(String::new, |a| format!(" ({a})"));
        items.push((label, p, age));
    }
    items
}

const WIDE_BAR: usize = 8;
/// The narrow tier's bar widths, widest first.
const NARROW_BARS: [usize; 3] = [8, 6, 4];
/// The row labels' column: the narrow usage row moves here before it gives up its bars.
const LABEL_COLUMN: usize = 3;
const LIMIT_SEP: &str = "   ";

/// The usage row under the account tabs: one fixed-width cell per limit (label padded to the
/// longest, a bar, the percent right-aligned), the age last. It starts on the value column of the
/// rows below. The wide tier stacks the cells one per line when they do not fit. The narrow tier
/// tries narrower bars, then the label column, then drops the bars, then keeps the leading
/// pieces that fit.
pub fn limit_lines(l: &LimitRecord, indent: usize, inner: usize, wide: bool, g: &Glyphs) -> Vec<String> {
    let items = limit_items(l);
    if items.is_empty() {
        return Vec::new();
    }
    let lw = items.iter().map(|(label, _, _)| label.chars().count()).max().unwrap_or(0);
    let cells = |bar: usize| -> Vec<String> {
        items
            .iter()
            .map(|(label, p, age)| {
                if bar == 0 {
                    format!("{label:<lw$} {p:>3}%{age}")
                } else {
                    format!("{label:<lw$} {} {p:>3}%{age}", new_bar(*p, bar, g, BarStyle::Blocks))
                }
            })
            .collect()
    };
    let age = (!l.age_text.is_empty()).then_some(l.age_text.as_str());
    let tries: Vec<(usize, usize)> = if wide {
        vec![(indent, WIDE_BAR)]
    } else {
        [indent, LABEL_COLUMN]
            .iter()
            .flat_map(|i| NARROW_BARS.iter().map(move |b| (*i, *b)))
            .chain([(LABEL_COLUMN, 0)])
            .collect()
    };
    for (at, bar) in tries {
        let mut pieces = cells(bar);
        pieces.extend(age.map(str::to_owned));
        let one = format!("{}{}", " ".repeat(at), pieces.join(LIMIT_SEP));
        if display_width(&one) <= inner {
            return vec![one];
        }
    }
    if wide {
        let stacked = cells(WIDE_BAR);
        let last = stacked.len() - 1;
        let fits = stacked
            .iter()
            .all(|c| indent + display_width(c) + LIMIT_SEP.len() + age.map_or(0, display_width) <= inner);
        let lead = " ".repeat(if fits { indent } else { LABEL_COLUMN });
        return stacked
            .into_iter()
            .enumerate()
            .map(|(i, c)| match age.filter(|_| i == last) {
                Some(a) => format!("{lead}{c}{LIMIT_SEP}{a}"),
                None => format!("{lead}{c}"),
            })
            .collect();
    }
    let mut line = " ".repeat(LABEL_COLUMN);
    for (i, p) in cells(0).into_iter().chain(age.map(str::to_owned)).enumerate() {
        let next = if i == 0 {
            format!("{line}{p}")
        } else {
            format!("{line}{LIMIT_SEP}{p}")
        };
        if display_width(&next) > inner {
            break;
        }
        line = next;
    }
    (!line.trim().is_empty()).then_some(line).into_iter().collect()
}

fn pad_right(s: &str, width: usize) -> String {
    format!("{s:<width$}")
}

/// `Get-LaunchFrame`.
#[expect(clippy::too_many_lines, reason = "one row loop, as in Screens.ps1")]
pub fn launch_frame(roster: &Roster, inp: &LaunchFrameInput<'_>) -> Frame {
    let state = inp.state;
    let (restored, restored_age): (Vec<String>, String) = inp.restored.map_or_else(
        || (state.restored.clone(), state.restored_age.clone()),
        |(r, a)| (r.to_vec(), a.to_owned()),
    );
    if inp.width < MIN_WIDTH || inp.height < MIN_HEIGHT {
        return Frame {
            lines: frame::too_small_frame(inp.width, inp.height, inp.style.ascii),
            map: RowMap::default(),
        };
    }
    let g = theme::glyphs(inp.style.ascii);
    let fw = frame::frame_width(inp.width);
    let box_width = fw.min(100);
    let inner = box_width - 4;
    let wide = inp.width >= TWO_PANE_WIDTH;
    let label_width = if wide {
        12
    } else {
        roster.rows.iter().map(|r| r.label.len()).max().unwrap_or(0) + 3
    };

    let left = format!(" {} claude-auto", g.sparkle);
    let mut right = String::new();
    if let Some(inst) = inp.version.installed.as_deref().filter(|s| !s.is_empty()) {
        right = format!("v{inst}");
        if let Some(newest) = inp.version.newest.as_deref().filter(|n| !n.is_empty() && *n != inst) {
            right = format!("{right}  {} {newest}", g.up);
        }
    }
    let pad = inner.saturating_sub(display_width(&left) + display_width(&right)).max(1);
    let header = format!("{left}{}{right}", " ".repeat(pad));

    let limit = inp.limits.get(&state.account);
    let available = limit.and_then(|l| l.available_models.as_deref());
    let ctx = OptionText {
        default_model_label: inp.default_model_label,
        default_advisor_label: inp.default_advisor_label,
        available,
        default_labels: inp.default_labels,
    };
    let hover_row = state.hover_row;
    let mark_of = |row: &super::RowDef| {
        if restored.iter().any(|r| r.eq_ignore_ascii_case(row.name.as_str())) {
            "*"
        } else {
            " "
        }
    };
    let shown_of = |row: &super::RowDef| {
        let current = state.get(row.name);
        if is_hidden_row_value(roster, row, current, &ctx) {
            "default"
        } else {
            current
        }
    };
    let radio_of = |row: &super::RowDef, prefix: &str, hover: &str| {
        let texts: Vec<String> = row.values.iter().map(|v| ctx.text(roster, row, v)).collect();
        let values = visible_row_values(roster, row, &ctx);
        let vtexts: Vec<String> = values
            .iter()
            .map(|v| {
                row.values
                    .iter()
                    .position(|x| x == v)
                    .and_then(|p| texts.get(p).cloned())
                    .unwrap_or_else(|| ctx.text(roster, row, v))
            })
            .collect();
        new_radio_row(
            &RadioSpec {
                prefix,
                label: &format!("{}{}", row.label, mark_of(row)),
                values: &values,
                current: shown_of(row),
                texts: &vtexts,
                label_width,
                max_width: if wide { inner } else { 0 },
                hover,
                compact: false,
            },
            &g,
        )
    };
    // The narrow tier draws every value row one way: all as radio lists when each one fits,
    // else all collapsed to `< value >`.
    let collapse_all = !wide
        && roster
            .rows
            .iter()
            .filter(|r| r.name != RowName::Account)
            .any(|r| display_width(&radio_of(r, "   ", "").text) > inner);
    let mut body: Vec<String> = Vec::new();
    let mut hits: Vec<(usize, &'static str, usize, Vec<CellSpan>)> = Vec::new();
    for (i, row) in roster.rows.iter().enumerate() {
        let prefix = if state.row == i {
            format!(" {} ", g.cursor)
        } else {
            "   ".to_owned()
        };
        let label_part = pad_right(&format!("{}{}", row.label, mark_of(row)), label_width);
        let hovered_here = i32::try_from(i).is_ok_and(|ii| ii == hover_row);
        let (mut line, mut cells) = if row.name == RowName::Account {
            account_strip(row, state, inp.limits, &prefix, &label_part, inner, &g, hovered_here)
        } else {
            let radio = radio_of(row, &prefix, if hovered_here { &state.hover_value } else { "" });
            (radio.text, radio.cells)
        };
        if (collapse_all && row.name != RowName::Account) || display_width(&line) > inner {
            line = format!(
                "{prefix}{label_part}{} {} {}",
                g.l_angle,
                ctx.text(roster, row, shown_of(row)),
                g.r_angle
            );
            cells = Vec::new();
        }
        hits.push((i, row.name.as_str(), body.len(), cells));
        body.push(line);
        if row.name == RowName::Account
            && let Some(l) = limit
        {
            body.extend(limit_lines(l, 3 + label_width, inner, wide, &g));
        }
    }

    let head: Vec<String> = if wide {
        new_box(&[header], box_width, "", &g, inp.style.ascii)
    } else {
        vec![format!(" {header}")]
    };
    let offset = head.len() + 1;
    let mut map = RowMap {
        rows: hits
            .into_iter()
            .map(|(index, name, y, cells)| RowHit {
                index,
                name: name.to_owned(),
                y: i32::try_from(offset + y).unwrap_or(0),
                cells,
            })
            .collect(),
        ..RowMap::default()
    };
    let mut lines = head;
    lines.push(String::new());
    lines.extend(body);
    lines.push(String::new());
    lines.push(format!("  {}", std::iter::repeat_n(g.h, box_width - 4).collect::<String>()));
    if !restored.is_empty() {
        let when = if restored_age == "just now" {
            "just now".to_owned()
        } else {
            format!("{restored_age} ago")
        };
        lines.push(format!("  * restored ({when}), ctrl+r resets"));
    }
    let footer = launch_footer(&g, fw, inp.style.color);
    let light = FooterLight {
        hover: frame::footer_hover(&footer, state.hover),
        selected: Vec::new(),
    };
    let painter = |l: &str, enabled: bool| add_launch_color(l, enabled, &g, roster);
    let lines = frame::complete_frame(&lines, &footer, fw, inp.style, &painter, &light, &mut map);
    Frame { lines, map }
}

/// The footer: arrows are not clickable ("w/s" names two directions), every single action is.
pub fn launch_footer(g: &Glyphs, width: usize, color: bool) -> Footer {
    new_hint_footer(
        &[
            Hint::plain("w/s", "row"),
            Hint::plain("a/d", "value"),
            Hint::key("enter", "next", "Enter"),
            Hint::ch("u", "maintenance", 'u'),
            Hint::key("esc", "quit", "Escape"),
        ],
        g,
        width,
        !color,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the strip reads the row, the state and the geometry together"
)]
fn account_strip(
    row: &super::RowDef,
    state: &LaunchState,
    limits: &Limits,
    prefix: &str,
    label_part: &str,
    inner: usize,
    g: &Glyphs,
    hovered_here: bool,
) -> (String, Vec<CellSpan>) {
    let joiner = format!(" {} ", g.hint_sep);
    let mut cells: Vec<String> = Vec::new();
    for with_percent in [true, false] {
        cells = row
            .values
            .iter()
            .map(|v| {
                let text = match limits.get(v).and_then(|l| l.five_hour) {
                    Some(p) if with_percent => format!("{v} {p}%"),
                    _ => v.clone(),
                };
                if v.eq_ignore_ascii_case(&state.account) {
                    format!("[{text}]")
                } else {
                    text
                }
            })
            .collect();
        if display_width(&format!("{prefix}{label_part}{}", cells.join(&joiner))) <= inner {
            break;
        }
    }
    let draw: Vec<String> = cells
        .iter()
        .zip(&row.values)
        .map(|(c, v)| {
            if hovered_here && v.eq_ignore_ascii_case(&state.hover_value) {
                theme::add_hover_span(c)
            } else {
                c.clone()
            }
        })
        .collect();
    let line = format!("{prefix}{label_part}{}", draw.join(&joiner));
    let spans = measure_cell_spans(&cells, &joiner, display_width(&format!("{prefix}{label_part}")), &row.values);
    (line, spans)
}

static SGR_ROW_LABEL: LazyLock<Regex> = LazyLock::new(|| theme::re(r"^.{3}account[\*\s]"));
static COMPACT_PREFIX: LazyLock<Regex> = LazyLock::new(|| theme::re("^.{3}[a-z]+\\*? +(?:[^\\s\\[\\]]+ )*\u{4}?$"));
static BRACKET: LazyLock<Regex> = LazyLock::new(|| theme::re(r"\[([^\[\]]+)\]"));
static PERCENT: LazyLock<Regex> = LazyLock::new(|| theme::re(r"(\d+)%"));
static ROW_WORDS: LazyLock<Regex> = LazyLock::new(|| theme::re(r"\b(account|model|effort|advisor|permission|remote|mode)\b"));
static AGE: LazyLock<Regex> = LazyLock::new(|| theme::re(r"(\d+ (?:min|h|d) ago|just now)"));

/// The selected cell's bracket `[...]` with at most one nested pair, starting at `open`
/// (a char index of `[`); returns the exclusive end char index.
fn selected_bracket_end(chars: &[char], open: usize) -> Option<usize> {
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

fn is_selected_anchor(chars: &[char], open: usize, on: char) -> bool {
    if open >= 2 && chars.get(open - 2) == Some(&on) && chars.get(open - 1) == Some(&' ') {
        return true;
    }
    let prefix: String = chars.get(..open).map_or_else(String::new, |c| c.iter().collect());
    let not_account = prefix.chars().skip(3).collect::<String>();
    COMPACT_PREFIX.is_match(&prefix) && !not_account.starts_with("account")
}

/// `Add-LaunchColor`: painted AFTER layout; the selected cell first (anchored on the On glyph or
/// the compact row's label), then the account tab, percentages, row words, cursor, sparkle, ages.
pub fn add_launch_color(line: &str, enabled: bool, g: &Glyphs, roster: &Roster) -> String {
    if !enabled || line.is_empty() {
        return line.to_owned();
    }
    let is_account_row = SGR_ROW_LABEL.is_match(line);
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
    out = out.replace(g.cursor, &format!("{}{}{}", theme::BRIGHT_YELLOW, g.cursor, theme::RESET));
    out = out.replace(g.sparkle, &format!("{}{}{}", theme::ACCENT, g.sparkle, theme::RESET));
    AGE.replace_all(&out, format!("{}${{1}}{}", theme::DIM, theme::RESET).as_str())
        .into_owned()
}

#[cfg(test)]
mod tests;
