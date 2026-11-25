//! The speed budget: each hot path's work counts (bytes and calls read, bytes written) checked
//! against `perf/budget.json` plus its headroom, so a read or write regression fails the normal
//! suite. Counts only, never a wall time. Its own test binary because the I/O counters are
//! process-wide: the path tests take one lock, and nothing else in this binary does I/O.

mod world;

use std::error::Error;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use serde_json::Value;
use world::Counts;

type TestResult = Result<(), Box<dyn Error>>;

static IO_LOCK: Mutex<()> = Mutex::new(());
const EXE: &str = env!("CARGO_BIN_EXE_claude-auto");
const BUDGET: &str = include_str!("../../perf/budget.json");

fn budget() -> Result<Value, serde_json::Error> {
    serde_json::from_str(BUDGET)
}

/// Every breach of `path`'s budget, one line each; empty = within. Default deny: a path or metric
/// without a budget, a budgeted metric not measured, or a count of 0 (a counter not wired) breaches.
fn breaches(budget: &Value, path: &str, counts: &Counts) -> Vec<String> {
    let Some(headroom) = budget["headroomPercent"].as_u64() else {
        return vec!["budget.json: no headroomPercent".into()];
    };
    let Some(entry) = budget["paths"][path]["counts"].as_object() else {
        return vec![format!("{path}: no budget entry")];
    };
    let mut out = Vec::new();
    for (metric, value) in counts {
        match entry.get(*metric).and_then(Value::as_u64) {
            None => out.push(format!("{path}.{metric}: no budget (measured {value})")),
            // A budget of 0 is a promise of no work (a revisit reads nothing): 0 is its pass.
            Some(limit) if *value == 0 && limit > 0 => out.push(format!("{path}.{metric}: measured 0, the counter is not wired")),
            Some(limit) if u128::from(*value) * 100 > u128::from(limit) * u128::from(100 + headroom) => {
                out.push(format!("{path}.{metric}: {value} over the budget {limit} +{headroom} %"));
            }
            Some(_) => {}
        }
    }
    for metric in entry.keys() {
        if !counts.iter().any(|(m, _)| m == metric) {
            out.push(format!("{path}.{metric}: budgeted but not measured"));
        }
    }
    out
}

/// Measures `path` on the shared world and checks it against the budget.
fn check(path: &str) -> TestResult {
    let _only_io = IO_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let w = world::world(Path::new(env!("CARGO_TARGET_TMPDIR")))?;
    let (_, run) = world::PATHS
        .iter()
        .find(|(n, _)| *n == path)
        .ok_or_else(|| format!("{path}: not a measured path"))?;
    let (counts, _wall_time_never_gated) = run(Path::new(EXE), &w)?;
    let found = breaches(&budget()?, path, &counts);
    if found.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{}\nmeasured: {counts:?}\nupdate the budget only per perf/README.md",
            found.join("\n")
        )
        .into())
    }
}

#[test]
fn startup_to_the_first_launch_frame_stays_within_budget() -> TestResult {
    check("startup")
}

#[test]
fn project_screen_open_stays_within_budget() -> TestResult {
    check("project_screen")
}

#[test]
fn picker_open_with_a_cold_cache_stays_within_budget() -> TestResult {
    check("picker_open_cold")
}

#[test]
fn picker_open_with_a_warm_cache_stays_within_budget() -> TestResult {
    check("picker_open_warm")
}

#[test]
fn the_interactive_pickers_ui_thread_reads_quick_rows_only_before_its_first_frame() -> TestResult {
    check("picker_ui_open_cold")
}

#[test]
fn the_interactive_pickers_worker_scans_the_first_page_within_budget() -> TestResult {
    check("picker_worker_scan_cold")
}

#[test]
fn picker_cursor_move_writes_within_budget() -> TestResult {
    check("picker_cursor")
}

#[test]
fn chat_pane_fill_from_the_newest_transcript_reads_within_budget() -> TestResult {
    check("chat_load")
}

#[test]
fn chat_revisit_reads_nothing() -> TestResult {
    check("chat_revisit")
}

#[test]
fn chat_prefetch_on_open_reads_within_budget() -> TestResult {
    check("chat_prefetch_open")
}

#[test]
fn every_budgeted_path_is_measured_and_every_measured_path_budgeted() -> TestResult {
    let b = budget()?;
    let budgeted: Vec<&String> = b["paths"].as_object().ok_or("budget.json: no paths")?.keys().collect();
    let measured: Vec<&str> = world::PATHS.iter().map(|(n, _)| *n).collect();
    let unknown: Vec<&&String> = budgeted.iter().filter(|p| !measured.contains(&p.as_str())).collect();
    let unbudgeted: Vec<&&str> = measured.iter().filter(|m| !budgeted.iter().any(|b| b == *m)).collect();
    assert!(unknown.is_empty(), "budget paths nothing measures: {unknown:?}");
    assert!(unbudgeted.is_empty(), "measured paths without a budget: {unbudgeted:?}");
    Ok(())
}

#[test]
fn publish_replaces_a_stale_root_and_keeps_a_complete_one() -> TestResult {
    let _only_io = IO_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("publish-test-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d)?;
    }
    let (root, tmp) = (d.join("root"), d.join("tmp"));
    // Given: a stale root (a leftover file, no `done`), as rust-cache restores it.
    std::fs::create_dir_all(root.join("home"))?;
    std::fs::write(root.join("home").join("left.txt"), "x")?;
    std::fs::create_dir_all(&tmp)?;
    std::fs::write(tmp.join("done"), world::VERSION)?;
    // When: the built tree is published. Then: it replaces the stale root.
    world::publish(&tmp, &root)?;
    assert!(!root.join("home").exists() && root.join("done").exists() && !tmp.exists());
    // Given: a complete root and a second built tree. Then: the complete root stays, the tree goes.
    std::fs::write(root.join("kept.txt"), "x")?;
    std::fs::create_dir_all(&tmp)?;
    std::fs::write(tmp.join("done"), world::VERSION)?;
    world::publish(&tmp, &root)?;
    assert!(root.join("kept.txt").exists() && !tmp.exists());
    std::fs::remove_dir_all(&d)?;
    Ok(())
}

#[test]
fn breaches_deny_a_zero_count_a_missing_entry_and_an_unmeasured_metric() -> TestResult {
    // Given: a budget of 100 read bytes and 10 read calls, 10 % headroom.
    let b: Value = serde_json::from_str(r#"{"headroomPercent":10,"paths":{"p":{"counts":{"read_bytes":100,"read_ops":10}}}}"#)?;
    // Then: within (110 is the edge) passes, one over fails.
    assert_eq!(breaches(&b, "p", &vec![("read_bytes", 110), ("read_ops", 1)]), Vec::<String>::new());
    assert_eq!(breaches(&b, "p", &vec![("read_bytes", 111), ("read_ops", 1)]).len(), 1);
    // And: a zero count, an unbudgeted metric, an unmeasured one and an unknown path each fail.
    assert_eq!(breaches(&b, "p", &vec![("read_bytes", 0), ("read_ops", 1)]).len(), 1);
    assert_eq!(
        breaches(&b, "p", &vec![("read_bytes", 1), ("read_ops", 1), ("write_bytes", 1)]).len(),
        1
    );
    assert_eq!(breaches(&b, "p", &vec![("read_bytes", 1)]).len(), 1);
    assert_eq!(breaches(&b, "q", &vec![("read_bytes", 1)]).len(), 1);
    // And: a budget of 0 passes 0 and fails any work at all.
    let zero: Value = serde_json::from_str(r#"{"headroomPercent":10,"paths":{"z":{"counts":{"read_bytes":0}}}}"#)?;
    assert_eq!(breaches(&zero, "z", &vec![("read_bytes", 0)]), Vec::<String>::new());
    assert_eq!(breaches(&zero, "z", &vec![("read_bytes", 1)]).len(), 1);
    Ok(())
}
