//! The picker's preview card (AC25): title, `DD.MM HH:MM · N msgs · size`, a rule, then the
//! chat (`chat.rs`). Exports `Preview` and `size_text`.

use super::super::listing::Session;
use super::PickerScope;
use super::Zone;
use super::chat::{ChatView, chat_lines};
use super::text::n0;
use crate::layout::limit_line;
use crate::theme::{DIM_CLOSE, DIM_OPEN, Glyphs, TONE_BOLD, TONE_CLOSE, strip_ctl};
/// The selected session's preview: the detail pane (wide) or the block under the list (narrow).
pub(super) struct Preview<'a> {
    pub(super) s: &'a Session,
    pub(super) g: &'a Glyphs,
    pub(super) ascii: bool,
    pub(super) zone: Zone,
}

impl Preview<'_> {
    /// Worktree, account and (outside one project's scope) project: where the session lives.
    fn place(&self, scope: PickerScope) -> String {
        let mut place = if scope == PickerScope::Project {
            String::new()
        } else {
            strip_ctl(&self.s.project)
        };
        if !self.s.worktree.is_empty() {
            place = format!("{place} {} {}", self.g.worktree, strip_ctl(&self.s.worktree));
        }
        if !self.s.account.is_empty() {
            place = format!("[{}] {place}", strip_ctl(&self.s.account));
        }
        place.trim().to_owned()
    }

    /// `DD.MM HH:MM · N msgs · size`, then the place when there is one: dim, the two numbers in
    /// normal weight.
    fn meta(&self, scope: PickerScope) -> String {
        let dot = if self.ascii { '-' } else { '\u{B7}' };
        let sep = format!("{DIM_OPEN} {dot} {DIM_CLOSE}");
        let mut parts = vec![
            format!("{DIM_OPEN}{}{DIM_CLOSE}", self.zone.stamp(self.s.modified_ms)),
            format!("{} msgs", n0_int(self.s.message_count)),
            size_text(self.s.size_bytes),
        ];
        let place = self.place(scope);
        if !place.is_empty() {
            parts.push(format!("{DIM_OPEN}{place}{DIM_CLOSE}"));
        }
        parts.join(&sep)
    }

    /// AC25: title (bold), meta, a dim rule, then the chat. Fewer than six rows fold title and
    /// meta into one line.
    pub(super) fn card(&self, width: usize, rows: usize, scope: PickerScope, chat: ChatView<'_>) -> Vec<String> {
        let h = self.g.h;
        let title = limit_line(&strip_ctl(&self.s.title), width, self.ascii);
        let meta = self.meta(scope);
        let mut out = if rows >= 6 {
            vec![format!("{TONE_BOLD}{title}{TONE_CLOSE}"), limit_line(&meta, width, self.ascii)]
        } else {
            vec![limit_line(
                &format!("{TONE_BOLD}{title}{TONE_CLOSE}  {h}  {meta}"),
                width,
                self.ascii,
            )]
        };
        if rows > 2 {
            out.push(format!("{DIM_OPEN}{}{DIM_CLOSE}", h.to_string().repeat(width)));
        }
        let room = rows.saturating_sub(out.len());
        out.extend(chat_lines(chat, width, room, self.ascii, self.zone));
        out
    }
}
/// A whole count, thousands grouped.
fn n0_int(n: i64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "a message count is far inside f64's exact range")]
    n0(n as f64)
}

/// `1,234 KB` under a megabyte, `1.6 MB` from there.
pub(super) fn size_text(bytes: u64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "a file size")]
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        format!("{} KB", n0(kb))
    } else {
        format!("{:.1} MB", kb / 1024.0)
    }
}
