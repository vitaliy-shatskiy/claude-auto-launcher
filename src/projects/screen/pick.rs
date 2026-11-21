//! The project screen's picks: a list row into a choice, the path navigator's keys and picks, the create question.

use super::ProjectScreen;
use crate::input::{KeyPress, vk};
use crate::projects::ProjectChoice;
use crate::projects::navigator::{NavStep, Navigator};
use crate::projects::paths::folder_present;
use crate::projects::select::{RowKind, slugs_for_path};
use crate::ui::screen_loop::Flow;

impl ProjectScreen<'_> {
    /// The row under the cursor into a result; every kind is checked for existence without a
    /// network wait, so a row whose directory vanished is refused with a notice. The free-path
    /// row opens the navigator.
    pub(super) fn commit(&mut self, how: &str) -> Flow<ProjectChoice> {
        let Some(row) = usize::try_from(self.cursor.index).ok().and_then(|i| self.rows.get(i)) else {
            return Flow::Continue;
        };
        if row.kind == RowKind::Path {
            self.list_index = self.cursor.index;
            self.cursor.index = 0;
            let mut nav = Navigator::new(self.lister, self.limit);
            nav.preview = self.preview;
            self.nav = Some(nav);
            return Flow::Continue;
        }
        let target = row.path.clone();
        let slugs = if row.kind == RowKind::Cwd {
            slugs_for_path(&self.projects, &target)
        } else {
            row.slugs.clone()
        };
        if !folder_present(&target, &mut self.drives) {
            "path not found".clone_into(&mut self.notice);
            return Flow::Continue;
        }
        Flow::Done(Some(choice(target, &self.action, how, slugs)))
    }

    /// Enter or a second click in the navigator: a picked folder ends the screen (the records
    /// carry the LENGTH of the path only, `log_fields`).
    pub(super) fn nav_enter(&mut self, how: &str) -> Flow<ProjectChoice> {
        let index = usize::try_from(self.cursor.index).unwrap_or(0);
        let Some(nav) = self.nav.as_mut() else { return Flow::Continue };
        let NavStep::Pick(path) = nav.enter(index) else {
            return Flow::Continue;
        };
        let slugs = slugs_for_path(&self.projects, &path);
        Flow::Done(Some(choice(path, &self.action, how, slugs)))
    }

    /// The navigator owns the typed characters and Backspace; the arrows, Enter, Tab and Esc
    /// go to the loop, which hands them to this screen's handlers.
    pub(super) fn nav_key(&mut self, key: &KeyPress) -> bool {
        let Some(nav) = self.nav.as_mut() else { return false };
        nav.notice.clear();
        if key.is(vk::BACKSPACE) {
            nav.backspace(key.ctrl);
            return true;
        }
        let passed = [vk::UP, vk::DOWN, vk::LEFT, vk::RIGHT, vk::ENTER, vk::TAB, vk::ESCAPE];
        if passed.iter().any(|k| key.is(*k)) || key.is_ctrl(vk::C) {
            return false;
        }
        // Ctrl gives a control character; AltGr (Ctrl+Alt) still types `\` on many layouts.
        if key.ch != '\0' && !key.ch.is_control() {
            nav.type_char(key.ch);
        }
        true
    }

    /// The selected project's sessions once its detail is in (the launch page's sessions panel).
    pub(crate) fn sessions(&self) -> &[crate::projects::detail::SessionLine] {
        let at = usize::try_from(self.cursor.index).ok().and_then(|i| self.rows.get(i));
        match (at, self.detail.as_ref()) {
            (Some(r), Some(d)) if r.kind != RowKind::Path && d.key == crate::projects::paths::project_key(&r.path) => &d.sessions,
            _ => &[],
        }
    }

    /// Whether the list is in a state of its own (a typed filter, the navigator) that Tab and Esc
    /// belong to.
    pub(crate) fn busy(&self) -> bool {
        self.cursor.typing || self.nav.is_some()
    }

    /// The selected project as a resume pick: what the launch page's sessions panel continues.
    pub(crate) fn resume_here(&mut self, how: &str) -> Option<ProjectChoice> {
        let keep = std::mem::replace(&mut self.action, "resume".to_owned());
        let flow = self.commit(how);
        self.action = keep;
        match flow {
            Flow::Done(Some(c)) => Some(c),
            Flow::Done(None) | Flow::Continue => None,
        }
    }

    /// The create question takes a deliberate yes (Enter or its footer button); a click or a
    /// cursor move drops it. `true` = there was one.
    pub(super) fn drop_question(&mut self) -> bool {
        self.nav.as_mut().is_some_and(|n| n.confirm.take().is_some())
    }
}

fn choice(path: String, action: &str, how: &str, slugs: Vec<String>) -> ProjectChoice {
    ProjectChoice {
        path,
        action: action.to_owned(),
        slug: slugs.first().cloned().unwrap_or_default(),
        slugs,
        how: how.to_owned(),
    }
}
