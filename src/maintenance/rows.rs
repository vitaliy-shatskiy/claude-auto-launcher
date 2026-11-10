//! The launch-rows editor behind the maintenance screen's `l`: which launch rows show and in what
//! order, saved as config `rows`. w/s cursor, shift+w/s move, space show/hide, enter save, esc cancel.

use crate::frame::{
    FooterLight, Frame, Hint, MIN_HEIGHT, MIN_WIDTH, RowMap, Style, complete_frame, footer_hover, frame_width, new_hint_footer,
    too_small_frame,
};
use crate::input::{KeyPress, is_hotkey_exact, vk};
use crate::launch::{CONFIG_ROWS, RowName};
use crate::layout::new_box;
use crate::theme::glyphs;
use crate::ui::Term;
use crate::ui::screen_loop::{Cursor, Flow, Screen};

/// Every config row with its shown flag, in the order being edited.
pub(crate) struct RowsEditor {
    pub(crate) entries: Vec<(RowName, bool)>,
    cursor: Cursor,
    style: Style,
    notice: &'static str,
}

impl std::fmt::Debug for RowsEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowsEditor").field("entries", &self.entries).finish_non_exhaustive()
    }
}

impl RowsEditor {
    /// The shown rows first in their order, the hidden ones after them in the default order.
    pub(crate) fn new(current: Option<&[RowName]>, style: Style) -> Self {
        let shown = current.unwrap_or(&CONFIG_ROWS);
        let mut entries: Vec<(RowName, bool)> = shown.iter().map(|r| (*r, true)).collect();
        entries.extend(CONFIG_ROWS.iter().filter(|r| !shown.contains(r)).map(|r| (*r, false)));
        Self {
            entries,
            cursor: Cursor::at(0),
            style,
            notice: "",
        }
    }

    /// The `rows` value to save: the shown rows in order.
    pub(crate) fn rows(&self) -> Vec<RowName> {
        self.entries.iter().filter(|(_, on)| *on).map(|(r, _)| *r).collect()
    }

    fn index(&self) -> usize {
        usize::try_from(self.cursor.index).unwrap_or(0)
    }

    fn shift(&mut self, delta: isize) {
        let i = self.index();
        let Some(j) = i.checked_add_signed(delta).filter(|j| *j < self.entries.len()) else {
            return;
        };
        self.entries.swap(i, j);
        self.cursor.index = i32::try_from(j).unwrap_or(0);
    }

    fn toggle(&mut self) {
        let i = self.index();
        if let Some((row, on)) = self.entries.get_mut(i) {
            if *row == RowName::Account {
                self.notice = "account is always shown";
            } else {
                *on = !*on;
            }
        }
    }
}

impl Screen for RowsEditor {
    type Output = Vec<RowName>;

    fn name(&self) -> &'static str {
        "rows"
    }

    fn cursor(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    fn draw(&mut self, size: (usize, usize)) -> Frame {
        let (width, height) = size;
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            return Frame {
                lines: too_small_frame(width, height, self.style.ascii),
                map: RowMap::default(),
            };
        }
        let g = glyphs(self.style.ascii);
        let fw = frame_width(width);
        let mut body: Vec<String> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, (row, on))| {
                let mark = if i == self.index() { g.cursor } else { ' ' };
                let state = if *on { g.on } else { g.off };
                format!(
                    " {mark} {state} {:<12}{}",
                    row.as_str().to_lowercase(),
                    if *on { "shown" } else { "hidden" }
                )
            })
            .collect();
        body.push(String::new());
        body.push(format!(
            "  {}",
            if self.notice.is_empty() {
                "saved to the config; the launch screen shows it on return"
            } else {
                self.notice
            }
        ));
        let footer = new_hint_footer(
            &[
                Hint::plain("w/s", "row"),
                Hint::plain("W/S", "move"),
                Hint::plain("space", "show/hide"),
                Hint::key("enter", "save", "Enter"),
                Hint::key("esc", "cancel", "Escape"),
            ],
            &g,
            fw,
            !self.style.color,
        );
        let light = FooterLight {
            hover: footer_hover(&footer, self.cursor.hover),
            selected: Vec::new(),
        };
        let lines = new_box(&body, fw.min(100), "launch rows", &g, self.style.ascii);
        let mut map = RowMap::default();
        let painter = |l: &str, e: bool| crate::list::add_picker_color(l, e, &g);
        let lines = complete_frame(&lines, &footer, fw, self.style, &painter, &light, &mut map);
        Frame { lines, map }
    }

    fn rows(&self) -> Option<i32> {
        Some(i32::try_from(self.entries.len()).unwrap_or(0))
    }

    fn enter(&mut self) -> Option<Flow<Vec<RowName>>> {
        Some(Flow::Done(Some(self.rows())))
    }

    /// Shift+w/s (or shift+arrows) move the row; space shows or hides it. Ahead of the loop's own
    /// w/s, which would only move the cursor.
    fn on_key(&mut self, key: &KeyPress, _term: &mut dyn Term) -> bool {
        self.notice = "";
        if is_hotkey_exact(key, 'W') || (key.shift && key.is(vk::UP)) {
            self.shift(-1);
            return true;
        }
        if is_hotkey_exact(key, 'S') || (key.shift && key.is(vk::DOWN)) {
            self.shift(1);
            return true;
        }
        if key.ch == ' ' || key.is(vk::SPACE) {
            self.toggle();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{PreviewTerm, parse_preview_keys};

    fn edit(current: Option<&[RowName]>, keys: Vec<KeyPress>) -> Option<Vec<RowName>> {
        let mut e = RowsEditor::new(current, Style { color: false, ascii: true });
        let mut t = PreviewTerm::new((100, 30), keys.into(), true);
        crate::ui::screen_loop::run(&mut e, &mut t).unwrap()
    }

    fn shifted(c: char) -> KeyPress {
        KeyPress {
            shift: true,
            ..KeyPress::char(c)
        }
    }

    #[test]
    fn space_hides_a_row_and_enter_saves_the_shown_ones() {
        let keys: Vec<KeyPress> = parse_preview_keys("s,s,s,s,s,s,Spacebar,Enter").unwrap().into();
        let out = edit(None, keys).unwrap();
        assert_eq!(
            out,
            [
                RowName::Account,
                RowName::Remote,
                RowName::Model,
                RowName::Effort,
                RowName::Advisor,
                RowName::Permission
            ]
        );
    }

    #[test]
    fn shift_s_moves_a_row_down_and_hidden_rows_follow_the_shown_ones() {
        let mut keys: Vec<KeyPress> = parse_preview_keys("s").unwrap().into();
        keys.push(shifted('S'));
        keys.extend(parse_preview_keys("Enter").unwrap());
        let out = edit(Some(&[RowName::Account, RowName::Model, RowName::Effort]), keys).unwrap();
        assert_eq!(out, [RowName::Account, RowName::Effort, RowName::Model]);
        let e = RowsEditor::new(Some(&[RowName::Mode]), Style { color: false, ascii: true });
        assert_eq!(e.entries.first(), Some(&(RowName::Mode, true)));
        assert!(e.entries.iter().skip(1).all(|(_, on)| !on), "the rest listed hidden");
    }

    #[test]
    fn account_cannot_be_hidden_and_escape_cancels() {
        let out = edit(None, parse_preview_keys("Spacebar,Enter").unwrap().into()).unwrap();
        assert_eq!(out, CONFIG_ROWS, "space on account changes nothing");
        assert_eq!(edit(None, parse_preview_keys("Spacebar,Escape").unwrap().into()), None);
    }
}
