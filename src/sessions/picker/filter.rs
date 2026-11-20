//! The picker's filter box: the keys it owns while open and the two closes it logs.

use serde_json::{Map, Value, json};

use super::PickerScreen;
use crate::input::{KeyPress, vk};
use crate::sessions::frame::PickerScope;
use crate::ui::Term;

impl PickerScreen<'_> {
    fn log_typing(&mut self, term: &dyn Term, fields: &[(&str, Value)]) {
        let mut d = Map::new();
        d.insert("screen".into(), json!("picker"));
        for (k, v) in fields {
            d.insert((*k).into(), v.clone());
        }
        d.insert("scope".into(), json!(scope_name(self.scope)));
        term.logger().write("key", d);
    }

    /// The filter box owns every key while open; only its two closes are logged, never the text.
    pub(super) fn type_filter(&mut self, key: &KeyPress, term: &dyn Term) {
        if key.is(vk::ENTER) {
            self.cursor.typing = false;
            let n = self.filter.chars().count();
            self.log_typing(
                term,
                &[("key", json!("Enter")), ("filter", json!("close")), ("filterLength", json!(n))],
            );
        } else if key.is(vk::ESCAPE) || key.is_ctrl(vk::C) {
            let n = self.filter.chars().count();
            let index = self.cursor.index;
            self.log_typing(
                term,
                &[
                    ("key", json!("Escape")),
                    ("filter", json!("clear")),
                    ("filterLength", json!(n)),
                    ("index", json!(index)),
                ],
            );
            self.cursor.typing = false;
            self.filter.clear();
            self.cursor.index = 0;
        } else if key.is(vk::BACKSPACE) {
            self.filter.pop();
        } else if key.ch.is_alphanumeric() || key.ch == ' ' || key.ch == '-' {
            self.filter.push(key.ch);
            self.cursor.index = 0;
        }
    }
}

pub(super) fn scope_name(s: PickerScope) -> &'static str {
    match s {
        PickerScope::None => "none",
        PickerScope::Project => "project",
        PickerScope::All => "all",
    }
}
