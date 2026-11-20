//! The picker's preview card (AC25): title, `DD.MM HH:MM · N msgs · size`, a rule, then the
//! first prompt, the last prompt and the reply to it. Exports `Preview` and `size_text`.

use super::super::listing::Session;
use super::PickerScope;
use super::text::{local_stamp, n0};
use crate::layout::{limit_line, split_text_lines};
use crate::theme::{self, Glyphs, strip_ctl};
/// The selected session's preview: the detail pane (wide) or the block under the list (narrow).
pub(super) struct Preview<'a> {
    pub(super) s: &'a Session,
    pub(super) g: &'a Glyphs,
    pub(super) ascii: bool,
    pub(super) offset: i32,
}

/// The card's three texts under the rule, labelled to one width.
const CARD_LABELS: [&str; 3] = ["first", "last ", "reply"];

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

    /// `DD.MM HH:MM · N msgs · size`, then the place when there is one.
    fn meta(&self, scope: PickerScope) -> String {
        let dot = if self.ascii { '-' } else { '\u{B7}' };
        let mut parts = vec![
            local_stamp(self.s.modified_ms, self.offset),
            format!("{} msgs", n0_int(self.s.message_count)),
            size_text(self.s.size_bytes),
        ];
        let place = self.place(scope);
        if !place.is_empty() {
            parts.push(place);
        }
        parts.join(&format!(" {dot} "))
    }

    /// One labelled text wrapped into at most `rows` lines, the continuation under the text.
    fn labelled(&self, label: &str, text: &str, width: usize, rows: usize) -> Vec<String> {
        let head = format!("{label} {} ", self.g.r_angle);
        let indent = head.chars().count();
        let text = if text.is_empty() { "-" } else { text };
        split_text_lines(&strip_ctl(text), width.saturating_sub(indent).max(1), rows, self.ascii)
            .into_iter()
            .enumerate()
            .map(|(i, l)| {
                if i == 0 {
                    format!("{}{head}{}{l}", theme::DIM_OPEN, theme::DIM_CLOSE)
                } else {
                    format!("{}{l}", " ".repeat(indent))
                }
            })
            .collect()
    }

    /// AC25: title, meta, a rule, then the first prompt, the last prompt and the reply to it.
    /// Fewer than six rows fold title and meta into one line; the texts share what is left.
    pub(super) fn card(&self, width: usize, rows: usize, scope: PickerScope) -> Vec<String> {
        let h = self.g.h;
        let title = strip_ctl(&self.s.title);
        let meta = self.meta(scope);
        let mut out = if rows >= 6 {
            vec![
                limit_line(&title, width, self.ascii),
                format!("{}{}{}", theme::DIM_OPEN, limit_line(&meta, width, self.ascii), theme::DIM_CLOSE),
            ]
        } else {
            vec![limit_line(&format!("{title}  {h}  {meta}"), width, self.ascii)]
        };
        if rows > 2 {
            out.push(h.to_string().repeat(width));
        }
        let room = rows.saturating_sub(out.len());
        let reply = if self.s.last_assistant.is_empty() {
            "(no reply)"
        } else {
            self.s.last_assistant.as_str()
        };
        let last = if self.s.last_user.is_empty() {
            &self.s.first_prompt
        } else {
            &self.s.last_user
        };
        let texts = [self.s.first_prompt.as_str(), last.as_str(), reply];
        let shares: Vec<(usize, usize)> = match room {
            0 => Vec::new(),
            1 => vec![(1, 1)],
            2 => vec![(0, 1), (1, 1)],
            n => {
                let q = (n / 4).max(1);
                vec![(0, q), (1, q), (2, n - 2 * q)]
            }
        };
        let mut used = 0;
        for (i, n) in shares {
            let lines = self.labelled(CARD_LABELS[i], texts[i], width, n.min(room - used));
            used += lines.len();
            out.extend(lines);
        }
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
