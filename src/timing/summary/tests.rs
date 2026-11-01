use super::*;
fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("claude-auto-rs-timing-{name}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn summary_math_over_a_fixture_log() {
    let d = scratch("fixture");
    let mut text = String::from("{\"stage\":\"start\",\"ms\":999}\r\nnot json\r\n");
    text.extend(
        [7, 3, 20, 1, 15, 9, 2, 18, 11, 5, 14, 4, 19, 6, 13, 10, 8, 17, 12, 16]
            .map(|v| format!("{{\"stage\":\"timing\",\"name\":\"paint:launch\",\"ms\":{v}}}\r\n")),
    );
    text.push_str("{\"stage\":\"timing\",\"name\":\"config\",\"ms\":3.5}\r\n");
    let f = d.join("claude-auto-2026-11-01.jsonl");
    std::fs::write(&f, text).unwrap();
    let rows = summarize(&std::fs::read_to_string(&f).unwrap().lines().filter_map(record).collect::<Vec<_>>());
    assert_eq!(
        rows,
        [
            Row {
                name: "config".into(),
                count: 1,
                p50: 3.5,
                p95: 3.5,
                max: 3.5
            },
            Row {
                name: "paint:launch".into(),
                count: 20,
                p50: 10.0,
                p95: 19.0,
                max: 20.0
            },
        ]
    );
    let table = report(&[f]);
    assert_eq!(table.len(), 3);
    assert!(table[2].starts_with("paint:launch") && table[2].ends_with("20.00"));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_bad_byte_in_one_line_keeps_the_rest_of_the_day() {
    let d = scratch("bad-byte");
    let f = d.join("claude-auto-2026-11-01.jsonl");
    let mut bytes = b"{\"stage\":\"timing\",\"name\":\"config\",\"ms\":1}\r\n".to_vec();
    bytes.extend_from_slice(b"{\"stage\":\"start\",\"cwd\":\"\xff\"}\r\n");
    bytes.extend_from_slice(b"{\"stage\":\"timing\",\"name\":\"config\",\"ms\":3}\r\n");
    std::fs::write(&f, bytes).unwrap();
    let table = report(&[f]);
    assert_eq!(table.len(), 2, "{table:?}");
    assert!(table[1].starts_with("config") && table[1].contains("     2  "), "{table:?}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn hook_spans_list_right_under_hooks() {
    let recs: Vec<(String, f64)> = ["mcp", "hooks:chain:b.ps1+c.ps1", "hooks:a.ps1", "hooks", "hook"]
        .iter()
        .map(|n| ((*n).to_owned(), 1.0))
        .collect();
    let names: Vec<String> = summarize(&recs).into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["hook", "hooks", "hooks:a.ps1", "hooks:chain:b.ps1+c.ps1", "mcp"]);
}

#[test]
fn percentile_is_nearest_rank() {
    let v = [1.0, 2.0, 3.0, 4.0];
    assert_eq!(percentile(&v, 50), 2.0);
    assert_eq!(percentile(&v, 95), 4.0);
    assert_eq!(percentile(&v, 100), 4.0);
    assert_eq!(percentile(&[], 50), 0.0);
}

#[test]
fn a_missing_or_empty_log_reports_no_records() {
    let d = scratch("empty");
    let empty = d.join("claude-auto-2026-11-01.jsonl");
    std::fs::write(&empty, "").unwrap();
    assert_eq!(report(&[empty, d.join("missing.jsonl")]), ["no timing records"]);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn log_files_walk_back_across_a_month_end() {
    let f = log_files(std::path::Path::new("r"), (2028, 3, 1), 3);
    let names: Vec<String> = f.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(
        names,
        [
            "claude-auto-2028-03-01.jsonl",
            "claude-auto-2028-02-29.jsonl",
            "claude-auto-2028-02-28.jsonl"
        ]
    );
}

#[test]
fn timings_arguments_parse_strictly() {
    let a = |s: &[&str]| s.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    assert_eq!(parse_args(&a(&["--launcher-timings"])), Some(Ok(1)));
    assert_eq!(parse_args(&a(&["--launcher-timings", "--days", "7"])), Some(Ok(7)));
    assert!(matches!(parse_args(&a(&["--launcher-timings", "--days", "0"])), Some(Err(_))));
    assert!(matches!(parse_args(&a(&["--launcher-timings", "x"])), Some(Err(_))));
    assert_eq!(parse_args(&a(&["-p", "--launcher-timings"])), None);
}
