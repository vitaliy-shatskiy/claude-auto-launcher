//! The `/` filter box of the project screen: the keys it owns, where it parks the cursor, its log records.

use serde_json::{Map, Value, json};

use super::ProjectScreen;
use crate::input::{KeyPress, vk};
use crate::projects::select::{RowKind, project_rows};
use crate::ui::Term;

impl ProjectScreen<'_> {
    /// R16: a filter parks the cursor on its first match - row 0 when the cwd's project matches,
    /// else the first list row; no filter, or no match, is row 0.
    fn park(&self) -> i32 {
        if self.filter.is_empty() {
            return 0;
        }
        let rows = project_rows(&self.view, &self.filter, &self.cwd);
        if rows.first().is_some_and(|r| r.matched) {
            0
        } else {
            i32::from(rows.iter().any(|r| r.kind == RowKind::Project))
        }
    }

    /// No row matches the filter and the cursor is on the current directory: Enter would run it.
    /// The free-path row still opens the navigator.
    fn no_match(&self) -> bool {
        let on_cwd = usize::try_from(self.cursor.index)
            .ok()
            .and_then(|i| self.rows.get(i))
            .is_some_and(|r| r.kind == RowKind::Cwd);
        on_cwd && !self.filter.trim().is_empty() && !self.rows.iter().any(|r| r.kind == RowKind::Project || r.matched)
    }

    /// The filter box owns every key while it is open, but the arrows (they move the cursor and
    /// step the field) and Enter (D4: it runs the highlighted row, handed back as `false`). Its
    /// ways of closing are logged with the LENGTH of the filter, never its text.
    pub(super) fn type_filter(&mut self, key: &KeyPress, term: &dyn Term) -> bool {
        if [vk::UP, vk::DOWN, vk::LEFT, vk::RIGHT].iter().any(|k| key.is(*k)) {
            return false;
        }
        if key.is(vk::ENTER) {
            if self.no_match() {
                "no project matches".clone_into(&mut self.notice);
                return true;
            }
            self.cursor.typing = false;
            self.log(
                term,
                "Enter",
                &[("filter", json!("run")), ("filterLength", json!(self.filter.chars().count()))],
            );
            return false;
        }
        if key.is(vk::ESCAPE) || key.is_ctrl(vk::C) {
            self.log(
                term,
                "Escape",
                &[("filter", json!("clear")), ("filterLength", json!(self.filter.chars().count()))],
            );
            self.cursor.typing = false;
            self.filter.clear();
            self.cursor.index = 0;
            return true;
        }
        if key.is(vk::BACKSPACE) {
            self.filter.pop();
            self.cursor.index = self.park();
            return true;
        }
        // Any printable char: a Windows folder name may hold `~` (8.3 names), brackets, `+`, `@`.
        if key.ch != '\0' && !key.ch.is_control() {
            self.filter.push(key.ch);
            self.cursor.index = self.park();
        }
        true
    }

    fn log(&self, term: &dyn Term, key: &str, extra: &[(&str, Value)]) {
        let mut d = Map::new();
        d.insert("screen".into(), json!("project"));
        d.insert("key".into(), json!(key));
        d.insert("index".into(), json!(self.cursor.index));
        for (k, v) in extra {
            d.insert((*k).into(), v.clone());
        }
        term.logger().write("key", d);
    }
}
