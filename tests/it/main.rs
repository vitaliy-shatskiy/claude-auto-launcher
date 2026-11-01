//! End-to-end through the built binary: preview frame dump, argv pass-through, the child marker,
//! `--mcp-config` placement, and `--launcher-version`. Every run gets its own fake home, so
//! nothing touches the real `~/.claude`.
#![expect(clippy::unwrap_used, reason = "test helpers fail the test on a broken fixture")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const EXE: &str = env!("CARGO_BIN_EXE_claude-auto-rs");

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("claude-auto-rs-it-{name}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(d.join("home").join(".claude")).unwrap();
    std::fs::create_dir_all(d.join("bin")).unwrap();
    d
}

/// A fake `claude` that prints its argv and the launch marker, then exits 7.
fn fake_claude(dir: &Path) {
    std::fs::write(
        dir.join("bin").join("claude.cmd"),
        "@echo ARGV %*\r\n@echo MARK [%CLAUDE_AUTO_LAUNCHED%]\r\n@exit /b 7\r\n",
    )
    .unwrap();
}

fn launcher(dir: &Path) -> Command {
    let mut c = Command::new(EXE);
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    c.env("USERPROFILE", dir.join("home"))
        .env("PATH", format!(r"{};{system}\System32", dir.join("bin").display()))
        .env("PATHEXT", ".COM;.EXE;.BAT;.CMD")
        .env("CLAUDE_AUTO_CONFIG", dir.join("home").join("none.json"))
        .env("CLAUDE_AUTO_PREFS", dir.join("prefs.json"))
        .env("NO_COLOR", "1")
        .env("CLAUDE_NO_ROAM", "1")
        .env_remove("CLAUDE_AUTO_PREVIEW")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_AUTO_LAUNCHED")
        .env_remove("CLAUDE_AUTO_DEBUG")
        .current_dir(dir);
    c
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n")
}

#[test]
fn launcher_version_alone_prints_and_exits_zero() {
    let d = scratch("version");
    let o = launcher(&d).arg("--launcher-version").output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(text(&o).trim_end(), format!("claude-auto {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn arguments_pass_through_verbatim_with_exit_code_and_marker() {
    let d = scratch("pass");
    fake_claude(&d);
    std::fs::write(d.join("home").join(".claude").join("mcp-shared.json"), "{}").unwrap();
    let o = launcher(&d)
        .args(["--resume", "abc", "-p", "x y", "--launcher-version"])
        .output()
        .unwrap();
    let out = text(&o);
    assert_eq!(o.status.code(), Some(7), "the child's exit code: {out}");
    let mcp = d.join("home").join(".claude").join("mcp-shared.json");
    assert!(
        out.contains(&format!(
            "ARGV --resume abc -p \"x y\" --launcher-version --mcp-config {}",
            mcp.display()
        )),
        "{out}"
    );
    assert!(out.contains("MARK [1]"), "CLAUDE_AUTO_LAUNCHED=1 for the child: {out}");
    assert!(out.contains("-> work account"), "{out}");
    std::fs::remove_dir_all(&d).unwrap();
}

fn timing_records(d: &Path) -> Vec<serde_json::Value> {
    let logs = d.join("home").join(".claude").join("launcher-logs");
    let Ok(entries) = std::fs::read_dir(&logs) else { return Vec::new() };
    entries
        .flatten()
        .flat_map(|e| {
            std::fs::read_to_string(e.path())
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(&l).ok())
        .filter(|v| v["stage"] == "timing")
        .collect()
}

#[test]
fn debug_off_logs_no_timing_records_and_debug_on_logs_spans() {
    let d = scratch("timing-log");
    fake_claude(&d);
    let o = launcher(&d).args(["-p", "x"]).output().unwrap();
    assert_eq!(o.status.code(), Some(7), "{}", text(&o));
    assert_eq!(timing_records(&d), Vec::<serde_json::Value>::new());
    let o = launcher(&d).env("CLAUDE_AUTO_DEBUG", "1").args(["-p", "x"]).output().unwrap();
    assert_eq!(o.status.code(), Some(7), "{}", text(&o));
    let recs = timing_records(&d);
    let names: Vec<&str> = recs.iter().filter_map(|r| r["name"].as_str()).collect();
    for n in ["config", "process", "spawn"] {
        assert!(names.contains(&n), "{n} in {names:?}");
    }
    let config = recs.iter().find(|r| r["name"] == "config").unwrap();
    assert_eq!(config["parent"], "process");
    assert!(config["ms"].is_f64() && config["startMs"].is_f64(), "{config}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn claudes_own_debug_flag_alone_passes_through() {
    let d = scratch("debug-pass");
    fake_claude(&d);
    for debug_env in [false, true] {
        let mut c = launcher(&d);
        if debug_env {
            c.env("CLAUDE_AUTO_DEBUG", "1");
        }
        let o = c.arg("--debug").output().unwrap();
        let out = text(&o);
        assert_eq!(o.status.code(), Some(7), "{out}");
        assert!(out.contains("ARGV --debug"), "debug env {debug_env}: {out}");
    }
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn timings_without_a_log_prints_no_records_and_exits_zero() {
    let d = scratch("timings-empty");
    let o = launcher(&d).args(["--launcher-timings", "--days", "3"]).output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(text(&o).trim_end(), "no timing records");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn preview_in_debug_mode_shows_the_footer_and_the_timings_screen() {
    let d = scratch("debug-preview");
    let config = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("config-four.json");
    let run = |debug: bool| {
        let mut c = launcher(&d);
        c.env("CLAUDE_AUTO_CONFIG", &config)
            .env("CLAUDE_AUTO_PREVIEW", "1")
            .env("CLAUDE_AUTO_PREVIEW_KEYS", "F12,Escape,Escape");
        if debug {
            c.arg("--launcher-debug");
        }
        text(&c.output().unwrap())
    };
    let on = run(true);
    assert!(on.contains("  load "), "{on}");
    assert!(on.contains("last key->frame"), "{on}");
    assert!(on.contains("timings of this run"), "{on}");
    assert!(on.contains("screen:launch"), "{on}");
    let off = run(false);
    assert!(!off.contains("last key->frame") && !off.contains("timings of this run"), "{off}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn subcommand_gets_no_mcp_config() {
    let d = scratch("sub");
    fake_claude(&d);
    std::fs::write(d.join("home").join(".claude").join("mcp-shared.json"), "{}").unwrap();
    let o = launcher(&d).args(["mcp", "list"]).output().unwrap();
    let out = text(&o);
    assert!(out.contains("ARGV mcp list\n"), "{out}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn missing_claude_exits_one() {
    let d = scratch("missing");
    let o = launcher(&d).arg("--version").output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).contains("claude is not on PATH - install Claude Code first"));
    std::fs::remove_dir_all(&d).unwrap();
}

/// Maps the Unicode goldens onto the ASCII glyph set, for a console not on UTF-8.
fn to_ascii(s: &str) -> String {
    let pairs = [
        ('\u{256D}', '+'),
        ('\u{256E}', '+'),
        ('\u{2570}', '+'),
        ('\u{256F}', '+'),
        ('\u{2500}', '-'),
        ('\u{2502}', '|'),
        ('\u{273B}', '*'),
        ('\u{276F}', '>'),
        ('\u{25CF}', '*'),
        ('\u{25CB}', '-'),
        ('\u{250A}', ':'),
    ];
    s.chars()
        .map(|c| pairs.iter().find(|(u, _)| *u == c).map_or(c, |(_, a)| *a))
        .collect()
}

#[test]
fn preview_dumps_the_launch_frame_and_launches_nothing() {
    let d = scratch("preview");
    fake_claude(&d);
    let config = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("config-four.json");
    let o = launcher(&d)
        .env("CLAUDE_AUTO_CONFIG", &config)
        .env("CLAUDE_AUTO_PREVIEW", "1")
        .env("CLAUDE_AUTO_PREVIEW_SIZE", "100x30")
        .env("CLAUDE_AUTO_PREVIEW_KEYS", "Enter,Enter")
        .output()
        .unwrap();
    let out = text(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    let golden =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wide-frames/launch-100x30.txt")).unwrap();
    let golden = golden.replace("\r\n", "\n");
    let expected = if out.contains('\u{256D}') { golden } else { to_ascii(&golden) };
    assert!(out.starts_with(&expected), "frame differs:\n{out}");
    assert!(out.contains("--- preview, nothing launched ---"));
    assert!(out.contains("launch args        : \n"), "a default launch adds no arguments: {out}");
    assert!(!out.contains("ARGV"), "preview never starts claude");
    assert!(!d.join("prefs.json").exists(), "preview writes no prefs");
    assert!(
        !d.join("home").join(".claude").join("launcher-logs").exists(),
        "preview writes no log"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn preview_steps_rows_into_launch_arguments() {
    let d = scratch("steps");
    fake_claude(&d);
    let config = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("config-four.json");
    let o = launcher(&d)
        .env("CLAUDE_AUTO_CONFIG", &config)
        .env("CLAUDE_AUTO_PREVIEW", "1")
        .env("CLAUDE_AUTO_PREVIEW_KEYS", "RightArrow,s,s,d,s,d,d,d,d,s,d,Enter,Enter")
        .output()
        .unwrap();
    let out = text(&o);
    assert!(out.contains("account            : personal"), "{out}");
    assert!(
        out.contains("launch args        : --model fable --effort xhigh --advisor fable"),
        "{out}"
    );
    assert!(out.contains("CLAUDE_CONFIG_DIR  : "), "{out}");
    std::fs::remove_dir_all(&d).unwrap();
}
