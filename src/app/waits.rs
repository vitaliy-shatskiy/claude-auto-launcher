//! What stands between Enter and the launch: the wait for the update check (and the wait of `u`
//! before maintenance) and the held-session warning.

use std::cell::RefCell;

use crate::background::Left;
use crate::frame::{self, FooterLight, Frame, Hint, RowMap, Style};
use crate::input::{Event, vk};
use crate::layout::split_text_lines;
use crate::maintenance;
use crate::sessions;
use crate::theme;
use crate::timing::Timings;
use crate::ui::screen_loop::{Cursor, Flow, Screen};
use crate::ui::{Term, UiStop};

use super::screens::status;

/// Claude never starts mid-update: a launch waits for the check (`WinHTTP` allows 5 s per phase,
/// four phases; `claude update` has no limit). A quit leaves a check that is only talking to the
/// network and waits only for a running install. Either wait is `wait:update`; `left` (Esc,
/// Ctrl+C) ends it unread.
pub(super) fn wait_for_update(
    launching: bool,
    update: &mut maintenance::UpdateWatch,
    tm: &Timings,
    on_slow: impl FnOnce(),
    left: impl FnMut() -> bool,
) -> Result<(), Left> {
    let waited = if launching {
        update.finish(on_slow, left)
    } else {
        update.quit_or_leave(on_slow, left)
    }?;
    if let Some(w) = waited {
        tm.add(&["wait", "update"], None, w.start, w.end, None);
    }
    Ok(())
}

/// `wait_for_update` on the console: the status line once it outlasts `SLOW`, then Esc or Ctrl+C
/// leaves it (the raw console reads Ctrl+C as a key, so nothing else could).
pub(super) fn wait_on_screen(
    launching: bool,
    update: &mut maintenance::UpdateWatch,
    tm: &Timings,
    term: &mut dyn Term,
    preview: bool,
) -> Result<(), Left> {
    let term = RefCell::new(term);
    wait_for_update(
        launching,
        update,
        tm,
        || status(&mut **term.borrow_mut(), preview, UPDATE_STATUS)(),
        || leave_key(&mut **term.borrow_mut()),
    )
}

/// The wait's status line: Esc leaves even a running install, which finishes on its own.
const UPDATE_STATUS: &str = "checking for a Claude Code update... (esc: quit; claude update keeps running in the background)";

/// One poll of the console: Esc or Ctrl+C. Other input during the wait is dropped.
fn leave_key(term: &mut dyn Term) -> bool {
    let size = term.size();
    matches!(term.wait(size), Ok(Event::Key(k)) if k.is(vk::ESCAPE) || k.is_ctrl(vk::C))
}

/// `u`: maintenance opens only once the background check is done, so its update keys never run
/// beside the check's own `claude update`. Leaving the wait goes back to the launch screen; the
/// check runs on and the next Enter or `u` waits for it again.
pub(super) fn open_maintenance(
    update: &RefCell<maintenance::UpdateWatch>,
    tm: &Timings,
    term: &mut dyn Term,
    preview: bool,
    open: impl FnOnce(&mut dyn Term),
) {
    if wait_on_screen(true, &mut update.borrow_mut(), tm, term, preview).is_ok() {
        open(term);
    }
}

/// `Confirm-HeldSession`: Enter or `[enter] go on` = go on, Esc/Ctrl+C or `[esc] back` = back;
/// nothing held = go on.
pub(super) fn confirm_held(term: &mut dyn Term, holder: Option<&str>, style: Style) -> Result<bool, UiStop> {
    let Some(h) = holder else { return Ok(true) };
    let mut screen = HeldConfirm {
        warning: sessions::held_warning(h),
        style,
        cursor: Cursor::at(0),
    };
    Ok(crate::ui::screen_loop::run(&mut screen, term)?.unwrap_or(false))
}

/// The held-session warning: the text, then the two buttons as its footer.
struct HeldConfirm {
    warning: String,
    style: Style,
    cursor: Cursor,
}

impl Screen for HeldConfirm {
    type Output = bool;

    fn name(&self) -> &'static str {
        "held"
    }

    fn cursor(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    /// The buttons placed first, the warning wrapped into what is left.
    fn draw(&mut self, (width, height): (usize, usize)) -> Frame {
        let g = theme::glyphs(self.style.ascii);
        let fw = frame::frame_width(width);
        let hints = [Hint::key("enter", "go on", "Enter"), Hint::key("esc", "back", "Escape")];
        let footer = frame::new_hint_footer(&hints, &g, fw, !self.style.color);
        // 2 = the blank lines above and below the warning.
        let room = height.saturating_sub(footer.lines.len() + 2).max(1);
        let mut lines = vec![String::new()];
        lines.extend(
            split_text_lines(&self.warning, fw.saturating_sub(2).max(1), room, self.style.ascii)
                .into_iter()
                .map(|l| format!("  {l}")),
        );
        lines.push(String::new());
        frame::pad_to_bottom(&mut lines, height, footer.lines.len());
        let light = FooterLight {
            hover: frame::footer_hover(&footer, self.cursor.hover),
            selected: Vec::new(),
        };
        let mut map = RowMap::default();
        let lines = frame::complete_frame(&lines, &footer, fw, self.style, &|l: &str, _| l.to_owned(), &light, &mut map);
        Frame { lines, map }
    }

    fn enter(&mut self) -> Option<Flow<bool>> {
        Some(Flow::Done(Some(true)))
    }
}
