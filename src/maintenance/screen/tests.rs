use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use super::*;
use crate::frame::{MIN_HEIGHT, RowMap, frame_width};
use crate::input::{Event, MouseEvent};
use crate::layout::display_width;
use crate::ui::screen_loop::run;
use crate::ui::{Activation, PreviewTerm, UiStop};

const BIN: &str = r"C:\Users\sample-user\.local\bin\claude.exe";

fn info(matches: bool, ih: Option<&str>, nh: Option<&str>) -> InstallInfo {
    InstallInfo {
        bin_path: BIN.into(),
        installed_hash: ih.map(str::to_owned),
        newest_version: Some("2.1.230".into()),
        newest_hash: nh.map(str::to_owned),
        matches,
        version_count: 3,
        versions_bytes: 900_000_000,
        ..InstallInfo::default()
    }
}

fn plain() -> Style {
    Style {
        color: false,
        ascii: false,
    }
}

fn reindex() -> Vec<MaintenanceAction> {
    vec![MaintenanceAction {
        key: "i".into(),
        label: "full reindex".into(),
        script: concat!(env!("CARGO_MANIFEST_DIR"), r"\tests\fixtures\config-four.json").into(),
        confirm_twice: true,
    }]
}

/// The page as the screen builds it: built-in rows, then `actions`; `status` as the last
/// action's report (done) when not empty.
fn page_of(i: InstallInfo, status: &str, actions: &[MaintenanceAction]) -> Page {
    Page {
        info: i,
        entries: entries(&[], actions),
        last: "doctor".into(),
        phase: if status.is_empty() { Phase::Idle } else { Phase::Done },
        status: status.into(),
        hover: -1,
        ..Page::default()
    }
}

fn draw(p: &Page, size: (usize, usize)) -> Frame {
    maintenance_frame(p, size, plain())
}

fn widest(lines: &[String]) -> usize {
    lines
        .iter()
        .map(|l| display_width(&crate::theme::remove_ansi_color(l)))
        .max()
        .unwrap_or(0)
}

fn count(lines: &[String], needle: &str) -> usize {
    lines.iter().filter(|l| l.contains(needle)).count()
}

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

/// Counts what the keys did.
#[derive(Default)]
struct Log {
    runs: Vec<String>,
    prunes: usize,
    doctor: usize,
    updates: usize,
    rows: Vec<Vec<RowName>>,
    /// Config `rows` as the file holds it; every save lands here.
    config: Option<Vec<RowName>>,
    /// Config `hiddenValues` as the file holds it.
    hidden: std::collections::BTreeMap<RowName, String>,
}

struct FakeOps {
    log: Rc<RefCell<Log>>,
    exit_code: i32,
}

impl Ops for FakeOps {
    fn info(&mut self) -> InstallInfo {
        InstallInfo {
            matches: true,
            newest_version: Some("2.1.233".into()),
            installed_hash: Some("A".into()),
            newest_hash: Some("A".into()),
            version_count: 3,
            bin_path: "x".into(),
            ..InstallInfo::default()
        }
    }
    fn update(&mut self, step: Step<'_>) -> Verdict {
        step("downloading 45 %");
        self.log.borrow_mut().updates += 1;
        Verdict::done("updated")
    }
    fn rename_swap(&mut self) -> Verdict {
        Verdict::done("swapped")
    }
    fn command_text(&mut self, args: &[&str], _step: Step<'_>) -> Verdict {
        self.log.borrow_mut().doctor += 1;
        Verdict::done(format!("doctor report for {}", args.join(" ")))
    }
    fn prune(&mut self) -> Pruned {
        self.log.borrow_mut().prunes += 1;
        Pruned::default()
    }
    fn run(&mut self, script: &Path, _step: Step<'_>) -> Result<ScriptRun, String> {
        self.log.borrow_mut().runs.push(script.display().to_string());
        Ok(ScriptRun {
            output: "fleet: 48/48 ok\nartifacts refreshed".into(),
            exit_code: self.exit_code,
        })
    }
    fn load_rows(&mut self) -> RowsConfig {
        let log = self.log.borrow();
        RowsConfig {
            rows: log.config.clone(),
            hidden_values: log.hidden.clone(),
            ..RowsConfig::default()
        }
    }
    fn save_rows(&mut self, rows: &[RowName], hidden: &BTreeMap<RowName, String>) -> Result<(), String> {
        let mut log = self.log.borrow_mut();
        log.rows.push(rows.to_vec());
        log.config = Some(rows.to_vec());
        hidden.clone_into(&mut log.hidden);
        Ok(())
    }
}

/// Compares with `tests/fixtures/wide-frames/<name>.txt`; `CLAUDE_AUTO_BLESS` rewrites it.
fn assert_golden(name: &str, actual: &str) {
    let path = format!("{}/tests/fixtures/wide-frames/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("CLAUDE_AUTO_BLESS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
    assert_eq!(actual, expected, "{name}");
}

#[test]
fn cockpit_goldens_at_130x40_80x24_and_for_a_new_user() {
    let mut p = page_of(
        info(true, None, None),
        "Current version: 2.1.230\nChecking for updates...\nClaude Code is up to date (2.1.230)",
        &reindex(),
    );
    "update".clone_into(&mut p.last);
    p.phase = Phase::Done;
    p.warnings = vec![
        "config: unknown key \"colour\"".into(),
        "custom action 'sync': command not found".into(),
    ];
    p.config_path = r"C:\Users\sample\.claude\claude-auto.json".into();
    let wide = draw(&p, (130, 40)).lines;
    assert_golden("maintenance-130x40", &wide.join("\n"));
    assert!(wide.iter().any(|l| l.contains("update \u{b7} last run")) && wide.iter().any(|l| l.contains("error")));
    assert_golden("maintenance-80x24", &draw(&p, (80, 24)).lines.join("\n"));
    let three = draw(&p, (250, 70)).lines;
    assert_golden("maintenance-250x70", &three.join("\n"));
    let versions = three.iter().find(|l| l.contains("installed")).unwrap();
    assert!(
        versions.ends_with('\u{2502}') && display_width(versions) == 249,
        "the facts close the page: {versions}"
    );
    let value_end = display_width(versions.split_once("newest").unwrap().0) + 6;
    assert!(
        value_end > 249 - 66,
        "the versions sit in the last 66 cells, beside the last run: {versions}"
    );
    let s1 = draw(&page_of(info(true, None, None), "", &[]), (130, 40)).lines;
    assert_golden("maintenance-130x40-s1", &s1.join("\n"));
    assert_eq!(count(&s1, "warnings"), 0, "an empty warnings panel is not drawn");
    assert_eq!(count(&s1, "Custom"), 0, "an empty group is not drawn");
}

#[test]
fn a_narrow_window_keeps_every_fact_and_moves_a_long_hint_under_its_row() {
    let mut p = page_of(
        info(true, None, None),
        "Current version: 2.1.230\nClaude Code is up to date (2.1.230)",
        &reindex(),
    );
    p.entries = entries(&[('b', "roll back claude-auto".into())], &reindex());
    "update".clone_into(&mut p.last);
    p.phase = Phase::Done;
    "self-update notify, up to date".clone_into(&mut p.launcher);
    p.selected = 1;
    let lines: Vec<String> = draw(&p, (55, 20)).lines;
    assert_golden("maintenance-55x20", &lines.join("\n"));
    let at = |needle: &str| {
        lines
            .iter()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("{needle}: {lines:#?}"))
    };
    assert_eq!(
        at("while claude.exe is in use"),
        at("[r] update anyway") + 1,
        "the hint under its row, whole"
    );
    assert_eq!(
        at("Current version: 2.1.230"),
        at("[u] update") + 1,
        "the last run under the action that ran"
    );
    assert!(lines[at("Claude Code")].contains("2.1.230 \u{b7} newest \u{b7} 3 builds"));
    assert!(lines[at("claude-auto  ")].contains("0.9.0 \u{b7} up to date"));
    assert!(lines.iter().all(|l| display_width(l) <= 54));
}

#[test]
fn the_grouped_page_matches_its_goldens() {
    let mut p = page_of(info(true, None, None), "", &reindex());
    p.selected = 1;
    p.hover_row = 4;
    p.warnings = vec!["config: unknown key \"colour\"".into()];
    p.config_path = r"C:\Users\sample\.claude\claude-auto.json".into();
    assert_golden("maintenance-130x45", &draw(&p, (130, 45)).lines.join("\n"));
    let colored = maintenance_frame(&p, (130, 45), Style { color: true, ascii: false });
    let escaped: Vec<String> = colored.lines.iter().map(|l| l.replace('\u{1b}', "\\e")).collect();
    assert_golden("maintenance-130x45-hover-color", &escaped.join("\n"));
}

/// The shared hover rule over every compact screen: launch, maintenance, rows editor.
#[test]
fn every_clickable_region_hovers_and_never_selects() {
    use crate::ui::screen_loop::hover_walk::assert_every_region_hovers;
    let color = Style { color: true, ascii: false };
    let mut ops = FakeOps {
        log: Rc::new(RefCell::new(Log::default())),
        exit_code: 0,
    };
    let actions = reindex();
    let mut m = MaintScreen::new(&mut ops, &actions, color);
    assert_every_region_hovers(&mut m, (130, 45));
    let mut rows = RowsEditor::new(
        &RowsConfig {
            rows: Some(vec![RowName::Account, RowName::Effort]),
            ..RowsConfig::default()
        },
        color,
    );
    assert_every_region_hovers(&mut rows, (130, 45));
    let mut roster = crate::launch::tests::four();
    let view = crate::launch::screen::LaunchView {
        limits: crate::types::Limits::new(),
        version: VersionInfo::default(),
        default_model_label: "default".into(),
        default_advisor_label: "default".into(),
        labels_by_account: BTreeMap::new(),
        running: Vec::new(),
        mcp: String::new(),
        style: color,
    };
    let prefs = crate::prefs::Prefs::default();
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = roster.new_state();
    let mut launch = crate::launch::screen::LaunchScreen::new(&mut roster, st, &view, &prefs, 0, &mut hook);
    assert_every_region_hovers(&mut launch, (130, 45));
}

fn launch_view(color: Style) -> crate::launch::screen::LaunchView {
    crate::launch::screen::LaunchView {
        limits: crate::types::Limits::new(),
        version: VersionInfo::default(),
        default_model_label: "default".into(),
        default_advisor_label: "default".into(),
        labels_by_account: BTreeMap::new(),
        running: Vec::new(),
        mcp: String::new(),
        style: color,
    }
}

/// Every footer hint of maintenance, the rows editor and the launch screen is alive at every size.
#[test]
fn every_compact_screen_hint_is_alive() {
    use crate::ui::screen_loop::hover_walk::assert_every_hint_is_alive;
    let color = Style { color: true, ascii: false };
    let actions = reindex();
    for size in [(80, 24), (120, 30), (211, 52), (280, 72)] {
        let mut make = || {
            let ops = Box::leak(Box::new(FakeOps {
                log: Rc::new(RefCell::new(Log::default())),
                exit_code: 0,
            }));
            MaintScreen::new(ops, &actions, color)
        };
        assert_every_hint_is_alive(&mut make, size, &|| 0);
        let mut make = || {
            let mut r = RowsEditor::new(
                &RowsConfig {
                    rows: Some(vec![RowName::Account, RowName::Effort, RowName::Mode]),
                    ..RowsConfig::default()
                },
                color,
            );
            *r.cursor() = Cursor::at(1);
            r
        };
        assert_every_hint_is_alive(&mut make, size, &|| 0);
        let opened: &'static std::cell::Cell<usize> = Box::leak(Box::new(std::cell::Cell::new(0)));
        let view: &'static _ = Box::leak(Box::new(launch_view(color)));
        let prefs: &'static _ = Box::leak(Box::new(crate::prefs::Prefs::default()));
        // A launch screen borrows its roster and hook mutably: each hint gets fresh ones (leaked,
        // a test's few bytes).
        let mut make = || {
            let roster = Box::leak(Box::new(crate::launch::tests::four()));
            let hook: &'static mut crate::launch::screen::OnKey<'static> = Box::leak(Box::new(|k: &KeyPress, _: &mut dyn Term| {
                let hit = crate::input::is_hotkey(k, 'u');
                opened.set(opened.get() + usize::from(hit));
                hit
            }));
            let st = roster.new_state();
            crate::launch::screen::LaunchScreen::new(roster, st, view, prefs, 0, hook)
        };
        assert_every_hint_is_alive(&mut make, size, &|| opened.get());
    }
}

/// The click rule over every compact screen at every size: a first click only selects, a click
/// on no target (a border, the column beside, past the frame) changes nothing.
#[test]
fn a_first_click_only_selects_and_a_stray_click_does_nothing() {
    use crate::ui::screen_loop::hover_walk::{assert_first_click_never_acts, assert_no_stray_click_acts};
    let color = Style { color: true, ascii: false };
    for size in [(80, 24), (120, 30), (211, 52), (280, 72)] {
        let mut ops = FakeOps {
            log: Rc::new(RefCell::new(Log::default())),
            exit_code: 0,
        };
        let actions = reindex();
        let mut m = MaintScreen::new(&mut ops, &actions, color);
        assert_no_stray_click_acts(&mut m, size);
        let mut rows = RowsEditor::new(
            &RowsConfig {
                rows: Some(vec![RowName::Account, RowName::Effort]),
                ..RowsConfig::default()
            },
            color,
        );
        assert_no_stray_click_acts(&mut rows, size);
        assert_first_click_never_acts(&mut rows, size);
        let mut roster = crate::launch::tests::four();
        let view = launch_view(color);
        let prefs = crate::prefs::Prefs::default();
        let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
        let st = roster.new_state();
        let mut launch = crate::launch::screen::LaunchScreen::new(&mut roster, st, &view, &prefs, 0, &mut hook);
        assert_no_stray_click_acts(&mut launch, size);
        assert_first_click_never_acts(&mut launch, size);
    }
}

#[test]
fn l_saves_a_hidden_rows_value_and_names_it_in_the_status() {
    let log = Rc::new(RefCell::new(Log {
        config: Some(vec![RowName::Account, RowName::Effort]),
        ..Log::default()
    }));
    let mut ops = FakeOps {
        log: Rc::clone(&log),
        exit_code: 0,
    };
    let mut s = MaintScreen::new(&mut ops, &[], plain());
    // Rows: account, effort, then the hidden ones in default order (remote, model, ...).
    let keys = crate::ui::parse_preview_keys("l,s,s,s,d,d,d,Enter,Escape").unwrap();
    let mut t = PreviewTerm::new((100, 30), keys, true);
    assert!(run(&mut s, &mut t).unwrap().is_none());
    assert_eq!(log.borrow().hidden.get(&RowName::Model).map(String::as_str), Some("opus1m"));
    assert!(s.page.status.contains("hidden model -> opus1m"), "{}", s.page.status);
}

#[test]
fn l_edits_the_launch_rows_and_saves_them() {
    let log = Rc::new(RefCell::new(Log {
        config: Some(vec![RowName::Account, RowName::Model, RowName::Mode]),
        ..Log::default()
    }));
    let mut ops = FakeOps {
        log: Rc::clone(&log),
        exit_code: 0,
    };
    let mut s = MaintScreen::new(&mut ops, &[], plain());
    let keys = crate::ui::parse_preview_keys("l,s,s,Spacebar,Enter,l,Enter,Escape").unwrap();
    let mut t = PreviewTerm::new((100, 30), keys, true);
    assert!(run(&mut s, &mut t).unwrap().is_none());
    assert!(s.page.status.contains("launch rows saved: account, model"), "{}", s.page.status);
    let saved = vec![RowName::Account, RowName::Model];
    assert_eq!(log.borrow().rows, [saved.clone(), saved], "the next open starts from the save");
}

#[test]
fn the_rows_editor_opens_on_the_config_as_it_is_now() {
    let log = Rc::new(RefCell::new(Log {
        config: Some(vec![RowName::Account, RowName::Mode]),
        ..Log::default()
    }));
    let mut ops = FakeOps {
        log: Rc::clone(&log),
        exit_code: 0,
    };
    let mut s = MaintScreen::new(&mut ops, &[], plain());
    let keys = crate::ui::parse_preview_keys("l,Enter,Escape").unwrap();
    let mut t = PreviewTerm::new((100, 30), keys, true);
    assert!(run(&mut s, &mut t).unwrap().is_none());
    assert_eq!(
        log.borrow().rows,
        [vec![RowName::Account, RowName::Mode]],
        "another launcher's save is what the editor starts from"
    );
}

#[test]
fn only_a_plain_l_opens_the_rows_editor() {
    let shifted = Event::Key(KeyPress {
        shift: true,
        ..KeyPress::char('L')
    });
    // psmux drops the Caps Lock flag: a Latin letter then arrives as a bare upper-case char.
    let bare_upper = Event::Key(KeyPress::char('L'));
    for stray in [shifted, bare_upper] {
        let (_, status, _, r) = go(vec![stray, esc()], &[], vec![], 0, (100, 24));
        assert_eq!(r, Ok(None), "Escape left the maintenance screen, no editor took it");
        assert!(!status.contains("Last action"), "{status}");
    }
    let caps = Event::Key(KeyPress {
        caps_lock: true,
        ..KeyPress::char('L')
    });
    for press in [key('l'), caps] {
        let (_, status, _, r) = go(vec![press, esc(), esc()], &[], vec![], 0, (100, 24));
        assert_eq!(r, Ok(None));
        assert!(status.contains("launch rows unchanged"), "{status}");
    }
}

#[test]
fn an_error_inside_the_rows_editor_still_ends_the_screen() {
    // The editor's wait fails (keys exhausted, as a lost console would): the screen's own next
    // wait fails the same way, so the error reaches the caller.
    let (.., r) = go(vec![key('l')], &[], vec![], 0, (100, 24));
    assert_eq!(r, Err(UiStop::KeysExhausted));
}

/// Scripted events with an injectable record clock; frames and statuses recorded.
struct T {
    inner: PreviewTerm,
    q: VecDeque<Event>,
    clock: Vec<Option<u64>>,
    now: Option<u64>,
    frames: Vec<String>,
    drains: usize,
    size: (usize, usize),
}

impl Term for T {
    fn size(&mut self) -> (usize, usize) {
        self.size
    }
    fn paint(&mut self, lines: &[String]) {
        self.frames.push(lines.join("\n"));
    }
    fn wait(&mut self, _s: (usize, usize)) -> Result<Event, UiStop> {
        if !self.clock.is_empty() {
            self.now = self.clock.remove(0);
        }
        self.q.pop_front().ok_or(UiStop::KeysExhausted)
    }
    fn window_top(&mut self) -> i32 {
        0
    }
    fn input_pending(&mut self) -> bool {
        false
    }
    fn record_time(&self) -> Option<u64> {
        self.now
    }
    fn last_activation(&mut self) -> &mut Option<Activation> {
        self.inner.last_activation()
    }
    fn drain(&mut self) {
        self.drains += 1;
    }
    fn logger(&self) -> &crate::log::Logger {
        self.inner.logger()
    }
    fn read_line(&mut self, _p: &str) -> String {
        String::new()
    }
}

fn key(c: char) -> Event {
    Event::Key(KeyPress::char(c))
}

fn esc() -> Event {
    Event::Key(KeyPress::named(vk::ESCAPE))
}

fn click(x: i32, y: i32, double: bool) -> Event {
    Event::Mouse(MouseEvent {
        x,
        y,
        left: true,
        buttons: 1,
        is_double_click: double,
        ..MouseEvent::default()
    })
}

/// Runs the screen over `events`; returns (log, the last frame painted, term, result).
fn go(
    events: Vec<Event>,
    actions: &[MaintenanceAction],
    clock: Vec<Option<u64>>,
    exit_code: i32,
    size: (usize, usize),
) -> (Log, String, T, Result<Option<()>, UiStop>) {
    go_timed(events, actions, clock, exit_code, size, crate::timing::Timings::default())
}

fn go_timed(
    events: Vec<Event>,
    actions: &[MaintenanceAction],
    clock: Vec<Option<u64>>,
    exit_code: i32,
    size: (usize, usize),
    timings: crate::timing::Timings,
) -> (Log, String, T, Result<Option<()>, UiStop>) {
    let log = Rc::new(RefCell::new(Log::default()));
    let mut ops = FakeOps {
        log: Rc::clone(&log),
        exit_code,
    };
    let mut t = T {
        inner: PreviewTerm::new(size, VecDeque::new(), true).with_logger(crate::log::Logger::silent(timings)),
        q: events.into(),
        clock,
        now: None,
        frames: Vec::new(),
        drains: 0,
        size,
    };
    let mut s = MaintScreen::new(&mut ops, actions, Style { color: true, ascii: false });
    let r = run(&mut s, &mut t);
    drop(s);
    // The last frame without colour: what a reader sees.
    let status = crate::theme::remove_ansi_color(&t.frames.last().cloned().unwrap_or_default());
    let log = std::mem::take(&mut *log.borrow_mut());
    (log, status, t, r)
}

fn map_at(size: (usize, usize), actions: &[MaintenanceAction]) -> RowMap {
    let mut o = FakeOps {
        log: Rc::default(),
        exit_code: 0,
    };
    draw(&page_of(o.info(), "", actions), size).map
}

fn footer_span(map: &RowMap, key: &str) -> crate::frame::FooterSpan {
    map.footer.iter().find(|s| s.key == key).unwrap().clone()
}

#[test]
fn ctrl_c_and_escape_leave() {
    let ctrl_c = Event::Key(KeyPress {
        vk: vk::C,
        ctrl: true,
        ch: '\u{3}',
        ..KeyPress::default()
    });
    assert_eq!(go(vec![ctrl_c], &[], vec![], 0, (100, 24)).3, Ok(None));
    assert_eq!(go(vec![esc()], &[], vec![], 0, (100, 24)).3, Ok(None));
}

#[test]
fn clicking_esc_back_leaves_and_one_column_past_it_does_not() {
    let m = map_at((100, 24), &reindex());
    let e = footer_span(&m, "Escape");
    let y = m.footer_y.unwrap() + i32::try_from(e.line).unwrap();
    assert_eq!(go(vec![click(e.start, y, false)], &reindex(), vec![], 0, (100, 24)).3, Ok(None));
    assert_eq!(
        go(vec![click(e.end + 1, y, false)], &reindex(), vec![], 0, (100, 24)).3,
        Err(UiStop::KeysExhausted)
    );
}

#[test]
fn hovering_buttons_paints_three_distinct_frames() {
    let m = map_at((100, 24), &reindex());
    let y = m.footer_y.unwrap();
    let mv = |s: &crate::frame::FooterSpan| {
        Event::Mouse(MouseEvent {
            x: s.start,
            y: y + i32::try_from(s.line).unwrap(),
            is_move: true,
            ..MouseEvent::default()
        })
    };
    let (_, _, t, _) = go(
        vec![mv(&footer_span(&m, "Enter")), mv(&footer_span(&m, "Escape")), esc()],
        &reindex(),
        vec![],
        0,
        (100, 24),
    );
    assert_eq!(t.frames.len(), 3);
    assert_ne!(t.frames[0], t.frames[1]);
    assert_ne!(t.frames[1], t.frames[2]);
    assert!(
        t.frames[1].contains(crate::theme::ACCENT_BG),
        "the hovered button gets the accent cap"
    );
    assert!(!t.frames[0].contains(crate::theme::ACCENT_BG));
}

#[test]
fn debug_mode_draws_the_maintenance_screen_exactly_as_off() {
    let ev = || vec![key('i'), key('i'), esc()];
    let on = crate::timing::Timings::on(std::time::Instant::now());
    let (_, _, t_on, _) = go_timed(ev(), &reindex(), vec![], 0, (100, 24), on);
    let (_, _, t_off, _) = go(ev(), &reindex(), vec![], 0, (100, 24));
    assert!(t_off.frames.len() > 1, "the screen painted before and after the command");
    assert_eq!(t_on.frames, t_off.frames);
}

#[test]
fn one_press_only_warns_the_second_runs_once() {
    let (log, status, _, _) = go(vec![key('i'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.runs.len(), 0);
    assert!(status.contains("confirm: press i"));
    let (log, status, t, _) = go(vec![key('i'), key('i'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.runs.len(), 1);
    assert!(status.contains("full reindex") && status.contains("finished green"), "{status}");
    assert_eq!(t.drains, 1, "the input queue is drained after a command that owned the screen");
    assert!(
        t.frames.iter().any(|f| crate::theme::remove_ansi_color(f).contains("running")),
        "a running frame first"
    );
}

#[test]
fn a_click_selects_a_row_and_enter_runs_it() {
    let m = map_at((100, 24), &[]);
    let d = m.rows.iter().find(|r| r.name == "d").unwrap();
    let enter = Event::Key(KeyPress::named(vk::ENTER));
    let (log, status, ..) = go(vec![click(4, d.y, false), enter, esc()], &[], vec![], 0, (100, 24));
    assert_eq!(log.doctor, 1, "{status}");
    let (log, ..) = go(vec![click(4, d.y, false), esc()], &[], vec![], 0, (100, 24));
    assert_eq!(log.doctor, 0, "a click alone runs nothing");
}

#[test]
fn beside_the_rows_a_wide_page_shows_what_the_selected_action_does_and_how_to_run_it() {
    let mut ops = FakeOps {
        log: Rc::new(RefCell::new(Log::default())),
        exit_code: 0,
    };
    let actions = reindex();
    let mut m = MaintScreen::new(
        &mut ops,
        &actions,
        Style {
            color: false,
            ascii: false,
        },
    );
    let text = |m: &mut MaintScreen<'_>, at: i32| {
        *m.cursor() = Cursor::at(at);
        m.before();
        m.draw((211, 52)).lines.join("\n")
    };
    let first = text(&mut m, 0);
    assert!(
        first.contains("─ update ─") && first.contains("downloads the newest Claude Code"),
        "{first}"
    );
    assert!(first.contains("enter  or  u  runs it"), "{first}");
    let doctor = m.page.entries.iter().position(|e| e.key == 'd').unwrap();
    let d = text(&mut m, i32::try_from(doctor).unwrap());
    assert!(d.contains("runs claude doctor") && d.contains("enter  or  d  runs it"), "{d}");
    let narrow = {
        *m.cursor() = Cursor::at(0);
        m.before();
        m.draw((100, 30)).lines.join("\n")
    };
    assert!(!narrow.contains("runs it"), "stacked, the details stay off the page: {narrow}");
}

#[test]
fn a_second_click_on_the_selected_row_runs_it_and_a_click_on_another_only_selects() {
    let m = map_at((100, 24), &[]);
    let d = m.rows.iter().find(|r| r.name == "d").unwrap();
    let other = m.rows.iter().find(|r| r.name != "d").unwrap();
    let (log, status, ..) = go(vec![click(4, d.y, false), click(4, d.y, false), esc()], &[], vec![], 0, (100, 24));
    assert_eq!(log.doctor, 1, "{status}");
    let (log, ..) = go(
        vec![click(4, other.y, false), click(4, d.y, false), esc()],
        &[],
        vec![],
        0,
        (100, 24),
    );
    assert_eq!(log.doctor, 0, "the first click on a row only selects it");
}

#[test]
fn w_and_s_move_the_selection_when_no_row_owns_them() {
    let enter = || Event::Key(KeyPress::named(vk::ENTER));
    // Rows: u, r, p, d - three moves down land on doctor.
    let (log, ..) = go(vec![key('s'), key('s'), key('s'), enter(), esc()], &[], vec![], 0, (100, 24));
    assert_eq!(log.doctor, 1);
    let four_down_one_up = vec![key('s'), key('s'), key('s'), key('s'), key('w'), enter(), esc()];
    assert_eq!(go(four_down_one_up, &[], vec![], 0, (100, 24)).0.doctor, 1);
}

#[test]
fn exit_codes_reach_the_status() {
    assert!(
        go(vec![key('i'), key('i'), esc()], &reindex(), vec![], 2, (100, 24))
            .1
            .contains("COULD NOT RUN")
    );
    assert!(
        go(vec![key('i'), key('i'), esc()], &reindex(), vec![], 1, (100, 24))
            .1
            .contains("FAILED")
    );
}

#[test]
fn uppercase_and_sgr_terminators_never_press_a_key() {
    let upper = Event::Key(KeyPress::char('I'));
    assert_eq!(go(vec![upper, upper, esc()], &reindex(), vec![], 0, (100, 24)).0.runs.len(), 0);
    let sgr_m = Event::Key(KeyPress {
        ch: 'M',
        vk: 0x4D,
        shift: true,
        ..KeyPress::default()
    });
    let (log, ..) = go(vec![sgr_m, sgr_m, esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.doctor, 0, "two SGR terminators run nothing");
}

#[test]
fn the_cyrillic_letter_on_the_i_key_runs_the_action() {
    let ru_i = Event::Key(KeyPress {
        ch: '\u{0448}',
        vk: 0x49,
        ..KeyPress::default()
    });
    assert_eq!(go(vec![ru_i, ru_i, esc()], &reindex(), vec![], 0, (100, 24)).0.runs.len(), 1);
}

#[test]
fn actions_come_from_the_config() {
    let (_, status, ..) = go(vec![key('i'), esc()], &[], vec![], 0, (100, 24));
    assert!(!status.contains("confirm"), "a key with no action does nothing");
    let mut one_shot = reindex();
    one_shot[0].key = "x".into();
    one_shot[0].label = "quick".into();
    one_shot[0].confirm_twice = false;
    assert!(
        go(vec![key('x'), esc()], &one_shot, vec![], 0, (100, 24))
            .1
            .contains("quick finished green")
    );
}

#[test]
fn the_confirm_is_state_not_a_parse_of_the_status() {
    let (log, status, ..) = go(vec![key('z'), esc()], &reindex(), vec![], 0, (100, 24));
    assert!(log.runs.is_empty() && !status.contains("confirm"));
    let (log, status, ..) = go(vec![key('i'), key('z'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.runs.len(), 0);
    assert!(
        !status.contains("confirm"),
        "the stale confirm text is cleared by the unrelated key"
    );
    let (log, status, ..) = go(vec![key('i'), key('p'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!((log.prunes, log.runs.len()), (0, 0), "one confirm never arms another key");
    assert!(status.contains("confirm: press p again"));
    let mut two = reindex();
    two.push(MaintenanceAction {
        key: "x".into(),
        label: "quick".into(),
        script: two[0].script.clone(),
        confirm_twice: true,
    });
    let (log, status, ..) = go(vec![key('i'), key('x'), key('x'), esc()], &two, vec![], 0, (100, 24));
    assert_eq!(log.runs.len(), 1);
    assert!(status.contains("quick finished green"));
}

fn clock(step: u64, n: usize) -> Vec<Option<u64>> {
    (1..=n as u64).map(|i| Some(1000 + i * step)).collect()
}

#[test]
fn a_paste_is_not_two_presses() {
    let (log, status, ..) = go(vec![key('p'), key('p'), esc()], &reindex(), clock(10, 3), 0, (100, 24));
    assert_eq!(log.prunes, 0, "two p 10 ms apart are a paste");
    assert!(status.contains("confirm: press p again"));
    assert_eq!(
        go(vec![key('p'), key('p'), esc()], &reindex(), clock(400, 3), 0, (100, 24))
            .0
            .prunes,
        1
    );
    assert_eq!(
        go(vec![key('i'), key('i'), esc()], &reindex(), clock(10, 3), 0, (100, 24))
            .0
            .runs
            .len(),
        0
    );
    assert_eq!(
        go(vec![key('i'), key('i'), esc()], &reindex(), clock(400, 3), 0, (100, 24))
            .0
            .runs
            .len(),
        1
    );
    let four = vec![key('p'), key('p'), key('p'), key('p'), esc()];
    assert_eq!(
        go(four, &reindex(), clock(10, 5), 0, (100, 24)).0.prunes,
        0,
        "a long paste only re-arms"
    );
    assert_eq!(
        go(vec![key('p'), key('p'), esc()], &reindex(), vec![], 0, (100, 24)).0.prunes,
        1,
        "no stamp leaves the gate inert"
    );
}

#[test]
fn d_is_doctor_and_w_s_are_not_arrows() {
    let (log, status, ..) = go(vec![key('d'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.doctor, 1);
    assert!(status.contains("doctor report"));
    let (log, status, ..) = go(vec![key('w'), key('s'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.doctor, 0);
    assert!(!status.contains("doctor report"));
    let mut ws = reindex();
    ws[0] = MaintenanceAction {
        key: "s".into(),
        label: "sweep".into(),
        script: ws[0].script.clone(),
        confirm_twice: false,
    };
    ws.push(MaintenanceAction {
        key: "w".into(),
        label: "warm".into(),
        script: ws[0].script.clone(),
        confirm_twice: false,
    });
    assert_eq!(go(vec![key('s'), key('w'), esc()], &ws, vec![], 0, (100, 24)).0.runs.len(), 2);
}

#[test]
fn update_draws_a_running_frame_and_drains() {
    let (log, status, t, _) = go(vec![key('u'), esc()], &[], vec![], 0, (100, 24));
    assert_eq!(log.updates, 1);
    assert!(status.contains("updated"));
    assert!(t.frames.iter().any(|f| crate::theme::remove_ansi_color(f).contains(" u  update")));
    assert!(
        t.frames
            .iter()
            .any(|f| crate::theme::remove_ansi_color(f).contains("updating \u{b7} downloading 45 %")),
        "every line the updater prints reaches the step line"
    );
    assert_eq!(t.drains, 1);
}

/// `FakeOps` with one launcher entry, as a known newer release gives.
struct WithLauncher(FakeOps);

impl Ops for WithLauncher {
    fn info(&mut self) -> InstallInfo {
        self.0.info()
    }
    fn update(&mut self, step: Step<'_>) -> Verdict {
        self.0.update(step)
    }
    fn rename_swap(&mut self) -> Verdict {
        self.0.rename_swap()
    }
    fn command_text(&mut self, args: &[&str], step: Step<'_>) -> Verdict {
        self.0.command_text(args, step)
    }
    fn prune(&mut self) -> Pruned {
        self.0.prune()
    }
    fn run(&mut self, script: &Path, step: Step<'_>) -> Result<ScriptRun, String> {
        self.0.run(script, step)
    }
    fn load_rows(&mut self) -> RowsConfig {
        self.0.load_rows()
    }
    fn save_rows(&mut self, rows: &[RowName], hidden: &BTreeMap<RowName, String>) -> Result<(), String> {
        self.0.save_rows(rows, hidden)
    }
    fn launcher_entries(&mut self) -> Vec<(char, String)> {
        vec![('c', "update claude-auto to 9.9.9".into())]
    }
}

#[test]
fn the_running_frame_keeps_the_launcher_rows() {
    let mut ops = WithLauncher(FakeOps {
        log: Rc::default(),
        exit_code: 0,
    });
    let mut t = T {
        inner: PreviewTerm::new((100, 30), VecDeque::new(), true),
        q: vec![key('u')].into(),
        clock: Vec::new(),
        now: None,
        frames: Vec::new(),
        drains: 0,
        size: (100, 30),
    };
    let mut s = MaintScreen::new(&mut ops, &[], plain());
    assert_eq!(run(&mut s, &mut t), Err(UiStop::KeysExhausted));
    let running = t.frames.iter().find(|f| f.contains("updating")).unwrap();
    assert!(running.contains("update claude-auto to 9.9.9"), "{running}");
}

/// AC27: at every size the panels run to the footer row; nothing is left blank under them.
#[test]
fn the_panels_run_to_the_footer_at_every_size() {
    for status in ["", "doctor: all good"] {
        let p = page_of(info(true, Some("A"), Some("A")), status, &[]);
        for size in [(80, 24), (100, 30), (120, 30), (160, 45), (211, 52), (280, 72)] {
            let f = draw(&p, size);
            let fy = usize::try_from(f.map.footer_y.unwrap()).unwrap();
            assert_eq!(fy, size.1 - 2, "{size:?}: the footer sits on the last row: {:#?}", f.lines);
            let above = &f.lines[fy - 1];
            assert!(
                above.starts_with('\u{2570}'),
                "{size:?}: a panel closes right above the footer: {above}"
            );
            assert_eq!(display_width(above), size.0 - 1, "{size:?}: every column closes there: {above}");
        }
    }
}
