//! The maintenance page as drawn: header, rows, chips, statuses, warnings, script verdicts.

use super::*;

#[test]
fn the_header_spans_the_width_and_the_page_ends_at_its_footer() {
    let f = draw(&page_of(info(true, Some("A"), Some("A")), "", &[]), (80, 24));
    assert_eq!(widest(&f.lines), frame_width(80));
    assert!(f.lines[0].contains("claude-auto \u{2500} maintenance"), "{}", f.lines[0]);
    assert_eq!(count(&f.lines, "Claude Code  2.1.230 \u{b7} newest"), 1, "{:#?}", f.lines);
    assert_eq!(f.lines.len(), 23, "the panels run to the footer (AC27)");
    assert!(f.lines.last().unwrap().contains("[esc] back"));
}

#[test]
fn an_older_install_says_an_update_is_available() {
    let mut p = page_of(info(false, None, None), "", &[]);
    p.installed = Some("2.1.226".into());
    let f = draw(&p, (100, 30));
    assert_eq!(count(&f.lines, "2.1.226 \u{b7} update available 2.1.230"), 1, "{:#?}", f.lines);
}

#[test]
fn builds_on_disk_read_as_words_and_hashes_stay_off() {
    let (a, b) = ("a1".repeat(32), "b2".repeat(32));
    let mut i = info(false, Some(&a), Some(&b));
    i.version_count = 12;
    i.versions_bytes = 3_650_722_201;
    let f = draw(&page_of(i, "", &[]), (60, MIN_HEIGHT + 8));
    assert_eq!(widest(&f.lines), frame_width(60));
    assert_eq!(count(&f.lines, "12 builds \u{b7} 3.4 GB"), 1, "{:#?}", f.lines);
    assert_eq!(count(&f.lines, "a1a1"), 0, "no hash on the page");
}

#[test]
fn a_finished_action_shows_its_reports_first_line_on_its_own_row() {
    let s = "Running: native (2.1.229)\nPlatform: win32-x64\n\nNo installation issues found.";
    let f = draw(&page_of(info(false, None, None), s, &[]), (100, 30));
    let row = f.lines.iter().find(|l| l.contains("[d] doctor")).unwrap();
    assert!(
        row.ends_with("Running: native (2.1.229)  \u{2502}"),
        "right-aligned on the row: {row:?}"
    );
    assert_eq!(count(&f.lines, "Last action"), 0);
}

#[test]
fn a_tall_status_never_outgrows_the_terminal() {
    let long: Vec<String> = (1..=40).map(|i| format!("status line {i}")).collect();
    let long = long.join("\n");
    for size in [(50, MIN_HEIGHT), (78, 24), (100, 30), (160, 45)] {
        let f = draw(&page_of(info(false, None, None), &long, &reindex()), size);
        assert!(f.lines.len() < size.1, "a spare row at {size:?}");
        assert!(widest(&f.lines) < size.0, "{size:?}");
        let last = f.map.rows.iter().map(|r| r.y).max().unwrap_or(0);
        assert!(
            usize::try_from(last).unwrap() < f.lines.len(),
            "no row map past the drawn rows at {size:?}"
        );
    }
}

#[test]
fn every_action_has_one_row_with_its_chip_and_name() {
    let f = draw(&page_of(info(true, None, None), "", &reindex()), (100, 24));
    let keys: Vec<String> = f.map.rows.iter().map(|r| r.name.clone()).collect();
    assert_eq!(keys, ["u", "r", "p", "d", "m", "l", "i"]);
    for (key, name) in [
        ('u', "update"),
        ('r', "update anyway (rename)"),
        ('p', "delete old builds"),
        ('d', "doctor"),
        ('m', "MCP servers"),
        ('l', "launch rows"),
        ('i', "full reindex"),
    ] {
        assert_eq!(count(&f.lines, &format!("[{key}] {name}")), 1, "{key}: {:#?}", f.lines);
    }
    let y = |k: &str| usize::try_from(f.map.rows.iter().find(|r| r.name == k).unwrap().y).unwrap();
    assert!(f.lines[y("m")].contains("[m] MCP servers"), "the row map points at the drawn row");
}

#[test]
fn only_the_selected_row_shows_its_hint() {
    let mut p = page_of(info(true, None, None), "", &reindex());
    p.selected = 1;
    let f = draw(&p, (120, 30));
    assert_eq!(count(&f.lines, "while claude"), 1, "{:#?}", f.lines);
    assert_eq!(count(&f.lines, "keeps the 2 newest"), 0, "no column of descriptions");
    let at = f.lines.iter().position(|l| l.contains("[r] update anyway")).unwrap();
    let under: String = f.lines[at + 1].chars().take(crate::cockpit::MAINTENANCE_LEFT_WIDTH).collect();
    assert!(
        under.contains("       while claude.exe is in use "),
        "a hint too long for its row sits whole under it: {under:?}"
    );
    let f = draw(&p, (160, 30));
    let p_row = f.lines.iter().find(|l| l.contains("[p] delete old builds")).unwrap();
    assert!(!p_row.contains("keeps"), "{p_row}");
}

#[test]
fn the_name_column_lines_up_on_every_row() {
    let f = draw(&page_of(info(true, None, None), "", &reindex()), (120, 30));
    let cols: Vec<usize> = f
        .map
        .rows
        .iter()
        .map(|r| {
            let line = &f.lines[usize::try_from(r.y).unwrap()];
            line.chars().position(|c| c == ']').unwrap()
        })
        .collect();
    assert!(cols.windows(2).all(|w| w[0] == w[1]), "{cols:?}");
}

#[test]
fn warnings_are_listed_with_the_place_to_fix_them() {
    let mut p = page_of(info(true, None, None), "", &[]);
    assert_eq!(count(&draw(&p, (120, 30)).lines, "warnings"), 0, "no warnings, no panel");
    p.warnings = vec!["config: unknown key \"colour\"".into(), "plugin git: describe timed out".into()];
    p.config_path = r"C:\Users\sample\.claude\claude-auto.json".into();
    let f = draw(&p, (160, 45));
    assert_eq!(count(&f.lines, "\u{26a0} config: unknown key \"colour\""), 1, "{:#?}", f.lines);
    assert_eq!(count(&f.lines, "fix it in C:\\Users\\sample\\.claude\\claude-auto.json"), 1);
    assert_eq!(count(&f.lines, "\u{26a0} plugin git: describe timed out"), 1);
}

#[test]
fn the_last_action_reads_done_failed_or_its_live_step() {
    let mut p = page_of(info(true, None, None), "", &[]);
    "MCP servers".clone_into(&mut p.last);
    for (phase, needle) in [
        (Phase::Running("checking\u{2026} \u{b7} 3 s".into()), "checking\u{2026} \u{b7} 3 s"),
        (Phase::Done, "done"),
        (Phase::Failed("1 of 2 connected".into()), "failed: 1 of 2 connected"),
    ] {
        p.phase = phase;
        let f = draw(&p, (120, 30));
        let row = f.lines.iter().find(|l| l.contains("[m] MCP servers")).unwrap();
        let left: String = row.chars().take(crate::cockpit::MAINTENANCE_LEFT_WIDTH).collect();
        assert!(left.ends_with(&format!("{needle}  \u{2502}")), "{needle}: {left:?}");
        assert_eq!(count(&f.lines, "Last action"), 0);
    }
    p.phase = Phase::Done;
    let colored = maintenance_frame(&p, (120, 30), Style { color: true, ascii: false });
    assert!(colored.lines.iter().any(|l| l.contains(&format!("{}done", crate::theme::GREEN))));
}

#[test]
fn an_installed_build_waiting_for_the_next_start_is_yellow_and_up_to_date_is_not() {
    let yellow = crate::theme::YELLOW;
    let colored = |launcher: &str| {
        let mut p = page_of(info(false, None, None), "", &[]);
        launcher.clone_into(&mut p.launcher);
        maintenance_frame(&p, (160, 40), Style { color: true, ascii: false })
            .lines
            .join("\n")
    };
    let waiting = colored("self-update notify, 9.9.9 installed - takes effect next start");
    assert!(
        waiting.contains(&format!("{yellow}9.9.9 installed - takes effect next start")),
        "{waiting:?}"
    );
    let current = colored("self-update notify, up to date");
    assert!(current.contains("up to date"), "{current:?}");
    assert!(!current.contains(&format!("{yellow}up to date")), "{current:?}");
}

#[test]
fn mcp_list_output_becomes_a_count_and_the_server_lines() {
    let out =
        "Checking MCP server health\u{2026}\n\nalpha: npx alpha - \u{2714} Connected\nbeta: http://x (HTTP) - \u{2717} Failed to connect";
    let v = mcp_summary(Verdict::done(out));
    assert!(!v.ok);
    assert_eq!(v.text.lines().next(), Some("1 of 2 connected"));
    assert_eq!(mcp_counts(&v), Some((1, 2)), "what the launch page keeps");
    assert_eq!(mcp_counts(&Verdict::done("No MCP servers configured.")), None);
    assert!(!v.text.contains("Checking"), "{}", v.text);
    let all = mcp_summary(Verdict::done("Checking\u{2026}\n\nalpha: npx alpha - \u{2714} Connected"));
    assert!(all.ok && all.text.starts_with("1 of 1 connected"), "{all:?}");
    let none = mcp_summary(Verdict::done("No MCP servers configured."));
    assert_eq!(none, Verdict::done("No MCP servers configured."));
}

/// The root cause of the stuck MCP check: `claude mcp list` exits while an MCP server it started
/// still holds the inherited output pipe. Here `cmd` exits at once and leaves `ping` holding it for
/// 40 s; the read must end about a second after the exit, past nextest's 30 s kill otherwise.
#[test]
fn a_grandchild_holding_the_pipe_never_holds_the_screen() {
    let mut seen = Vec::new();
    let mut step = |l: &str| seen.push(l.trim().to_owned());
    let (text, code) = crate::maintenance::captured_live(
        Path::new("cmd.exe"),
        &["/d", "/c", "start /b ping -n 40 127.0.0.1 >nul & echo checked"],
        &mut step,
    )
    .unwrap();
    assert_eq!(code, 0);
    assert!(text.contains("checked"), "{text}");
    assert!(seen.iter().any(|l| l == "checked"), "every line reaches the step line: {seen:?}");
}

#[test]
fn a_program_that_cannot_start_is_named_in_the_error() {
    let mut step = |_: &str| {};
    let err = crate::maintenance::captured_live(Path::new("claude-auto-no-such-program.exe"), &[], &mut step).unwrap_err();
    assert!(err.contains("claude-auto-no-such-program.exe"), "{err}");
}

#[test]
fn both_page_layouts_keep_the_frame_at_50_columns() {
    let long: Vec<String> = (1..=40).map(|i| format!("status line {i} with some words in it")).collect();
    for h in [24, 30] {
        let f = draw(&page_of(info(false, None, None), &long.join("\n"), &reindex()), (50, h));
        let text = f.lines.join("\n");
        for row in ["[u] update", "[l] launch rows", "[i] full reindex", "[esc] back"] {
            assert!(text.contains(row), "50x{h}: {row}");
        }
        assert!(f.lines[0].contains("maintenance"), "50x{h}: {}", f.lines[0]);
        assert!(f.lines.len() < h);
    }
}

#[test]
fn status_text_strips_every_invisible_character() {
    let e = '\u{1b}';
    let c = status_text(&format!("{e}[32mRunning: native (2.1.229){e}[0m\n{e}[2mPlatform: win32-x64{e}[0m"));
    assert!(!c.contains(e));
    assert_eq!(
        c.lines().collect::<Vec<_>>().join("|"),
        "Running: native (2.1.229)|Platform: win32-x64"
    );
    assert_eq!(status_text(&format!("{e}[2K{e}[1Gline one{e}[?25h")), "line one");
    assert_eq!(status_text("line\u{7}one").chars().count(), 7);
    let osc = status_text(&format!("{e}]0;window title\u{7}text"));
    assert!(osc.chars().all(|c| !c.is_control()));
}

#[test]
fn script_verdicts_never_read_exit_2_as_a_pass() {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let mut ok = |_: &Path| {
        Ok(ScriptRun {
            output: "fleet: 48/48 ok\nartifacts refreshed".into(),
            exit_code: 0,
        })
    };
    assert_eq!(
        run_script(&p, "full reindex", &mut ok).message,
        "full reindex finished green - fleet: 48/48 ok | artifacts refreshed"
    );
    let mut two = |_: &Path| {
        Ok(ScriptRun {
            output: "nothing".into(),
            exit_code: 2,
        })
    };
    assert!(run_script(&p, "x", &mut two).message.contains("COULD NOT RUN"));
    let mut one = |_: &Path| {
        Ok(ScriptRun {
            output: "boom".into(),
            exit_code: 1,
        })
    };
    assert!(run_script(&p, "x", &mut one).message.contains("FAILED"));
    let mut never = |_: &Path| -> Result<ScriptRun, String> { panic!("must not run") };
    let m = run_script(Path::new(r"C:\nope\x.ps1"), "q", &mut never);
    assert!(
        !m.ran && m.message.contains("not found"),
        "the existence check runs before any runner"
    );
}

#[test]
fn gib_rounds_like_n1() {
    assert_eq!(gib(900_000_000), "0.8");
    assert_eq!(gib(0), "0.0");
    assert_eq!(gib(3_650_722_201), "3.4");
    assert_eq!(gib(1500 * GIB), "1,500.0");
}

#[test]
fn every_built_in_row_says_what_it_does_and_only_a_configured_action_reads_as_one() {
    use crate::self_update::{ROLLBACK_KEY, UPDATE_KEY};
    let mut p = page_of(info(true, None, None), "", &reindex());
    p.entries = entries(
        &[(UPDATE_KEY, "update to 0.9.1".into()), (ROLLBACK_KEY, "roll back".into())],
        &reindex(),
    );
    for (i, e) in p.entries.clone().iter().enumerate() {
        p.selected = i;
        let text = draw(&p, (211, 52)).lines.join("\n");
        let generic = text.contains("a configured action");
        assert_eq!(generic, e.group == Group::Custom, "row {}: {text}", e.key);
    }
}
