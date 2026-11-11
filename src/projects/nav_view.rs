//! The path navigator's frame: the field, its status line, the rows, the action field, and a
//! footer naming the navigator's keys. Pure.

use super::navigator::{NavRow, Navigator};
use super::view::{action_hit, action_light, action_radio, clean_text};
use crate::frame::{self, FooterLight, Frame, Hint, ListMap, MIN_HEIGHT, MIN_WIDTH, RowMap, Style, new_hint_footer};
use crate::layout::{limit_cells_right, new_box, viewport};
use crate::list::{ListRow, add_picker_color, new_list_row};
use crate::theme::{self, glyphs};

/// Everything one navigator frame is drawn from.
#[derive(Debug, Clone)]
pub(crate) struct NavView<'a> {
    pub nav: &'a Navigator,
    pub index: usize,
    pub width: usize,
    pub height: usize,
    pub style: Style,
    pub action: &'a str,
    /// Footer button under the mouse (-1 none).
    pub hover: i32,
    /// Row under the mouse (-1 none); paints only.
    pub hover_row: i32,
    pub hover_value: &'a str,
}

fn hints(confirm: bool) -> Vec<Hint> {
    if confirm {
        return vec![Hint::key("enter", "yes, create it", "Enter"), Hint::key("esc", "no", "Escape")];
    }
    vec![
        Hint::plain("up/down", "move").kept(),
        Hint::key("enter", "open", "Enter"),
        Hint::plain("right", "open"),
        Hint::key("tab", "complete", "Tab"),
        Hint::key("bksp", "up", "Backspace"),
        Hint::plain("ctrl+bksp", "drop segment").kept(),
        Hint::key("esc", "cancel", "Escape"),
    ]
}

fn label(r: &NavRow) -> String {
    match r {
        NavRow::Use(t) => format!("use this folder: {t}"),
        NavRow::Up => "..".into(),
        NavRow::Folder(n) => format!("{n}\\"),
    }
}

/// The navigator in the project screen's box: field, status, rows, action row.
pub(crate) fn navigator_frame(v: &NavView<'_>) -> Frame {
    if v.width < MIN_WIDTH || v.height < MIN_HEIGHT {
        return Frame {
            lines: frame::too_small_frame(v.width, v.height, v.style.ascii),
            map: RowMap {
                list: Some(ListMap::default()),
                ..RowMap::default()
            },
        };
    }
    let g = glyphs(v.style.ascii);
    let fw = frame::frame_width(v.width);
    let inner = fw.saturating_sub(2);
    let footer = new_hint_footer(&hints(v.nav.confirm.is_some()), &g, fw, !v.style.color);
    let rows = &v.nav.rows;
    let list_lines = v.height.saturating_sub(7 + footer.lines.len()).max(3);
    let index = v.index.min(rows.len().saturating_sub(1));
    let (start, visible) = viewport(rows.len(), index, list_lines);
    let field = format!(
        "   path  {}_",
        limit_cells_right(&theme::strip_ctl(&v.nav.text), inner.saturating_sub(11))
    );
    let mut body = vec![field, format!("   {}", clean_text(&v.nav.status()))];
    let hover_row = usize::try_from(v.hover_row).ok();
    for (i, r) in rows.iter().enumerate().skip(start).take(visible) {
        let mark = if i == index { format!(" {} ", g.cursor) } else { "   ".to_owned() };
        body.push(new_list_row(&ListRow {
            mark: &mark,
            label: &label(r),
            width: inner,
            ascii: v.style.ascii,
            hover: hover_row == Some(i),
            ..ListRow::default()
        }));
    }
    body.resize(2 + list_lines, String::new());
    let radio = action_radio(v.action, v.hover_value, inner, &g);
    let action_index = body.len();
    body.push(radio.text.clone());
    let lines = new_box(&body, fw, &format!("project {} enter a path", g.h), &g, v.style.ascii);
    let first_row_y = lines.len() - body.len() - 1;
    let y = |n: usize| i32::try_from(first_row_y + n).unwrap_or(i32::MAX);
    let mut map = RowMap {
        list: Some(ListMap {
            first_row_y: y(2),
            row_count: i32::try_from(visible.min(rows.len())).unwrap_or(0),
            start: i32::try_from(start).unwrap_or(0),
            row_ys: Vec::new(),
        }),
        action: Some(action_hit(&radio, y(action_index))),
        ..RowMap::default()
    };
    let light = FooterLight {
        hover: frame::footer_hover(&footer, v.hover),
        selected: action_light(v.action),
    };
    let painter = |l: &str, enabled: bool| add_picker_color(l, enabled, &g);
    let lines = frame::complete_frame(&lines, &footer, fw, v.style, &painter, &light, &mut map);
    Frame { lines, map }
}
