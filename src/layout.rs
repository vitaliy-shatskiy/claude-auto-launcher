//! Pure geometry in display cells (`Layout.ps1`): width, truncation, wrapping, boxes, panes.

use unicode_general_category::{GeneralCategory, get_general_category};

use crate::theme::Glyphs;

/// East Asian Wide and Fullwidth plus the emoji blocks Windows Terminal draws two cells wide.
const WIDE_RANGES: &[(u32, u32)] = &[
    (0x1100, 0x115F),
    (0x2E80, 0x303E),
    (0x3041, 0x33FF),
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xA000, 0xA4CF),
    (0xA960, 0xA97F),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFE10, 0xFE19),
    (0xFE30, 0xFE6F),
    (0xFF00, 0xFF60),
    (0xFFE0, 0xFFE6),
    (0x1F300, 0x1F64F),
    (0x1F680, 0x1F6FF),
    (0x1F900, 0x1F9FF),
    (0x20000, 0x2FFFD),
    (0x30000, 0x3FFFD),
    (0x231A, 0x231B),
    (0x23E9, 0x23F3),
    (0x23F8, 0x23FA),
    (0x25FD, 0x25FE),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267F, 0x267F),
    (0x2693, 0x2693),
    (0x26A1, 0x26A1),
    (0x26AA, 0x26AB),
    (0x26BD, 0x26BE),
    (0x26C4, 0x26C5),
    (0x26CE, 0x26CE),
    (0x26D4, 0x26D4),
    (0x26EA, 0x26EA),
    (0x26F2, 0x26F3),
    (0x26F5, 0x26F5),
    (0x26FA, 0x26FA),
    (0x26FD, 0x26FD),
    (0x2705, 0x2705),
    (0x270A, 0x270B),
    (0x2728, 0x2728),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2B05, 0x2B07),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
    (0x1F004, 0x1F0CF),
    (0x1F18E, 0x1F19A),
    (0x1F1E6, 0x1F1FF),
    (0x1F200, 0x1F2FF),
];

/// `Get-CodePointWidth`: C0 zero, ASCII one, marks and format characters zero, wide ranges two.
pub fn char_width(c: char) -> usize {
    let cp = u32::from(c);
    if cp < 0x20 {
        return 0;
    }
    if cp < 0x7F {
        return 1;
    }
    if matches!(
        get_general_category(c),
        GeneralCategory::NonspacingMark | GeneralCategory::EnclosingMark | GeneralCategory::Format
    ) {
        return 0;
    }
    if WIDE_RANGES.iter().any(|&(lo, hi)| cp >= lo && cp <= hi) {
        2
    } else {
        1
    }
}

/// `Get-DisplayWidth`: the cells a string occupies.
pub fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// The one-cell truncation marker.
pub fn ellipsis(ascii: bool) -> char {
    if ascii { '~' } else { '\u{2026}' }
}

/// `Limit-Cells`: the longest prefix fitting `max` cells, cut between whole characters.
pub fn limit_cells(text: &str, max: usize) -> String {
    let mut w = 0;
    let mut out = String::new();
    for c in text.chars() {
        let cw = char_width(c);
        if w + cw > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out
}

/// `Limit-CellsRight`: the longest suffix fitting `max` cells.
pub fn limit_cells_right(text: &str, max: usize) -> String {
    let mut w = 0;
    let mut start = text.len();
    for (i, c) in text.char_indices().rev() {
        let cw = char_width(c);
        if w + cw > max {
            break;
        }
        w += cw;
        start = i;
    }
    text.get(start..).unwrap_or_default().to_owned()
}

/// `Limit-Line`: truncate with a one-cell marker rather than let the console wrap.
pub fn limit_line(text: &str, max: usize, ascii: bool) -> String {
    if display_width(text) <= max {
        return text.to_owned();
    }
    if max <= 1 {
        return limit_cells(text, max);
    }
    let mut s = limit_cells(text, max - 1);
    s.push(ellipsis(ascii));
    s
}

/// `Limit-Path`: middle truncation that keeps the leaf and at least one cell of head.
pub fn limit_path(text: &str, max: usize, ascii: bool) -> String {
    if display_width(text) <= max {
        return text.to_owned();
    }
    if max <= 3 {
        return limit_line(text, max, ascii);
    }
    let budget = max - 1;
    let tail_max = budget - 1;
    let leaf = text.rfind(['\\', '/']).and_then(|i| text.get(i..)).unwrap_or_default();
    let tail = if !leaf.is_empty() && display_width(leaf) <= tail_max {
        leaf.to_owned()
    } else {
        limit_cells_right(text, tail_max)
    };
    let head = limit_cells(text, budget.saturating_sub(display_width(&tail)));
    format!("{head}{}{tail}", ellipsis(ascii))
}

/// `Split-TextLines`: word wrap; a token longer than the pane is cut, never dropped.
pub fn split_text_lines(text: &str, width: usize, max_lines: usize, ascii: bool) -> Vec<String> {
    if text.trim().is_empty() || width == 0 {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let mut token = word.to_owned();
        let mut token_w = display_width(&token);
        while token_w > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let head = limit_cells(&token, width);
            if head.is_empty() {
                token.clear();
                break;
            }
            token.drain(..head.len());
            lines.push(head);
            token_w = display_width(&token);
        }
        if current.is_empty() {
            current = token;
        } else if display_width(&current) + 1 + token_w <= width {
            current.push(' ');
            current.push_str(&token);
        } else {
            lines.push(std::mem::replace(&mut current, token));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if max_lines > 0 && lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            if display_width(last) > width.saturating_sub(1) {
                *last = limit_cells(last, width.saturating_sub(1));
            }
            last.push(ellipsis(ascii));
        }
    }
    lines
}

/// `Split-OutputLines`: every physical line wrapped on its own, blank runs collapsed, the head kept.
pub fn split_output_lines(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    if text.trim().is_empty() || width == 0 {
        return Vec::new();
    }
    let mut rows: Vec<String> = Vec::new();
    let mut pending_blank = false;
    for raw in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        if raw.trim().is_empty() {
            if !rows.is_empty() {
                pending_blank = true;
            }
            continue;
        }
        if pending_blank {
            rows.push(String::new());
            pending_blank = false;
        }
        rows.extend(split_text_lines(raw, width, 0, false));
    }
    if max_lines > 0 && rows.len() > max_lines {
        let keep = max_lines - 1;
        let more = rows.len() - keep;
        rows.truncate(keep);
        rows.push(limit_line(&format!("(+{more} more lines)"), width, false));
    }
    rows
}

fn repeat(c: char, n: usize) -> String {
    std::iter::repeat_n(c, n).collect()
}

/// `New-Box`: a frame of exactly `width` columns; content is truncated, never wrapped.
pub fn new_box(lines: &[String], width: usize, title: &str, g: &Glyphs, ascii: bool) -> Vec<String> {
    if width < 3 {
        return Vec::new();
    }
    let inner = width - 2;
    let mut top = String::from(g.tl);
    if title.is_empty() {
        top.push_str(&repeat(g.h, inner));
    } else {
        let label = limit_line(&format!(" {title} "), inner.saturating_sub(2), ascii);
        top.push(g.h);
        top.push_str(&label);
        top.push_str(&repeat(g.h, inner.saturating_sub(display_width(&label) + 1)));
    }
    top.push(g.tr);
    let mut out = vec![top];
    for line in lines {
        let body = limit_line(line, inner, ascii);
        let pad = inner.saturating_sub(display_width(&body));
        out.push(format!("{}{body}{}{}", g.v, " ".repeat(pad), g.v));
    }
    out.push(format!("{}{}{}", g.bl, repeat(g.h, inner), g.br));
    out
}

/// `Join-Panes`: side by side with one divider, both panes padded in cells.
pub fn join_panes(left: &[String], right: &[String], left_w: usize, right_w: usize, g: &Glyphs, ascii: bool) -> Vec<String> {
    let height = left.len().max(right.len());
    (0..height)
        .map(|i| {
            let lc = limit_line(left.get(i).map_or("", String::as_str), left_w, ascii);
            let rc = limit_line(right.get(i).map_or("", String::as_str), right_w, ascii);
            format!(
                "{lc}{}{}{rc}{}",
                " ".repeat(left_w.saturating_sub(display_width(&lc))),
                g.v,
                " ".repeat(right_w.saturating_sub(display_width(&rc)))
            )
        })
        .collect()
}

/// `Get-Viewport`: keep the selection centred, clamped at both ends.
pub fn viewport(count: usize, index: usize, visible: usize) -> (usize, usize) {
    let v = visible.min(count).max(1);
    let start = index.saturating_sub(v / 2).min(count.saturating_sub(v));
    (start, v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::glyphs;

    #[test]
    fn width_counts_wide_zero_width_and_emoji() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("あい"), 4);
        assert_eq!(display_width("e\u{301}"), 1, "combining mark is zero cells");
        assert_eq!(display_width("a\u{200D}b"), 2, "ZWJ is a format character");
        assert_eq!(display_width("\u{1F600}"), 2);
        assert_eq!(display_width("\u{2B06}"), 2, "the update arrow is drawn wide");
        assert_eq!(display_width("\u{1}x\u{2}"), 1, "markers cost nothing");
        assert_eq!(display_width("привіт"), 6);
    }

    #[test]
    fn limit_line_never_splits_a_wide_char() {
        assert_eq!(limit_line("あいう", 4, false), "あ\u{2026}");
        assert_eq!(limit_line("abcdef", 4, true), "abc~");
        assert_eq!(limit_line("abc", 1, false), "a");
    }

    #[test]
    fn limit_path_keeps_leaf_and_drive() {
        let p = r"C:\Users\sample\Desktop\Projects\workspace\alpha";
        let cut = limit_path(p, 20, false);
        assert_eq!(display_width(&cut), 20);
        assert!(cut.starts_with("C:"));
        assert!(cut.ends_with(r"\alpha"));
    }

    #[test]
    fn split_text_wraps_and_marks_truncation_once() {
        let l = split_text_lines("one two three four five", 9, 2, false);
        assert_eq!(l, ["one two", "three\u{2026}"]);
    }

    #[test]
    fn split_text_cuts_a_long_token() {
        assert_eq!(split_text_lines("abcdefghij", 4, 0, false), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn box_is_exact_width() {
        let g = glyphs(true);
        let b = new_box(&["hi".to_owned()], 10, "", &g, true);
        assert_eq!(b, ["+--------+", "|hi      |", "+--------+"]);
        let t = new_box(&[], 12, "T", &g, true);
        assert_eq!(t.first().map(String::as_str), Some("+- T ------+"));
    }

    #[test]
    fn viewport_centres_and_clamps() {
        assert_eq!(viewport(10, 0, 4), (0, 4));
        assert_eq!(viewport(10, 5, 4), (3, 4));
        assert_eq!(viewport(10, 9, 4), (6, 4));
        assert_eq!(viewport(2, 1, 4), (0, 2));
    }

    #[test]
    fn output_lines_keep_head_and_count_rest() {
        let r = split_output_lines("a\n\n\nb\nc\nd", 20, 3);
        assert_eq!(r, ["a", "", "(+3 more lines)"]);
    }
}
