//! `Get-FreshestRateLimitRecord`, `Get-RateLimitSummary`: last-known limits per account from two
//! writers - `statusline.js` (`<name>.json`, while a session runs) and the usage widget
//! (`<name>.widget.json`, every five minutes). Freshest `atMs` wins for the numbers; the age is
//! always rendered. `available_models` comes from the WIDGET record only (one writer), three-state.
//! The model bucket comes from the freshest record, else from a fresh widget record.

use std::path::Path;

use serde_json::Value;

use crate::config::{Account, ps_str, ps_truthy};
use crate::types::{LimitRecord, Limits};

/// A widget `availableModels` older than this is unknown: six missed five-minute polls.
pub(crate) const AVAILABILITY_MAX_AGE_MS: i64 = 30 * 60 * 1000;

fn read(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// PowerShell's `[long]` / `[int]` of a JSON value: a number (rounded half to even) or a numeric
/// string. `None` = the cast would throw.
fn to_int(v: &Value) -> Option<i64> {
    let f = match v {
        Value::Number(n) => return n.as_i64().or_else(|| n.as_f64().map(|f| float_to_i64(f.round_ties_even()))),
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        Value::Bool(b) => return Some(i64::from(*b)),
        Value::Null | Value::Array(_) | Value::Object(_) => return None,
    };
    Some(float_to_i64(f.round_ties_even()))
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "PowerShell's cast saturates the same way at these magnitudes"
)]
fn float_to_i64(f: f64) -> i64 {
    f as i64
}

/// The cast PowerShell would have thrown on: the whole record is dropped.
#[derive(Debug)]
struct BadCast;

/// A present, non-null field as an integer; `Ok(None)` when absent or null.
fn opt_int(j: &Value, key: &str) -> Result<Option<i64>, BadCast> {
    match j.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => to_int(v).map(Some).ok_or(BadCast),
    }
}

fn at_ms(j: &Value) -> Option<i64> {
    j.get("atMs").filter(|v| !v.is_null()).and_then(to_int)
}

/// `Get-FreshestRateLimitRecord`: newest `atMs` of the two files; a file that is unreadable,
/// malformed or unstamped is no candidate.
pub(crate) fn freshest(dir: &Path, name: &str) -> Option<Value> {
    let mut best: Option<(i64, Value)> = None;
    for suffix in [".json", ".widget.json"] {
        let Some(j) = read(&dir.join(format!("{name}{suffix}"))) else {
            continue;
        };
        let Some(at) = at_ms(&j) else { continue };
        if best.as_ref().is_none_or(|(b, _)| at > *b) {
            best = Some((at, j));
        }
    }
    best.map(|(_, j)| j)
}

/// `'{0:N0}'`: rounded half away from zero.
#[expect(clippy::cast_possible_truncation, reason = "an age in whole units fits an i64")]
fn n0(x: f64) -> i64 {
    x.round() as i64
}

#[expect(clippy::cast_precision_loss, reason = "an age in milliseconds is far below 2^52")]
pub(crate) fn age_text(age_ms: i64) -> String {
    let secs = age_ms as f64 / 1000.0;
    if secs < 90.0 {
        "just now".into()
    } else if secs / 60.0 < 60.0 {
        format!("{} min ago", n0(secs / 60.0))
    } else if secs / 3600.0 < 24.0 {
        format!("{} h ago", n0(secs / 3600.0))
    } else {
        format!("{} d ago", n0(secs / 86_400.0))
    }
}

/// The widget's `availableModels`: `None` = no information (no file, half-written, unstamped,
/// stale, or the field absent); `Some(list)` = an answer, empty included. JSON null and members
/// that are null or blank are present-and-empty, never absent.
fn available(wj: &Value) -> Option<Vec<String>> {
    let list = match wj.get("availableModels")? {
        Value::Array(a) => a.iter().map(|v| ps_str(v).trim().to_owned()).filter(|s| !s.is_empty()).collect(),
        Value::Null => Vec::new(),
        other => Some(ps_str(other).trim().to_owned())
            .filter(|s| !s.is_empty())
            .into_iter()
            .collect(),
    };
    Some(list)
}

/// The widget record when it is stamped within `max_age_ms`: no file, half-written, unstamped or
/// stale is `None`.
fn fresh_widget(dir: &Path, name: &str, now_ms: i64, max_age_ms: i64) -> Option<Value> {
    let wj = read(&dir.join(format!("{name}.widget.json")))?;
    let at = at_ms(&wj)?;
    (now_ms.saturating_sub(at) <= max_age_ms).then_some(wj)
}

/// The per-model weekly bucket and its label; a bad cast is no bucket.
fn bucket(j: &Value) -> Result<(Option<i64>, Option<String>), BadCast> {
    Ok((
        opt_int(j, "modelSevenDay")?,
        ps_truthy(j.get("modelLabel")).then(|| j.get("modelLabel").map(ps_str).unwrap_or_default()),
    ))
}

fn record(j: &Value, age: String, widget: Option<&Value>) -> Result<LimitRecord, BadCast> {
    let (mut model, mut model_label) = bucket(j)?;
    // Only the widget writes the model bucket: a fresher statusline record must not hide it.
    if model.is_none()
        && let Some((Some(m), label)) = widget.and_then(|w| bucket(w).ok())
    {
        (model, model_label) = (Some(m), label);
    }
    Ok(LimitRecord {
        five_hour: opt_int(j, "fiveHour")?,
        seven_day: opt_int(j, "sevenDay")?,
        age_text: age,
        model,
        model_label,
        available_models: widget.and_then(available),
    })
}

/// `Get-RateLimitSummary` with the availability cap as a parameter. An account with no record,
/// or a record with neither percentage, is absent; a malformed one shows nothing.
pub(crate) fn summary(accounts: &[Account], dir: &Path, now_ms: i64, max_age_ms: i64) -> Limits {
    let mut out = Limits::new();
    for a in accounts {
        let Some(j) = freshest(dir, &a.key) else { continue };
        let Some(at) = at_ms(&j) else { continue };
        let present = |k: &str| j.get(k).is_some_and(|v| !v.is_null());
        if !present("fiveHour") && !present("sevenDay") {
            continue;
        }
        let widget = fresh_widget(dir, &a.key, now_ms, max_age_ms);
        if let Ok(r) = record(&j, age_text(now_ms.saturating_sub(at)), widget.as_ref()) {
            out.insert(a.key.clone(), r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    struct Store(std::path::PathBuf);
    impl Store {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!("claude-auto-rs-limits-{tag}-{}", crate::log::new_run_id()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn put(&self, name: &str, body: &str) {
            std::fs::write(self.0.join(name), body).unwrap();
        }
    }
    impl Drop for Store {
        fn drop(&mut self) {
            if std::fs::remove_dir_all(&self.0).is_err() {
                // A leftover temp directory is litter, not a test failure.
            }
        }
    }

    fn accounts() -> Vec<Account> {
        ["work", "personal", "shared", "low"]
            .iter()
            .map(|k| Account {
                key: (*k).into(),
                root: String::new(),
                label: String::new(),
                tint: String::new(),
                hidden: false,
                canonical: *k == "work",
            })
            .collect()
    }

    fn five(s: &Store, name: &str) -> Option<i64> {
        freshest(&s.0, name).and_then(|j| j.get("fiveHour").and_then(Value::as_i64))
    }

    fn sum(s: &Store) -> Limits {
        summary(&accounts(), &s.0, NOW, AVAILABILITY_MAX_AGE_MS)
    }

    #[test]
    fn the_freshest_of_two_writers_wins() {
        let s = Store::new("fresh");
        s.put("a.json", r#"{"fiveHour":10,"sevenDay":20,"atMs":1000}"#);
        s.put("a.widget.json", r#"{"fiveHour":11,"sevenDay":21,"atMs":2000}"#);
        assert_eq!(five(&s, "a"), Some(11), "the widget file wins when it is fresher");
        s.put("b.json", r#"{"fiveHour":30,"atMs":9000}"#);
        s.put("b.widget.json", r#"{"fiveHour":31,"atMs":8000}"#);
        assert_eq!(five(&s, "b"), Some(30), "a live session wins over an older widget poll");
        s.put("c.json", r#"{"fiveHour":40,"atMs":5000}"#);
        assert_eq!(five(&s, "c"), Some(40), "one file alone is used");
        assert!(freshest(&s.0, "nothing").is_none(), "no file at all is None, not an error");
        s.put("d.json", r#"{"fiveHour":50,"atMs":5000}"#);
        s.put("d.widget.json", r#"{"fiveHour":51,"#);
        assert_eq!(five(&s, "d"), Some(50), "a malformed file does not shadow a good one");
        s.put("e.json", r#"{"fiveHour":60}"#);
        s.put("e.widget.json", r#"{"fiveHour":61,"atMs":1}"#);
        assert_eq!(five(&s, "e"), Some(61), "a record without atMs is ignored");
    }

    #[test]
    fn the_model_bucket_travels_with_its_own_record() {
        let s = Store::new("bucket");
        s.put("work.json", &format!(r#"{{"fiveHour":39,"sevenDay":47,"atMs":{}}}"#, NOW - 600_000));
        s.put(
            "work.widget.json",
            &format!(r#"{{"fiveHour":40,"sevenDay":48,"atMs":{NOW},"modelSevenDay":15,"modelLabel":"FABLE"}}"#),
        );
        s.put(
            "personal.widget.json",
            &format!(
                r#"{{"fiveHour":10,"sevenDay":20,"atMs":{},"modelSevenDay":15,"modelLabel":"FABLE"}}"#,
                NOW - 600_000
            ),
        );
        s.put("personal.json", &format!(r#"{{"fiveHour":11,"sevenDay":21,"atMs":{NOW}}}"#));
        let l = sum(&s);
        assert_eq!(
            (l["work"].five_hour, l["work"].model, l["work"].model_label.as_deref()),
            (Some(40), Some(15), Some("FABLE"))
        );
        assert_eq!(
            (l["personal"].five_hour, l["personal"].model, l["personal"].model_label.as_deref()),
            (Some(11), Some(15), Some("FABLE")),
            "a fresher statusline record keeps the numbers, the fresh widget adds its model bucket"
        );
        assert_eq!(l["work"].age_text, "just now");
        assert_eq!(l["personal"].age_text, "just now");
    }

    #[test]
    fn a_stale_missing_or_malformed_widget_adds_no_model_bucket() {
        let s = Store::new("bucket-stale");
        s.put("work.json", &format!(r#"{{"sevenDay":83,"atMs":{NOW}}}"#));
        s.put(
            "work.widget.json",
            &format!(r#"{{"sevenDay":84,"atMs":{},"modelSevenDay":1,"modelLabel":"FABLE"}}"#, NOW - 31 * 60_000),
        );
        s.put("personal.json", &format!(r#"{{"sevenDay":83,"atMs":{NOW}}}"#));
        s.put("shared.json", &format!(r#"{{"sevenDay":83,"atMs":{NOW}}}"#));
        s.put("shared.widget.json", r#"{"modelSevenDay":1,"#);
        s.put("low.json", &format!(r#"{{"sevenDay":83,"atMs":{NOW}}}"#));
        s.put(
            "low.widget.json",
            &format!(r#"{{"sevenDay":84,"atMs":{},"modelSevenDay":"x","modelLabel":"FABLE"}}"#, NOW - 60_000),
        );
        let l = sum(&s);
        for k in ["work", "personal", "shared", "low"] {
            assert_eq!((l[k].seven_day, l[k].model, l[k].model_label.as_deref()), (Some(83), None, None), "{k}");
        }
    }

    #[test]
    fn available_models_come_from_the_widget_record_three_state() {
        let s = Store::new("avail");
        s.put(
            "work.json",
            &format!(r#"{{"fiveHour":77,"sevenDay":88,"atMs":{NOW},"availableModels":["fable"]}}"#),
        );
        s.put(
            "work.widget.json",
            &format!(r#"{{"fiveHour":11,"sevenDay":22,"atMs":{},"availableModels":[]}}"#, NOW - 300_000),
        );
        s.put("personal.json", &format!(r#"{{"fiveHour":10,"sevenDay":20,"atMs":{NOW}}}"#));
        s.put(
            "shared.widget.json",
            &format!(r#"{{"fiveHour":30,"sevenDay":40,"atMs":{NOW},"availableModels":["fable","opus"]}}"#),
        );
        s.put("low.json", &format!(r#"{{"fiveHour":55,"sevenDay":66,"atMs":{NOW}}}"#));
        s.put("low.widget.json", r#"{"availableModels":[],"#);
        let l = sum(&s);
        assert_eq!(
            l["work"].available_models,
            Some(vec![]),
            "from the OLDER widget record, never the fresher statusline one"
        );
        assert_eq!(l["work"].five_hour, Some(77));
        assert_eq!((l["personal"].available_models.clone(), l["personal"].five_hour), (None, Some(10)));
        assert_eq!(l["shared"].available_models, Some(vec!["fable".to_owned(), "opus".to_owned()]));
        assert_eq!((l["low"].available_models.clone(), l["low"].five_hour), (None, Some(55)));
    }

    #[test]
    fn only_an_absent_field_is_no_information() {
        let s = Store::new("empty");
        s.put("work.widget.json", &format!(r#"{{"fiveHour":40,"sevenDay":48,"atMs":{NOW}}}"#));
        s.put(
            "personal.widget.json",
            &format!(r#"{{"fiveHour":10,"sevenDay":20,"atMs":{NOW},"availableModels":null}}"#),
        );
        s.put(
            "low.widget.json",
            &format!(r#"{{"fiveHour":10,"sevenDay":20,"atMs":{NOW},"availableModels":[null,""]}}"#),
        );
        s.put(
            "shared.widget.json",
            &format!(r#"{{"fiveHour":10,"sevenDay":20,"atMs":{NOW},"availableModels":["fable"]}}"#),
        );
        let l = sum(&s);
        assert_eq!(l["work"].available_models, None);
        assert_eq!(
            l["personal"].available_models,
            Some(vec![]),
            "a JSON null field is PRESENT and empty"
        );
        assert_eq!(
            l["low"].available_models,
            Some(vec![]),
            "a list of null and blank is present and empty"
        );
        assert_eq!(l["shared"].available_models, Some(vec!["fable".to_owned()]));
    }

    #[test]
    fn an_unstamped_or_stale_widget_carries_no_availability() {
        let s = Store::new("stamp");
        s.put("work.json", &format!(r#"{{"fiveHour":40,"sevenDay":48,"atMs":{NOW}}}"#));
        s.put("work.widget.json", r#"{"availableModels":[]}"#);
        s.put("personal.json", &format!(r#"{{"fiveHour":10,"sevenDay":20,"atMs":{NOW}}}"#));
        s.put("personal.widget.json", &format!(r#"{{"availableModels":[],"atMs":{NOW}}}"#));
        let l = sum(&s);
        assert_eq!((l["work"].available_models.clone(), l["work"].five_hour), (None, Some(40)));
        assert_eq!(
            l["personal"].available_models,
            Some(vec![]),
            "the same record WITH atMs does carry it"
        );

        let s = Store::new("age");
        s.put(
            "work.widget.json",
            &format!(
                r#"{{"fiveHour":40,"sevenDay":48,"atMs":{},"availableModels":[]}}"#,
                NOW - 31 * 60_000
            ),
        );
        s.put(
            "personal.widget.json",
            &format!(
                r#"{{"fiveHour":10,"sevenDay":20,"atMs":{},"availableModels":[]}}"#,
                NOW - 29 * 60_000
            ),
        );
        let l = sum(&s);
        assert_eq!(
            (l["work"].available_models.clone(), l["work"].five_hour),
            (None, Some(40)),
            "older than 30 min is unknown"
        );
        assert_eq!(l["personal"].available_models, Some(vec![]));
        assert_eq!(l["work"].age_text, "31 min ago");
        let l = summary(&accounts(), &s.0, NOW, 60 * 60_000);
        assert!(l["work"].available_models.is_some(), "the cap is a parameter");
    }

    #[test]
    fn no_records_is_an_empty_table_and_ages_read_like_powershell() {
        let s = Store::new("none");
        assert!(sum(&s).is_empty());
        assert_eq!(age_text(89_000), "just now");
        assert_eq!(age_text(59 * 60_000 + 40_000), "60 min ago");
        assert_eq!(age_text(5 * 3_600_000), "5 h ago");
        assert_eq!(age_text(3 * 86_400_000), "3 d ago");
        assert_eq!(to_int(&serde_json::json!(39.5)), Some(40));
        assert_eq!(to_int(&serde_json::json!(40.5)), Some(40), "[int] rounds half to even");
    }
}
