//! `h` on the project screen: which projects the list hides, and the hide toggle saved to `hiddenProjects`.

use super::{HideChange, ProjectScreen};
use crate::projects::Project;
use crate::projects::paths::project_key;
use crate::projects::select::RowKind;

impl ProjectScreen<'_> {
    /// The listed projects: a hand-hidden one is hidden like a rule-hidden one; the show-hidden
    /// view lists every one and marks the hidden.
    pub(super) fn rebuild_view(&mut self) {
        let manual: Vec<String> = self
            .hiding
            .map(|h| h.paths.borrow().iter().map(|p| project_key(p)).collect())
            .unwrap_or_default();
        let hidden = |p: &Project| p.hidden || manual.contains(&project_key(&p.path));
        self.marked = if self.show_hidden {
            self.projects.iter().filter(|p| hidden(p)).map(|p| project_key(&p.path)).collect()
        } else {
            Vec::new()
        };
        self.view = self
            .projects
            .iter()
            .map(|p| Project {
                hidden: !self.show_hidden && hidden(p),
                ..p.clone()
            })
            .collect();
    }

    /// `h`: a listed project joins `hiddenProjects`, a hand-hidden one leaves it; a rule-hidden one
    /// (headless or scratch) stays as it is. Every change is saved at once.
    pub(super) fn toggle_hide(&mut self) {
        let Some(h) = self.hiding else { return };
        let Some((kind, name, path)) = usize::try_from(self.cursor.index)
            .ok()
            .and_then(|i| self.rows.get(i))
            .map(|r| (r.kind, r.name.clone(), r.path.clone()))
        else {
            return;
        };
        if kind != RowKind::Project {
            "only a listed project can be hidden".clone_into(&mut self.notice);
            return;
        }
        let key = project_key(&path);
        let mut paths = h.paths.borrow_mut();
        let change = if paths.iter().any(|p| project_key(p) == key) {
            // Every spelling of this folder goes, as the save does on disk.
            paths.retain(|p| project_key(p) != key);
            HideChange::Show
        } else if self.projects.iter().any(|p| p.hidden && project_key(&p.path) == key) {
            "hidden by rule (headless or scratch), not by h".clone_into(&mut self.notice);
            return;
        } else {
            paths.push(path.clone());
            HideChange::Hide
        };
        self.notice = match ((h.save)(&path, change), change) {
            (Ok(()), HideChange::Show) => format!("{name} shown again"),
            (Ok(()), HideChange::Hide) => format!("{name} hidden - v lists hidden projects"),
            (Err(e), HideChange::Show) => format!("{name} shown for this run only: {e}"),
            (Err(e), HideChange::Hide) => format!("{name} hidden for this run only: {e}"),
        };
    }
}
