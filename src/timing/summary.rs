//! `--launcher-timings [--days N]`: count, p50, p95 and max per span name over the `timing` records of the
//! run logs, today and the `N - 1` days before it.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// One span name's statistics, in ms.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub count: usize,
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
}

/// Nearest rank over sorted values: the smallest value with `pct` percent of them at or below it.
pub fn percentile(sorted: &[f64], pct: usize) -> f64 {
    let rank = (pct * sorted.len()).div_ceil(100).max(1);
    sorted.get(rank - 1).copied().unwrap_or(0.0)
}

/// A `timing` record's name and ms; `None` for any other line.
pub fn record(line: &str) -> Option<(String, f64)> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("stage")?.as_str()? != "timing" {
        return None;
    }
    Some((v.get("name")?.as_str()?.to_owned(), v.get("ms")?.as_f64()?))
}

/// The rows by span name.
pub fn summarize(records: &[(String, f64)]) -> Vec<Row> {
    let mut by_name: std::collections::BTreeMap<&str, Vec<f64>> = std::collections::BTreeMap::new();
    for (name, ms) in records {
        by_name.entry(name).or_default().push(*ms);
    }
    by_name
        .into_iter()
        .map(|(name, mut v)| {
            v.sort_by(f64::total_cmp);
            Row {
                name: name.to_owned(),
                count: v.len(),
                p50: percentile(&v, 50),
                p95: percentile(&v, 95),
                max: v.last().copied().unwrap_or(0.0),
            }
        })
        .collect()
}

/// The printed table; one line when there is nothing to show.
pub fn table(rows: &[Row]) -> Vec<String> {
    if rows.is_empty() {
        return vec!["no timing records".into()];
    }
    let w = rows.iter().map(|r| r.name.chars().count()).max().unwrap_or(0).max(4);
    let mut out = vec![format!(
        "{:<w$}  {:>6}  {:>9}  {:>9}  {:>9}",
        "span", "count", "p50 ms", "p95 ms", "max ms"
    )];
    out.extend(
        rows.iter()
            .map(|r| format!("{:<w$}  {:>6}  {:>9.2}  {:>9.2}  {:>9.2}", r.name, r.count, r.p50, r.p95, r.max)),
    );
    out
}

/// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The log files of `days` days ending with `today` (year, month, day), newest first.
pub fn log_files(root: &Path, today: (i64, i64, i64), days: u32) -> Vec<PathBuf> {
    let base = days_from_civil(today.0, today.1, today.2);
    (0..i64::from(days))
        .map(|back| {
            let (y, m, d) = civil_from_days(base - back);
            root.join(format!("claude-auto-{y:04}-{m:02}-{d:02}.jsonl"))
        })
        .collect()
}

/// `--launcher-timings` alone or `--launcher-timings --days N` (1 to 366); `None` = not this command.
pub fn parse_args(args: &[String]) -> Option<Result<u32, String>> {
    let (first, rest) = args.split_first()?;
    if first != "--launcher-timings" {
        return None;
    }
    Some(match rest {
        [] => Ok(1),
        [flag, n] if flag == "--days" => n
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=366).contains(n))
            .ok_or_else(|| format!("--days takes a number from 1 to 366, got '{n}'")),
        _ => Err("usage: claude-auto-rs --launcher-timings [--days N]".into()),
    })
}

/// The table over the given files; a missing or unreadable file counts as empty.
pub fn report(files: &[PathBuf]) -> Vec<String> {
    let records: Vec<(String, f64)> = files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .flat_map(|text| text.lines().filter_map(record).collect::<Vec<_>>())
        .collect();
    table(&summarize(&records))
}

#[cfg(test)]
mod tests;
