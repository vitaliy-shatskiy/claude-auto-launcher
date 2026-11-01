//! The texts a picker row and the preview show: counts, ages, stamps and the last exchange.

use super::super::listing::{Message, Session, Speaker};
use crate::layout::split_text_lines;
use crate::theme::{Glyphs, strip_ctl};

/// .NET `{0:N0}` in the invariant culture: rounded half away from zero, thousands grouped.
pub(super) fn n0(x: f64) -> String {
    #[expect(clippy::cast_possible_truncation, reason = "ages and sizes are far inside i64")]
    let v = x.round() as i64;
    let digits = v.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if v < 0 { format!("-{out}") } else { out }
}

/// `Format-RelativeAge`.
#[expect(clippy::cast_precision_loss, reason = "a millisecond age fits f64 exactly enough")]
pub fn relative_age(from_ms: i64, now_ms: i64) -> String {
    let secs = (now_ms - from_ms) as f64 / 1000.0;
    if secs < 60.0 {
        "now".into()
    } else if secs < 3600.0 {
        format!("{} min", n0(secs / 60.0))
    } else if secs < 86_400.0 {
        format!("{} h", n0(secs / 3600.0))
    } else {
        format!("{} d", n0(secs / 86_400.0))
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `ToString('dd MMM HH:mm')` in the invariant culture, at the clock's local offset.
pub fn local_stamp(ms: i64, offset_minutes: i32) -> String {
    let minutes = ms.div_euclid(60_000) + i64::from(offset_minutes);
    let (days, rem) = (minutes.div_euclid(1440), minutes.rem_euclid(1440));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let month = MONTHS.get(usize::try_from(m - 1).unwrap_or(0)).unwrap_or(&"Jan");
    format!("{d:02} {month} {:02}:{:02}", rem / 60, rem % 60)
}

/// `Get-SessionExchange`: the recent messages, or the last question and answer.
pub fn exchange(s: &Session) -> Vec<Message> {
    if !s.recent.is_empty() {
        return s.recent.clone();
    }
    let user = [&s.last_user, &s.title]
        .into_iter()
        .find(|t| !t.is_empty())
        .map_or_else(|| "(nothing was asked)".to_owned(), Clone::clone);
    let reply = if s.last_assistant.is_empty() {
        "(no reply)".to_owned()
    } else {
        s.last_assistant.clone()
    };
    vec![
        Message {
            speaker: Speaker::User,
            text: user,
        },
        Message {
            speaker: Speaker::Assistant,
            text: reply,
        },
    ]
}

/// `Get-ExchangeLines`: as many of the newest messages as fit, oldest on top, each capped so a
/// long one cannot eat the pane.
pub fn exchange_lines(messages: &[Message], width: usize, max_lines: usize, g: &Glyphs, ascii: bool) -> Vec<String> {
    const MIN_PER_MESSAGE: usize = 5;
    if max_lines == 0 || width == 0 {
        return Vec::new();
    }
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut used = 0;
    for (i, m) in messages.iter().enumerate().rev() {
        if used >= max_lines {
            break;
        }
        let left = max_lines - used;
        let cap = MIN_PER_MESSAGE.max(left / (i + 1).min(3));
        let budget = cap.min(left);
        let prefix = match m.speaker {
            Speaker::User => format!("you {} ", g.r_angle),
            Speaker::Assistant => format!("claude {} ", g.r_angle),
        };
        let plen = prefix.chars().count();
        let wrapped = split_text_lines(&strip_ctl(&m.text), width.saturating_sub(plen).max(1), budget, ascii);
        if wrapped.is_empty() {
            continue;
        }
        let lines: Vec<String> = wrapped
            .iter()
            .enumerate()
            .map(|(j, w)| {
                if j == 0 {
                    format!("{prefix}{w}")
                } else {
                    format!("{}{w}", " ".repeat(plen))
                }
            })
            .collect();
        used += lines.len();
        blocks.insert(0, lines);
    }
    blocks.concat()
}
