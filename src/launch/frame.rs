//! The launch screen's frame (limit lines). Pure.

use std::sync::LazyLock;

use regex::Regex;

use super::{LaunchState, OptionText, Roster, RowName, is_hidden_row_value, visible_row_values};
use crate::frame::{
    self, BODY_TOP, CellSpan, Footer, FooterLight, Frame, Hint, MIN_HEIGHT, MIN_WIDTH, RadioSpec, RowHit, RowMap, Style, TWO_PANE_WIDTH,
    measure_cell_spans, new_hint_footer, new_radio_row,
};
use crate::layout::display_width;
use crate::theme::{self, Glyphs};
use crate::types::{DefaultLabels, LimitRecord, Limits, VersionInfo};

/// Everything the launch frame draws from.
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
    /// The self-update line, drawn dim on the page's last row.
    pub notice: Option<&'a str>,
    /// How many warnings maintenance holds: the footer's badge.
    pub warnings: usize,
}

/// The account's limits for the header's right end, `5h 4% · 7d 96% · fable 24% · 4 h ago`:
/// percentages only, the data age last; empty without a record.
pub fn limits_text(l: &LimitRecord, g: &Glyphs) -> String {
    let mut items = Vec::new();
    if let Some(p) = l.five_hour {
        items.push(format!("5h {p}%"));
    }
    if let Some(p) = l.seven_day {
        items.push(format!("7d {p}%"));
    }
    if let Some(p) = l.model {
        let label = l
            .model_label
            .as_deref()
            .filter(|s| !s.is_empty())
            .map_or_else(|| "model".to_owned(), str::to_lowercase);
        let age = l.model_age.as_deref().map_or_else(String::new, |a| format!(" ({a})"));
        items.push(format!("{label} {p}%{age}"));
    }
    if items.is_empty() {
        return String::new();
    }
    if !l.age_text.is_empty() {
        items.push(l.age_text.clone());
    }
    items.join(&format!(" {} ", g.dot))
}
fn pad_right(s: &str, width: usize) -> String {
    format!("{s:<width$}")
}

#[expect(clippy::too_many_lines, reason = "one row loop, as in Screens.ps1")]
pub fn launch_frame(roster: &Roster, inp: &LaunchFrameInput<'_>) -> Frame {
    let state = inp.state;
    let restored: Vec<String> = inp.restored.map_or_else(|| state.restored.clone(), |(r, _)| r.to_vec());
    if inp.width < MIN_WIDTH || inp.height < MIN_HEIGHT {
        return Frame {
            lines: frame::too_small_frame(inp.width, inp.height, inp.style.ascii),
            map: RowMap::default(),
        };
    }
    let g = theme::glyphs(inp.style.ascii);
    let fw = frame::frame_width(inp.width);
    let box_width = fw;
    let inner = box_width - 4;
    let wide = inp.width >= TWO_PANE_WIDTH;
    let label_width = if wide {
        12
    } else {
        roster.rows.iter().map(|r| r.label.len()).max().unwrap_or(0) + 3
    };

    let title = format!("{} claude-auto", g.sparkle);
    let mut version = String::new();
    if let Some(inst) = inp.version.installed.as_deref().filter(|s| !s.is_empty()) {
        version = format!("v{inst}");
        if let Some(newest) = inp.version.newest.as_deref().filter(|n| !n.is_empty() && *n != inst) {
            version = format!("{version} {} {newest}", g.up);
        }
    }
    let limit = inp.limits.get(&state.account);
    let limits = limit.map_or_else(String::new, |l| limits_text(l, &g));
    // The limits go first when the box is too narrow, then the version.
    let rights: Vec<String> = [format!("{version}   {limits}"), version.clone()]
        .into_iter()
        .map(|r| r.trim().to_owned())
        .filter(|r| !r.is_empty())
        .collect();
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
        // Only a row that does not fit collapses to `< value >`: the others stay readable.
        if display_width(&line) > inner {
            line = format!(
                "{prefix}{label_part}{} {} {}",
                g.l_angle,
                ctx.text(roster, row, shown_of(row)),
                g.r_angle
            );
            cells = Vec::new();
        }
        // The pointer on the row outside its values paints the label: hover never selects.
        if hovered_here
            && state.hover_value.is_empty()
            && let Some(rest) = line.strip_prefix(&format!("{prefix}{label_part}"))
        {
            let label = format!("{}{}", row.label, mark_of(row));
            let pad = " ".repeat(label_width.saturating_sub(label.len()));
            line = format!("{prefix}{}{pad}{rest}", theme::add_hover_span(&label));
        }
        hits.push((i, row.name.as_str(), body.len(), cells));
        body.push(line);
    }

    // The rows start under the header box, at the terminal's first column.
    let mut map = RowMap {
        rows: hits
            .into_iter()
            .map(|(index, name, y, cells)| RowHit {
                index,
                name: name.to_owned(),
                y: i32::try_from(BODY_TOP + y).unwrap_or(0),
                cells,
            })
            .collect(),
        ..RowMap::default()
    };
    let extra = LaunchHints {
        warnings: inp.warnings,
        restored: !restored.is_empty(),
    };
    let footer = launch_footer(&g, fw, inp.style.color, &extra);
    // The self-update notice closes the body, dim, a blank row under the rows.
    if let Some(n) = inp.notice {
        body.push(String::new());
        body.push(format!("{}{}{n}{}", frame::PAD, theme::DIM_OPEN, theme::DIM_CLOSE));
    }
    let lines = frame::header_page(body, inp.width, &title, &rights, inp.style);
    let light = FooterLight {
        hover: frame::footer_hover(&footer, state.hover),
        selected: Vec::new(),
    };
    let painter = |l: &str, enabled: bool| add_launch_color(l, enabled, &g, roster);
    let lines = frame::complete_frame(&lines, &footer, fw, inp.style, &painter, &light, &mut map);
    Frame { lines, map }
}

/// The footer: arrows are not clickable ("w/s" names two directions), every single action is.
pub fn launch_footer(g: &Glyphs, width: usize, color: bool, extra: &LaunchHints) -> Footer {
    let badge = if extra.warnings > 0 {
        format!("{} {}", g.warn, extra.warnings)
    } else {
        String::new()
    };
    let mut hints = vec![
        Hint::plain("w/s", "row").kept(),
        Hint::plain(frame::switch_keys(g), "switch").kept(),
        Hint::key("enter", "next", "Enter"),
        Hint::ch("u", "maintenance", 'u').badged(badge),
    ];
    if extra.restored {
        hints.push(Hint::plain("ctrl+r", "reset").kept());
    }
    hints.push(Hint::key("esc", "quit", "Escape"));
    new_hint_footer(&hints, g, width, !color)
}

/// What the launch footer shows besides its fixed keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaunchHints {
    /// Collected warnings: a yellow `⚠ N` badge on `u maintenance`.
    pub warnings: usize,
    /// A row was restored: `ctrl+r reset` is offered.
    pub restored: bool,
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
static ROW_WORDS: LazyLock<Regex> = LazyLock::new(|| theme::re(r"\b(account|action|model|effort|advisor|permission|remote|mode)\b"));
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

/// Painted AFTER layout; the selected cell first (anchored on the On glyph or
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
