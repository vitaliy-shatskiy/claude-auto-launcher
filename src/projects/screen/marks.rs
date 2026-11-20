//! `h` and `p` on the project screen: which projects the list hides, which it pins on top, and the
//! toggles saved to `hiddenProjects` and `pinnedProjects` (a pin and a hide exclude each other, D2).

use super::{HideChange, ProjectScreen};
use crate::projects::Project;
use crate::projects::paths::{folder_present, project_key};
use crate::projects::select::{RowKind, pinned_first};

/// One folder's keys, every spelling of it counted once.
fn keys_of(paths: &[String]) -> Vec<String> {
    paths.iter().map(|p| project_key(p)).collect()
}

/// The notice after a saved (or unsaved) change; `unpinned` = a hide that also dropped a pin.
fn change_notice(name: &str, change: HideChange, unpinned: bool, saved: Result<(), String>) -> String {
    let (verb, tail) = match change {
        HideChange::Show => ("shown", " again"),
        HideChange::Hide => ("hidden", " - v lists hidden projects"),
        HideChange::Pin => ("pinned", " on top"),
        HideChange::Unpin => ("unpinned", ""),
    };
    let verb = if unpinned {
        format!("{verb} and unpinned")
    } else {
        verb.to_owned()
    };
    match saved {
        Ok(()) => format!("{name} {verb}{tail}"),
        Err(e) => format!("{name} {verb} for this run only: {e}"),
    }
}

impl ProjectScreen<'_> {
    /// The listed projects: the pins first (never hidden), a hand-hidden one hidden like a
    /// rule-hidden one; the show-hidden view lists every one and marks the hidden.
    pub(super) fn rebuild_view(&mut self) {
        let (manual, pins) = self
            .hiding
            .map(|h| (keys_of(&h.paths.borrow()), h.pinned.borrow().clone()))
            .unwrap_or_default();
        let base: Vec<Project> = self
            .projects
            .iter()
            .map(|p| Project {
                hidden: p.hidden || manual.contains(&project_key(&p.path)),
                ..p.clone()
            })
            .collect();
        let listed = pinned_first(&base, &pins);
        self.marked = if self.show_hidden {
            listed.iter().filter(|p| p.hidden).map(|p| project_key(&p.path)).collect()
        } else {
            Vec::new()
        };
        self.dimmed = self.gone_pins(&pins);
        self.pin_keys = keys_of(&pins);
        self.view = listed
            .into_iter()
            .map(|p| Project {
                hidden: !self.show_hidden && p.hidden,
                ..p
            })
            .collect();
    }

    /// AC5: the pins the registry does not list whose folder is gone (checked without a network
    /// wait, once per run); a listed project's folder was there when the registry was built.
    fn gone_pins(&mut self, pins: &[String]) -> Vec<String> {
        let listed: Vec<String> = self.projects.iter().map(|p| project_key(&p.path)).collect();
        let mut gone = Vec::new();
        for pin in pins {
            let key = project_key(pin);
            if key.is_empty() || listed.contains(&key) {
                continue;
            }
            let drives = &mut self.drives;
            if *self.absent.entry(key.clone()).or_insert_with(|| !folder_present(pin, drives)) {
                gone.push(key);
            }
        }
        gone
    }

    /// The project row under the cursor: (kind, name, path).
    fn cursor_row(&self) -> Option<(RowKind, String, String)> {
        usize::try_from(self.cursor.index)
            .ok()
            .and_then(|i| self.rows.get(i))
            .map(|r| (r.kind, r.name.clone(), r.path.clone()))
    }

    /// `h`: a listed project joins `hiddenProjects` (and leaves `pinnedProjects`), a hand-hidden
    /// one leaves it; a rule-hidden one (headless or scratch) stays as it is unless pinned. Every
    /// change is saved at once.
    pub(super) fn toggle_hide(&mut self) {
        let Some(h) = self.hiding else { return };
        let Some((kind, name, path)) = self.cursor_row() else { return };
        if kind != RowKind::Project {
            "only a listed project can be hidden".clone_into(&mut self.notice);
            return;
        }
        let key = project_key(&path);
        let pinned = h.pinned.borrow().iter().any(|p| project_key(p) == key);
        let mut paths = h.paths.borrow_mut();
        let change = if paths.iter().any(|p| project_key(p) == key) {
            // Every spelling of this folder goes, as the save does on disk.
            paths.retain(|p| project_key(p) != key);
            HideChange::Show
        } else if !pinned && self.projects.iter().any(|p| p.hidden && project_key(&p.path) == key) {
            "hidden by rule (headless or scratch), not by h".clone_into(&mut self.notice);
            return;
        } else {
            paths.push(path.clone());
            h.pinned.borrow_mut().retain(|p| project_key(p) != key);
            HideChange::Hide
        };
        drop(paths);
        self.notice = change_notice(&name, change, pinned && change == HideChange::Hide, (h.save)(&path, change));
    }

    /// `p`: a listed project joins `pinnedProjects` (and leaves `hiddenProjects`), a pinned one
    /// leaves it; the cursor follows the row. Saved at once.
    pub(super) fn toggle_pin(&mut self) {
        let Some(h) = self.hiding else { return };
        let Some((kind, name, path)) = self.cursor_row() else { return };
        if kind != RowKind::Project {
            "only a listed project can be pinned".clone_into(&mut self.notice);
            return;
        }
        let key = project_key(&path);
        let mut pins = h.pinned.borrow_mut();
        let change = if pins.iter().any(|p| project_key(p) == key) {
            pins.retain(|p| project_key(p) != key);
            HideChange::Unpin
        } else {
            pins.push(path.clone());
            h.paths.borrow_mut().retain(|p| project_key(p) != key);
            HideChange::Pin
        };
        drop(pins);
        self.follow = Some(key);
        self.notice = change_notice(&name, change, false, (h.save)(&path, change));
    }

    /// After a `p` the cursor stays on the project it moved, not on the row index.
    pub(super) fn follow_moved(&mut self) {
        let Some(key) = self.follow.take() else { return };
        if let Some(i) = self
            .rows
            .iter()
            .position(|r| r.kind == RowKind::Project && project_key(&r.path) == key)
        {
            self.cursor.index = i32::try_from(i).unwrap_or(0);
        }
    }
}
