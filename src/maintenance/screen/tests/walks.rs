//! The goldens and the shared walks: hover, hints, first and stray clicks, every size.

use super::*;

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
    assert!(lines[at("claude-auto  ")].contains("1.0.0 \u{b7} up to date"));
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
    // The walks try every cell: 280x72 lays out as 211x52 does, at twice the cells.
    for size in [(80, 24), (120, 30), (211, 52)] {
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

/// Every size a terminal can report: the maintenance page and the rows editor draw without a panic.
#[test]
fn maintenance_and_the_rows_editor_draw_at_every_size() {
    let long: Vec<String> = (1..=40).map(|i| format!("status line {i}")).collect();
    let page = page_of(info(false, None, None), &long.join("\n"), &reindex());
    let mut rows = RowsEditor::new(
        &RowsConfig {
            rows: Some(vec![RowName::Account, RowName::Effort]),
            ..RowsConfig::default()
        },
        Style { color: true, ascii: false },
    );
    for size in crate::ui::screen_loop::hover_walk::every_size() {
        let _ = draw(&page, size);
        let _ = rows.draw(size);
    }
}
