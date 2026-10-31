//! List-screen pieces the picker (W1) and the project screen (W2) share (`Screens.ps1`): one row
//! layout (`New-ListRow`), the list body painter (`Add-PickerColor`), and the hover-band helpers
//! the frame memo uses. Shared: no phase B worker edits this file.

use std::sync::LazyLock;

use regex::Regex;

use crate::layout::{display_width, limit_line, limit_path};
use crate::theme::{self, Glyphs};

/// `New-ListRow`'s switches and columns.
#[derive(Debug, Clone, Default)]
#[expect(clippy::struct_excessive_bools, reason = "the PowerShell switches, one each")]
pub struct ListRow<'a> {
    pub mark: &'a str,
    pub label: &'a str,
    pub tail: &'a str,
    pub age: &'a str,
    pub width: usize,
    pub ascii: bool,
    /// The picker puts the separator AFTER the age.
    pub trailing_space: bool,
    pub dim_tail: bool,
    pub dim_age: bool,
    /// The tail is a path: cut it in the middle and keep its leaf.
    pub path_tail: bool,
    pub hover: bool,
    /// One cell of right gutter even with an age (the narrow project rows).
    pub age_gutter: bool,
}

/// `New-ListRow`: mark + label + pad + tail + age in exactly `width` cells. The label is clamped
/// first; the tail takes what is left and is dropped below 9 cells. Markers are added AFTER the
/// arithmetic, so a marked row lays out like an unmarked one.
pub fn new_list_row(r: &ListRow<'_>) -> String {
    let mark = if r.mark.is_empty() { "   " } else { r.mark };
    let label_in = theme::strip_ctl(r.label);
    let tail_in = theme::strip_ctl(r.tail);
    let age = theme::strip_ctl(r.age);
    let age_w = display_width(&age);
    let mut age_col = if age.is_empty() {
        String::new()
    } else if r.trailing_space {
        format!("{age} ")
    } else {
        format!(" {age}")
    };
    let mark_w = display_width(mark);
    let age_col_w = display_width(&age_col);
    let extra = usize::from(r.age_gutter && !age.is_empty());
    let label_max = r.width.saturating_sub(mark_w + age_w + 2 + extra).max(1);
    let label = limit_line(&label_in, label_max, r.ascii);
    let label_w = display_width(&label);
    let room = r.width.saturating_sub(mark_w + label_w + age_col_w + 3 + extra);
    let mut tail = if tail_in.is_empty() || room <= 8 {
        String::new()
    } else if r.path_tail {
        limit_path(&tail_in, room, r.ascii)
    } else {
        limit_line(&tail_in, room, r.ascii)
    };
    let gutter = usize::from(age.is_empty() || r.age_gutter);
    let pad = r
        .width
        .saturating_sub(mark_w + label_w + display_width(&tail) + age_col_w + gutter)
        .max(1);
    if r.dim_tail && !tail.is_empty() {
        tail = format!("{}{tail}{}", theme::DIM_OPEN, theme::DIM_CLOSE);
    }
    if r.dim_age && !age_col.is_empty() {
        age_col = format!("{}{age_col}{}", theme::DIM_OPEN, theme::DIM_CLOSE);
    }
    let row = format!("{mark}{label}{}{tail}{age_col}", " ".repeat(pad));
    if r.hover {
        theme::add_hover_span(&format!("{row}{}", " ".repeat(gutter)))
    } else {
        row
    }
}

/// `Get-RowBandLength`: the characters a hovered row's band wraps, its pad included.
pub fn row_band_length(row: &str, pane_width: usize) -> usize {
    let plain: String = row
        .chars()
        .filter(|c| *c != theme::HOVER_OPEN && *c != theme::HOVER_CLOSE)
        .collect();
    plain.chars().count() + pane_width.saturating_sub(display_width(&plain))
}

/// `Add-HoverSpanAt`: the band around character columns of a finished line; `None` when the range
/// does not fit (the caller rebuilds rather than guesses).
pub fn add_hover_span_at(line: &str, start: usize, length: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    if length == 0 || start + length > chars.len() {
        return None;
    }
    let s = |a: usize, b: usize| chars.get(a..b).map_or_else(String::new, |c| c.iter().collect::<String>());
    Some(format!(
        "{}{}{}{}{}",
        s(0, start),
        theme::HOVER_OPEN,
        s(start, start + length),
        theme::HOVER_CLOSE,
        s(start + length, chars.len())
    ))
}

struct PickerPatterns {
    you: Regex,
    claude: Regex,
    age: Regex,
    msgs: Regex,
}

fn patterns(r_angle: char) -> PickerPatterns {
    let ra = regex::escape(&r_angle.to_string());
    PickerPatterns {
        you: theme::re(&format!(r"(?i)(^|\s)you {ra} ")),
        claude: theme::re(&format!(r"(?i)(^|\s)claude {ra} ")),
        age: theme::re(r"(?i)(\d+ (?:min|h|d)|now)$"),
        msgs: theme::re(r"(?i)(\d+ msgs)"),
    }
}

/// Built once per glyph set: the ASCII and the Unicode right angle.
static PATTERNS: LazyLock<[PickerPatterns; 2]> = LazyLock::new(|| [patterns('>'), patterns('\u{203A}')]);

/// The free-path row's bullet: one border, an optional hover marker, the 3-cell mark, the
/// bullet, then its own label. Returns the bullet's char index.
fn bullet_at(chars: &[char], g: &Glyphs) -> Option<usize> {
    let mut i = 1;
    if chars.get(i) == Some(&theme::HOVER_OPEN) {
        i += 1;
    }
    let mark: String = chars.get(i..i + 3)?.iter().collect();
    if mark != "   " && mark != format!(" {} ", g.cursor) {
        return None;
    }
    let b = i + 3;
    let rest: String = chars.get(b + 1..)?.iter().take(13).collect();
    (chars.get(b) == Some(&g.bullet) && rest.to_lowercase() == " enter a path").then_some(b)
}

/// `Add-PickerColor`: speaker attributions parked behind a sentinel, the free-path bullet, cursor,
/// worktree glyph, trailing age and message counts.
pub fn add_picker_color(line: &str, enabled: bool, g: &Glyphs) -> String {
    if !enabled || line.is_empty() {
        return line.to_owned();
    }
    let rx = if g.r_angle == '>' { &PATTERNS[0] } else { &PATTERNS[1] };
    let mark_user = "\u{3}u\u{3}";
    let mark_claude = "\u{3}a\u{3}";
    let mut out = rx.you.replace_all(line, format!("${{1}}{mark_user}").as_str()).into_owned();
    out = rx.claude.replace_all(&out, format!("${{1}}{mark_claude}").as_str()).into_owned();
    let chars: Vec<char> = out.chars().collect();
    if let Some(b) = bullet_at(&chars, g) {
        let head: String = chars.get(..b).map_or_else(String::new, |c| c.iter().collect());
        let tail: String = chars.get(b + 1..).map_or_else(String::new, |c| c.iter().collect());
        out = format!("{head}{}{}{}{tail}", theme::MAGENTA, g.bullet, theme::RESET);
    }
    out = out.replace(g.cursor, &format!("{}{}{}", theme::BRIGHT_YELLOW, g.cursor, theme::RESET));
    out = out.replace(g.worktree, &format!("{}{}{}", theme::YELLOW, g.worktree, theme::RESET));
    out = rx
        .age
        .replace_all(&out, format!("{}${{1}}{}", theme::DIM, theme::RESET).as_str())
        .into_owned();
    out = rx
        .msgs
        .replace_all(&out, format!("{}${{1}}{}", theme::DIM, theme::RESET).as_str())
        .into_owned();
    out = out.replace(mark_user, &format!("{}you {} {}", theme::BRIGHT_CYAN, g.r_angle, theme::RESET));
    out.replace(mark_claude, &format!("{}claude {} {}", theme::ACCENT, g.r_angle, theme::RESET))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::glyphs;

    fn row(label: &str, tail: &str, age: &str, width: usize) -> ListRow<'static> {
        ListRow {
            label: label.to_owned().leak(),
            tail: tail.to_owned().leak(),
            age: age.to_owned().leak(),
            width,
            ..ListRow::default()
        }
    }

    #[test]
    fn list_row_fills_width_with_age_last() {
        let r = new_list_row(&row("alpha", r"C:\Users\sample\alpha", "4 min", 60));
        assert_eq!(display_width(&r), 60);
        assert!(r.starts_with("   alpha "));
        assert!(r.ends_with(" 4 min"));
        assert!(r.contains(r"C:\Users\sample\alpha"));
    }

    #[test]
    fn label_sized_to_the_reserve_is_not_clamped_early() {
        let l = "x".repeat(49);
        assert_eq!(new_list_row(&row(&l, "", "37 min", 60)), format!("   {l}  37 min"));
    }

    #[test]
    fn short_room_drops_the_tail_and_no_age_keeps_a_gutter() {
        let r = new_list_row(&row("name", r"C:\very\long\path\that\cannot\fit", "1 h", 20));
        assert!(!r.contains(r"C:\"));
        assert_eq!(
            display_width(&new_list_row(&row("name", r"C:\verylongpath\that\gets\cut", "", 20))),
            19
        );
        let cjk = new_list_row(&row(&"あ".repeat(10), "", "5 min", 40));
        assert_eq!(display_width(&cjk), 40);
        assert!(cjk.ends_with(" 5 min"));
    }

    #[test]
    fn path_tail_keeps_the_leaf() {
        let a = new_list_row(&ListRow {
            path_tail: true,
            ..row("p", r"C:\Users\sample\Desktop\Projects\workspace\alpha", "4 min", 46)
        });
        assert_eq!(display_width(&a), 46);
        assert!(a.contains(r"\alpha") && a.contains(r"C:\"));
    }

    #[test]
    fn hover_band_and_its_length() {
        let h = new_list_row(&ListRow {
            hover: true,
            ..row("a", "", "", 10)
        });
        assert!(h.starts_with(theme::HOVER_OPEN));
        assert_eq!(row_band_length(&h, 10), 10);
        assert_eq!(add_hover_span_at("abcdef", 1, 2).as_deref(), Some("a\u{4}bc\u{5}def"));
        assert_eq!(add_hover_span_at("ab", 1, 2), None);
    }

    #[test]
    fn picker_color_attributions_bullet_and_restore() {
        let g = glyphs(true);
        let line = "  you > hi  claude > ok  12 msgs  3 min";
        let p = add_picker_color(line, true, &g);
        assert_eq!(theme::remove_ansi_color(&p), line);
        assert!(p.contains(&format!("{}you > {}", theme::BRIGHT_CYAN, theme::RESET)));
        let free = "|   + enter a path";
        assert!(add_picker_color(free, true, &g).contains(&format!("{}+{}", theme::MAGENTA, theme::RESET)));
        let corner = "+-----+";
        assert_eq!(
            add_picker_color(corner, true, &g),
            corner,
            "box corners are never painted as bullets"
        );
    }
}
