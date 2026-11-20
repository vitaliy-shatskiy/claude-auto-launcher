//! The texts a picker row and the preview show: counts, ages and stamps.

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

/// `DD.MM HH:MM`, 24-hour, at the clock's local offset (the owner's date format).
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
    format!("{d:02}.{m:02} {:02}:{:02}", rem / 60, rem % 60)
}
