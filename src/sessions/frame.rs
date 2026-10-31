//! The picker frame (`Get-PickerFrame` and its helpers): master-detail at >= 100 columns, one
//! column below with the preview underneath, and the frame memo that repaints only the lines a
//! hover change touched.

use std::hash::{DefaultHasher, Hash, Hasher};

use super::listing::{Message, Session, Speaker};
use super::transcript::clean_text;
use crate::frame::{
    Footer, FooterLight, Frame, Hint, ListMap, MIN_HEIGHT, MIN_WIDTH, RowMap, SHORT_PICKER_HEIGHT, Style, TWO_PANE_WIDTH, add_hint_color,
    complete_frame, footer_hover, format_body_line, frame_width, new_hint_footer, too_small_frame,
};
use crate::layout::{join_panes, limit_line, new_box, split_text_lines, viewport};
use crate::list::{ListRow, add_hover_span_at, add_picker_color, new_list_row, row_band_length};
use crate::theme::{self, Glyphs, glyphs, strip_ctl};

/// The clock a frame is drawn against: now, and the local offset from UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Clock {
    pub now_ms: i64,
    pub offset_minutes: i32,
}

/// `none` = no project context (no tab hint); `project` = the pool is one project; `all`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PickerScope {
    None,
    Project,
    All,
}

/// Everything one picker frame is drawn from.
#[derive(Debug, Clone)]
pub struct PickerView<'a> {
    pub sessions: &'a [&'a Session],
    pub index: usize,
    pub filter: &'a str,
    pub width: usize,
    pub height: usize,
    pub clock: Clock,
    pub style: Style,
    pub scope: PickerScope,
    pub project_name: &'a str,
    /// Footer button index under the mouse, -1 none.
    pub hover: i32,
    /// List row under the mouse, -1 none.
    pub hover_row: i32,
}

/// `Select-ResumableSessions`' rule: not headless and something was asked.
pub fn resumable(s: &Session) -> bool {
    !s.headless && s.prompt_count > 0
}

/// `Select-SessionMatch`: a case-insensitive substring over project, worktree and the texts.
pub fn matches(s: &Session, filter: &str) -> bool {
    let f = filter.trim();
    if f.is_empty() {
        return true;
    }
    let hay = format!("{} {} {} {} {}", s.project, s.worktree, s.title, s.last_user, s.last_assistant);
    hay.to_lowercase().contains(&f.to_lowercase())
}

/// `Format-PromptCount`: the number, plus '+' from the capped flag.
pub fn prompt_count_text(s: &Session) -> String {
    if s.prompt_count_capped {
        format!("{}+", s.prompt_count)
    } else {
        s.prompt_count.to_string()
    }
}

/// .NET `{0:N0}` in the invariant culture: rounded half away from zero, thousands grouped.
fn n0(x: f64) -> String {
    #[expect(clippy::cast_possible_truncation, reason = "ages and sizes are far inside i64")]
    let v = x.round() as i64;
    let digits = v.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if v < 0 { format!("-{out}") } else { out }
}

/// `Format-RelativeAge`.
#[expect(clippy::cast_precision_loss, reason = "a millisecond age fits f64 exactly enough")]
pub fn relative_age(from_ms: i64, now_ms: i64) -> String {
    let secs = (now_ms - from_ms) as f64 / 1000.0;
    if secs < 60.0 {
        "now".into()
    } else if secs < 3600.0 {
        format!("{} min", n0(secs / 60.0))
    } else if secs < 86_400.0 {
        format!("{} h", n0(secs / 3600.0))
    } else {
        format!("{} d", n0(secs / 86_400.0))
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `ToString('dd MMM HH:mm')` in the invariant culture, at the clock's local offset.
pub fn local_stamp(ms: i64, offset_minutes: i32) -> String {
    let minutes = ms.div_euclid(60_000) + i64::from(offset_minutes);
    let (days, rem) = (minutes.div_euclid(1440), minutes.rem_euclid(1440));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let month = MONTHS.get(usize::try_from(m - 1).unwrap_or(0)).unwrap_or(&"Jan");
    format!("{d:02} {month} {:02}:{:02}", rem / 60, rem % 60)
}

/// `Get-SessionExchange`: the recent messages, or the last question and answer.
pub fn exchange(s: &Session) -> Vec<Message> {
    if !s.recent.is_empty() {
        return s.recent.clone();
    }
    let user = [&s.last_user, &s.title]
        .into_iter()
        .find(|t| !t.is_empty())
        .map_or_else(|| "(nothing was asked)".to_owned(), Clone::clone);
    let reply = if s.last_assistant.is_empty() {
        "(no reply)".to_owned()
    } else {
        s.last_assistant.clone()
    };
    vec![
        Message {
            speaker: Speaker::User,
            text: user,
        },
        Message {
            speaker: Speaker::Assistant,
            text: reply,
        },
    ]
}

/// `Get-ExchangeLines`: as many of the newest messages as fit, oldest on top, each capped so a
/// long one cannot eat the pane.
pub fn exchange_lines(messages: &[Message], width: usize, max_lines: usize, g: &Glyphs, ascii: bool) -> Vec<String> {
    const MIN_PER_MESSAGE: usize = 5;
    if max_lines == 0 || width == 0 {
        return Vec::new();
    }
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut used = 0;
    for (i, m) in messages.iter().enumerate().rev() {
        if used >= max_lines {
            break;
        }
        let left = max_lines - used;
        let cap = MIN_PER_MESSAGE.max(left / (i + 1).min(3));
        let budget = cap.min(left);
        let prefix = match m.speaker {
            Speaker::User => format!("you {} ", g.r_angle),
            Speaker::Assistant => format!("claude {} ", g.r_angle),
        };
        let plen = prefix.chars().count();
        let wrapped = split_text_lines(&strip_ctl(&m.text), width.saturating_sub(plen).max(1), budget, ascii);
        if wrapped.is_empty() {
            continue;
        }
        let lines: Vec<String> = wrapped
            .iter()
            .enumerate()
            .map(|(j, w)| {
                if j == 0 {
                    format!("{prefix}{w}")
                } else {
                    format!("{}{w}", " ".repeat(plen))
                }
            })
            .collect();
        used += lines.len();
        blocks.insert(0, lines);
    }
    blocks.concat()
}

struct RowCtx<'a> {
    index: usize,
    hover_row: i32,
    scope: PickerScope,
    clock: Clock,
    g: &'a Glyphs,
    width: usize,
    ascii: bool,
}

/// `New-SessionRow`.
fn session_row(it: &Session, row: usize, c: &RowCtx<'_>) -> String {
    let mark = if row == c.index {
        format!(" {} ", c.g.cursor)
    } else {
        "   ".to_owned()
    };
    let mut place = if c.scope == PickerScope::Project {
        String::new()
    } else {
        it.project.clone()
    };
    if !it.worktree.is_empty() {
        place = format!("{} {}", c.g.worktree, it.worktree);
    }
    let age = relative_age(it.modified_ms, c.clock.now_ms);
    let what = [&it.last_user, &it.title]
        .into_iter()
        .find(|t| !t.is_empty())
        .cloned()
        .unwrap_or_default();
    let n = |s: &str| i64::try_from(s.chars().count()).unwrap_or(0);
    let room = i64::try_from(c.width).unwrap_or(0) - n(&mark) - n(&age) - 2;
    let mut label = place.clone();
    if !what.is_empty() && room > n(&place) + 4 {
        let sep = if place.is_empty() { "" } else { "  " };
        let max = usize::try_from(room - n(&place) - n(sep)).unwrap_or(0);
        label = format!("{place}{sep}{}", limit_line(&what, max, c.ascii));
    }
    new_list_row(&ListRow {
        mark: &mark,
        label: &label,
        age: &age,
        width: c.width,
        ascii: c.ascii,
        trailing_space: true,
        hover: i32::try_from(row).is_ok_and(|r| r == c.hover_row),
        ..ListRow::default()
    })
}

fn picker_footer(scope: PickerScope, g: &Glyphs, width: usize, plain: bool) -> Footer {
    let mut hints = vec![
        Hint::plain("w/s", "move"),
        Hint::ch("/", "filter", '/'),
        Hint::key("enter", "open", "Enter"),
        Hint::ch("f", "fork", 'f'),
    ];
    match scope {
        PickerScope::Project => hints.push(Hint::key("tab", "all projects", "Tab")),
        PickerScope::All => hints.push(Hint::key("tab", "this project", "Tab")),
        PickerScope::None => {}
    }
    hints.push(Hint::key("esc", "back", "Escape"));
    new_hint_footer(&hints, g, width, plain)
}

/// Where a hover band goes on one memoised row: line, first character, length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Band {
    y: usize,
    start: usize,
    len: usize,
}

/// The last frame built, for the hover-only repaint (`Get-MemoFrame`).
#[derive(Debug, Clone, Default)]
pub struct Memo {
    key: u64,
    unpainted: Vec<String>,
    painted: Vec<String>,
    hover_row: i32,
    hover: i32,
    rows: Vec<(i32, Band)>,
    footer: Option<Footer>,
    footer_index: usize,
    width: usize,
    map: RowMap,
}

fn memo_key(v: &PickerView<'_>) -> u64 {
    let mut h = DefaultHasher::new();
    (
        v.width,
        v.height,
        v.style.color,
        v.style.ascii,
        v.index,
        v.filter,
        v.scope,
        v.project_name,
    )
        .hash(&mut h);
    (v.clock.now_ms.div_euclid(60_000), v.clock.offset_minutes).hash(&mut h);
    for s in v.sessions {
        s.hash(&mut h);
    }
    h.finish()
}

fn painter(g: Glyphs) -> impl Fn(&str, bool) -> String {
    move |l: &str, c: bool| add_picker_color(l, c, &g)
}

impl Memo {
    /// The memoised frame repainted for a new hover, or `None` to rebuild.
    fn hit(&mut self, key: u64, v: &PickerView<'_>, g: Glyphs) -> Option<Frame> {
        if self.key != key || self.painted.is_empty() {
            return None;
        }
        let paint = painter(g);
        let mut painted = self.painted.clone();
        if self.hover_row != v.hover_row {
            for idx in [self.hover_row, v.hover_row] {
                let Some((_, b)) = self.rows.iter().find(|(r, _)| *r == idx) else {
                    continue;
                };
                let mut line = self.unpainted.get(b.y)?.clone();
                if idx == v.hover_row {
                    line = add_hover_span_at(&line, b.start, b.len)?;
                }
                *painted.get_mut(b.y)? = format_body_line(&line, self.width, v.style, &paint);
            }
        }
        if self.hover != v.hover
            && let Some(footer) = &self.footer
        {
            let light = FooterLight {
                hover: footer_hover(footer, v.hover),
                selected: Vec::new(),
            };
            for (f, fl) in footer.lines.iter().enumerate() {
                let y = self.footer_index + f;
                *painted.get_mut(y)? = add_hint_color(self.unpainted.get(y)?, &fl.spans, v.style.color, &light);
            }
        }
        self.painted.clone_from(&painted);
        self.hover_row = v.hover_row;
        self.hover = v.hover;
        Some(Frame {
            lines: painted,
            map: self.map.clone(),
        })
    }
}

fn picker_title(v: &PickerView<'_>, shown: usize, hidden: usize, headless: usize, h: char) -> String {
    let mut parts = vec![format!("resume {h} {shown} sessions")];
    if v.scope == PickerScope::Project {
        parts.push(if v.project_name.is_empty() {
            "this project".to_owned()
        } else {
            v.project_name.to_owned()
        });
    }
    if hidden > 0 {
        parts.push(format!("{hidden} empty hidden"));
    }
    if headless > 0 {
        parts.push(format!("{headless} headless hidden"));
    }
    if !v.filter.is_empty() {
        parts.push(format!("filter: {}", clean_text(v.filter)));
    }
    parts.join(&format!(" {h} "))
}

/// The selected session's preview: the detail pane (wide) or the block under the list (narrow).
struct Preview<'a> {
    s: &'a Session,
    g: &'a Glyphs,
    ascii: bool,
    offset: i32,
}

impl Preview<'_> {
    fn place(&self) -> String {
        let mut place = strip_ctl(&self.s.project);
        if !self.s.worktree.is_empty() {
            place = format!("{place}  {} {}", self.g.worktree, strip_ctl(&self.s.worktree));
        }
        place
    }

    fn exchange(&self, width: usize, max: usize, indent: &str) -> Vec<String> {
        exchange_lines(&exchange(self.s), width, max, self.g, self.ascii)
            .into_iter()
            .map(|l| format!("{indent}{l}"))
            .collect()
    }

    fn detail(&self, right: usize, body_rows: usize) -> Vec<String> {
        let h = self.g.h;
        let stamp = local_stamp(self.s.modified_ms, self.offset);
        #[expect(clippy::cast_precision_loss, reason = "a file size in KB")]
        let kb = n0(self.s.size_bytes as f64 / 1024.0);
        let mut detail = vec![
            format!(" {}", limit_line(&self.place(), right.saturating_sub(2), self.ascii)),
            format!(" {stamp}  {h}  {} msgs  {h}  {kb} KB", prompt_count_text(self.s)),
            format!(" {}", h.to_string().repeat(right.saturating_sub(2))),
        ];
        let room = body_rows.saturating_sub(detail.len()).max(2);
        detail.extend(self.exchange(right.saturating_sub(2), room, " "));
        detail
    }

    fn below(&self, fw: usize, preview_rows: usize) -> Vec<String> {
        let h = self.g.h;
        let stamp = local_stamp(self.s.modified_ms, self.offset);
        let head = format!("{}  {h}  {} msgs  {h}  {stamp}", self.place(), prompt_count_text(self.s));
        let mut out = vec![format!("  {}", limit_line(&head, fw.saturating_sub(4), self.ascii))];
        if preview_rows > 2 {
            out.push(format!("  {}", h.to_string().repeat(fw.saturating_sub(6))));
        }
        let budget = preview_rows.saturating_sub(out.len()).max(1);
        out.extend(self.exchange(fw.saturating_sub(4), budget, "  "));
        out
    }
}

struct Built {
    lines: Vec<String>,
    footer: Footer,
    rows: Vec<(i32, Band)>,
    map: RowMap,
}

/// `Get-PickerFrame`, memoised: a frame differing only in its hover repaints those lines.
pub fn picker_frame(v: &PickerView<'_>, memo: &mut Memo) -> Frame {
    if v.width < MIN_WIDTH || v.height < MIN_HEIGHT {
        return Frame {
            lines: too_small_frame(v.width, v.height, v.style.ascii),
            map: RowMap {
                list: Some(ListMap::default()),
                ..RowMap::default()
            },
        };
    }
    let g = glyphs(v.style.ascii);
    let key = memo_key(v);
    if let Some(f) = memo.hit(key, v, g) {
        return f;
    }
    let fw = frame_width(v.width);
    let b = build(v, &g, fw);
    let light = FooterLight {
        hover: footer_hover(&b.footer, v.hover),
        selected: Vec::new(),
    };
    let mut map = b.map;
    let paint = painter(g);
    let lines = complete_frame(&b.lines, &b.footer, fw, v.style, &paint, &light, &mut map);
    let mut unpainted: Vec<String> = b
        .lines
        .iter()
        .map(|l| l.replace([theme::HOVER_OPEN, theme::HOVER_CLOSE], ""))
        .collect();
    let footer_index = unpainted.len();
    unpainted.extend(
        b.footer
            .lines
            .iter()
            .map(|fl| limit_line(&theme::add_dim_span_color(&fl.text, false), fw, v.style.ascii)),
    );
    *memo = Memo {
        key,
        unpainted,
        painted: lines.clone(),
        hover_row: v.hover_row,
        hover: v.hover,
        rows: b.rows,
        footer: Some(b.footer),
        footer_index,
        width: fw,
        map: map.clone(),
    };
    Frame { lines, map }
}

fn empty_message(filter: &str, hidden: usize, headless: usize) -> String {
    if !filter.is_empty() {
        "  nothing matches that filter".to_owned()
    } else if headless > 0 {
        format!(
            "  all {} sessions here are empty or headless - nothing to resume",
            hidden + headless
        )
    } else if hidden > 0 {
        format!("  all {hidden} sessions here are empty - nothing to resume")
    } else {
        "  no sessions found".to_owned()
    }
}

/// (list pane, detail pane) widths: 40 % for the list at >= 100 columns, else the whole frame.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a frame width; [int] is banker's rounding"
)]
fn pane_widths(fw: usize, wide: bool) -> (usize, usize) {
    let inner = fw.saturating_sub(2);
    let left = if wide {
        28.max((inner as f64 * 0.4).round_ties_even() as usize)
    } else {
        inner
    };
    (left, inner.saturating_sub(left + 1))
}

#[expect(clippy::many_single_char_names, reason = "the builder's names follow Get-PickerFrame")]
fn build(v: &PickerView<'_>, g: &Glyphs, fw: usize) -> Built {
    let ascii = v.style.ascii;
    let resumable_rows: Vec<&Session> = v.sessions.iter().copied().filter(|s| resumable(s)).collect();
    let headless = v.sessions.iter().filter(|s| s.headless).count();
    let hidden = v.sessions.len().saturating_sub(resumable_rows.len() + headless);
    let items: Vec<&Session> = resumable_rows.into_iter().filter(|s| matches(s, v.filter)).collect();
    let index = v.index.min(items.len().saturating_sub(1));
    let h = g.h;
    let title = picker_title(v, items.len(), hidden, headless, h);
    let footer = picker_footer(v.scope, g, fw, !v.style.color);
    if items.is_empty() {
        let msg = empty_message(v.filter, hidden, headless);
        return Built {
            lines: new_box(&[String::new(), msg, String::new()], fw, &title, g, ascii),
            footer,
            rows: Vec::new(),
            map: RowMap {
                list: Some(ListMap::default()),
                ..RowMap::default()
            },
        };
    }
    let body_rows = v.height.saturating_sub(3 + footer.lines.len()).max(3);
    let wide = v.width >= TWO_PANE_WIDTH;
    let (left_width, right_width) = pane_widths(fw, wide);
    let ctx = RowCtx {
        index,
        hover_row: v.hover_row,
        scope: v.scope,
        clock: v.clock,
        g,
        width: left_width,
        ascii,
    };
    let s = items.get(index).copied().unwrap_or(items[0]);
    let mut list = Vec::new();
    let mut bands = Vec::new();
    let (preview_rows, list_rows) = if wide {
        (0, body_rows)
    } else {
        let p = if v.height < SHORT_PICKER_HEIGHT { 2 } else { 6 };
        (p, body_rows.saturating_sub(p).max(1))
    };
    let (start, visible) = viewport(items.len(), index, list_rows);
    for (i, it) in items.iter().enumerate().skip(start).take(visible) {
        let row = session_row(it, i, &ctx);
        bands.push((i, i - start, row_band_length(&row, left_width)));
        list.push(row);
    }
    let pv = Preview {
        s,
        g,
        ascii,
        offset: v.clock.offset_minutes,
    };
    let body = if wide {
        let detail = pv.detail(right_width, body_rows);
        join_panes(&list, &detail, left_width, right_width, g, ascii)
    } else {
        let mut body = list;
        body.extend(pv.below(fw, preview_rows));
        body
    };
    let lines = new_box(&body, fw, &title, g, ascii);
    let first_row_y = lines.len() - body.len() - 1;
    let to_i = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
    Built {
        lines,
        footer,
        rows: bands
            .into_iter()
            .map(|(i, y, len)| {
                (
                    to_i(i),
                    Band {
                        y: first_row_y + y,
                        start: 1,
                        len,
                    },
                )
            })
            .collect(),
        map: RowMap {
            list: Some(ListMap {
                first_row_y: to_i(first_row_y),
                row_count: to_i(visible),
                start: to_i(start),
                row_ys: Vec::new(),
            }),
            ..RowMap::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-16 12:00 local at offset 0.
    const GOLDEN_NOW: i64 = 1_789_560_000_000;

    fn golden_sessions() -> Vec<Session> {
        (1..=5i64)
            .map(|i| Session {
                session_id: format!("g{i}"),
                project: format!("project-{i}"),
                worktree: if i == 3 { "feature".into() } else { String::new() },
                title: format!("question {i}"),
                last_user: format!("snippet {i} about the thing that was asked"),
                last_assistant: "reply words ".repeat(usize::try_from(4 * i).unwrap()),
                modified_ms: GOLDEN_NOW - 7 * i * 60_000,
                prompt_count: 3 * i,
                size_bytes: 2048 * u64::try_from(i).unwrap(),
                ..Session::default()
            })
            .collect()
    }

    fn view<'a>(s: &'a [&'a Session], index: usize, w: usize, h: usize) -> PickerView<'a> {
        PickerView {
            sessions: s,
            index,
            filter: "",
            width: w,
            height: h,
            clock: Clock {
                now_ms: GOLDEN_NOW,
                offset_minutes: 0,
            },
            style: Style {
                color: false,
                ascii: false,
            },
            scope: PickerScope::None,
            project_name: "",
            hover: -1,
            hover_row: -1,
        }
    }

    fn golden(name: &str) -> String {
        let path = format!("{}/tests/fixtures/wide-frames/{name}.txt", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
    }

    fn assert_golden(name: &str, actual: &str) {
        let expected = golden(name);
        let first = expected.lines().zip(actual.lines()).position(|(e, a)| e != a);
        assert_eq!(expected, actual, "{name}: first difference at line {first:?}");
    }

    #[test]
    fn golden_clock_is_the_fixture_time() {
        assert_eq!(local_stamp(GOLDEN_NOW, 0), "16 Sep 12:00");
    }

    #[test]
    fn wide_frames_match_the_powershell_goldens() {
        let owned = golden_sessions();
        let s: Vec<&Session> = owned.iter().collect();
        let mut memo = Memo::default();
        assert_golden("picker-100x30", &picker_frame(&view(&s, 1, 100, 30), &mut memo).lines.join("\n"));
        assert_golden("picker-100x24", &picker_frame(&view(&s, 0, 100, 24), &mut memo).lines.join("\n"));
        let colour = PickerView {
            scope: PickerScope::Project,
            project_name: "alpha",
            hover_row: 1,
            hover: 1,
            style: Style { color: true, ascii: false },
            ..view(&s, 2, 120, 30)
        };
        assert_golden(
            "picker-120x30-project-color",
            &picker_frame(&colour, &mut Memo::default()).lines.join("\n"),
        );
    }

    #[test]
    fn memo_repaint_equals_a_rebuild() {
        let owned = golden_sessions();
        let s: Vec<&Session> = owned.iter().collect();
        for (w, h) in [(120, 30), (80, 30), (80, 24)] {
            let base = PickerView {
                style: Style { color: true, ascii: false },
                ..view(&s, 1, w, h)
            };
            let mut memo = Memo::default();
            picker_frame(&base, &mut memo);
            for (hr, hv) in [(2, -1), (0, 2), (-1, -1), (4, 0)] {
                let v = PickerView {
                    hover_row: hr,
                    hover: hv,
                    ..base.clone()
                };
                let key = memo_key(&v);
                assert_eq!(memo.key, key, "the hover is not part of the key");
                let repainted = picker_frame(&v, &mut memo);
                let rebuilt = picker_frame(&v, &mut Memo::default());
                assert_eq!(repainted, rebuilt, "{w}x{h} hover {hr}/{hv}");
            }
        }
    }

    #[test]
    fn a_band_that_does_not_fit_rebuilds() {
        let mut memo = Memo {
            key: 7,
            unpainted: vec!["ab".into()],
            painted: vec!["ab".into()],
            hover_row: -1,
            rows: vec![(0, Band { y: 0, start: 1, len: 5 })],
            ..Memo::default()
        };
        let owned = golden_sessions();
        let s: Vec<&Session> = owned.iter().collect();
        let v = PickerView {
            hover_row: 0,
            ..view(&s, 0, 100, 30)
        };
        assert!(memo.hit(7, &v, glyphs(false)).is_none());
    }

    #[test]
    fn narrow_layout_preview_shrinks_below_thirty_rows() {
        let owned = golden_sessions();
        let s: Vec<&Session> = owned.iter().collect();
        let tall = picker_frame(&view(&s, 0, 80, 30), &mut Memo::default());
        let short = picker_frame(&view(&s, 0, 80, 24), &mut Memo::default());
        assert!(tall.lines.iter().any(|l| l.contains("──────────")), "a rule under the preview head");
        assert!(tall.lines.iter().all(|l| crate::layout::display_width(l) <= 79));
        assert!(short.lines.len() <= 24 && tall.lines.len() <= 30);
        assert!(short.lines.iter().any(|l| l.contains("3 msgs")));
        let map = tall.map.list.unwrap();
        assert_eq!((map.first_row_y, map.start), (1, 0));
    }

    #[test]
    fn empty_and_hidden_counts_are_stated() {
        let mut owned = golden_sessions();
        owned[0].prompt_count = 0;
        owned[1].headless = true;
        let s: Vec<&Session> = owned.iter().collect();
        let f = picker_frame(&view(&s, 0, 100, 30), &mut Memo::default());
        assert!(
            f.lines[0].contains("3 sessions ─ 1 empty hidden ─ 1 headless hidden"),
            "{}",
            f.lines[0]
        );
        let filtered = PickerView {
            filter: "zzz",
            ..view(&s, 0, 100, 30)
        };
        assert!(
            picker_frame(&filtered, &mut Memo::default())
                .lines
                .iter()
                .any(|l| l.contains("nothing matches that filter"))
        );
        let none: Vec<&Session> = Vec::new();
        assert!(
            picker_frame(&view(&none, 0, 100, 30), &mut Memo::default())
                .lines
                .iter()
                .any(|l| l.contains("no sessions found"))
        );
        let small = picker_frame(&view(&s, 0, 40, 10), &mut Memo::default());
        assert!(small.lines[0].contains("need 50x16"));
    }

    #[test]
    fn relative_age_and_counts() {
        let now = 1_000_000_000_000;
        assert_eq!(relative_age(now - 20_000, now), "now");
        assert_eq!(relative_age(now - 5 * 60_000, now), "5 min");
        assert_eq!(relative_age(now - 3 * 3_600_000, now), "3 h");
        assert_eq!(relative_age(now - 2 * 86_400_000, now), "2 d");
        assert_eq!(n0(1234.5), "1,235");
        let capped = Session {
            prompt_count: 3,
            prompt_count_capped: true,
            ..Session::default()
        };
        assert_eq!(prompt_count_text(&capped), "3+");
    }

    #[test]
    fn exchange_falls_back_and_attributes_by_word() {
        let bare = Session::default();
        let e = exchange(&bare);
        assert_eq!((e[0].text.as_str(), e[1].text.as_str()), ("(nothing was asked)", "(no reply)"));
        let g = glyphs(true);
        let lines = exchange_lines(&e, 40, 10, &g, true);
        assert_eq!(lines, ["you > (nothing was asked)", "claude > (no reply)"]);
    }
}
