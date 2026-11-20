//! Full summaries landing on rows the picker first showed quick: replaced in place, so no row moves
//! and the cursor stays where it was.

use super::PickerScreen;
use crate::sessions::listing::Session;

impl PickerScreen<'_> {
    /// Takes every batch the worker finished; `true` when a shown row changed (one redraw).
    pub(super) fn take_landed(&mut self) -> bool {
        let batches: Vec<Vec<Session>> = match &self.landed {
            Some(rx) => rx.try_iter().collect(),
            None => return false,
        };
        let mut changed = false;
        for full in batches.iter().flatten() {
            for rows in [
                &mut self.project.sessions,
                &mut self.all.sessions,
                &mut self.none.sessions,
                &mut self.first_page,
            ] {
                changed |= replace(rows, full);
            }
        }
        changed
    }
}

/// The row of the same file takes the full summary; the account tag depends on who launched, so
/// the shown row keeps its own.
fn replace(rows: &mut [Session], full: &Session) -> bool {
    let mut hit = false;
    for r in rows.iter_mut().filter(|r| r.path == full.path && r.session_id == full.session_id) {
        let account = std::mem::take(&mut r.account);
        *r = Session { account, ..full.clone() };
        hit = true;
    }
    hit
}
