//! The launch screen's tests: keys, clicks, rows editing, the launch state it ends with.
#![expect(clippy::many_single_char_names, reason = "short fixture names")]

use super::*;
use crate::input::{Event, MouseEvent};
use crate::launch::tests::four;
use crate::ui::PreviewTerm;
use crate::ui::screen_loop::run;

fn view() -> LaunchView {
    LaunchView {
        limits: Limits::new(),
        version: VersionInfo::default(),
        default_model_label: "default".into(),
        default_advisor_label: "default".into(),
        labels_by_account: BTreeMap::new(),
        running: Vec::new(),
        mcp: String::new(),
        style: Style { color: false, ascii: true },
    }
}

fn drive(keys: &str) -> Option<LaunchState> {
    let mut r = four();
    let v = view();
    let p = Prefs::default();
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = r.new_state();
    let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
    let mut t = PreviewTerm::new((100, 30), crate::ui::parse_preview_keys(keys).unwrap(), true);
    run(&mut s, &mut t).unwrap()
}

/// The frame a scripted run ends on (its last), in colour.
fn last_frame(events: Vec<Event>) -> Vec<String> {
    let mut r = four();
    let v = LaunchView {
        style: Style { color: true, ascii: false },
        ..view()
    };
    let p = Prefs::default();
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = r.new_state();
    let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
    let mut t = crate::ui::script_term::ScriptTerm::new((130, 40), events);
    assert!(run(&mut s, &mut t).unwrap().is_none());
    t.inner.frames.unwrap().last().unwrap().clone()
}

#[test]
fn a_value_a_click_selects_draws_as_the_arrows_draw_it() {
    let mut r = four();
    let v = LaunchView {
        style: Style { color: true, ascii: false },
        ..view()
    };
    let p = Prefs::default();
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = r.new_state();
    let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
    let frame = s.draw((130, 40));
    let esc = || Event::Key(KeyPress::named(vk::ESCAPE));
    let mouse = |x: i32, y: i32, is_move: bool| {
        Event::Mouse(MouseEvent {
            x,
            y,
            left: !is_move,
            buttons: u32::from(!is_move),
            is_move,
            ..MouseEvent::default()
        })
    };
    for (row, value, keys) in [
        ("Account", "personal", vec!["RightArrow"]),
        ("Effort", "high", vec!["DownArrow", "DownArrow", "DownArrow"]),
    ] {
        let hit = frame.map.rows.iter().find(|h| h.name == row).unwrap();
        let cell = hit.cells.iter().find(|c| c.value == value).unwrap();
        // The pointer rests on the value it clicked.
        let clicked = last_frame(vec![mouse(cell.start, hit.y, true), mouse(cell.start, hit.y, false), esc()]);
        let mut typed: Vec<Event> = keys.iter().map(|k| Event::Key(crate::input::scripted_key(k).unwrap())).collect();
        if row == "Effort" {
            let steps = ["default", "low", "medium", "high"].iter().position(|x| *x == value).unwrap();
            typed.extend((0..steps).map(|_| Event::Key(KeyPress::named(vk::RIGHT))));
        }
        typed.push(esc());
        assert_eq!(clicked, last_frame(typed), "{row} = {value}: the click and the keys draw one frame");
    }
}

#[test]
fn enter_alone_keeps_every_default_and_the_model_pin() {
    let s = drive("Enter").unwrap();
    assert_eq!(
        (
            s.account.as_str(),
            s.remote.as_str(),
            s.action.as_str(),
            s.model.as_str(),
            s.mode.as_str()
        ),
        ("work", "on", "new", "opus1m", "normal")
    );
}

#[test]
fn escape_cancels() {
    assert!(drive("Escape").is_none());
}

#[test]
fn right_on_account_switches_tab_and_d_aliases_right() {
    assert_eq!(drive("RightArrow,Enter").unwrap().account, "personal");
    assert_eq!(drive("d,d,Enter").unwrap().account, "low");
    assert_eq!(drive("LeftArrow,Enter").unwrap().account, "low", "wraps");
}

#[test]
fn down_then_right_steps_the_model() {
    let s = drive("DownArrow,DownArrow,RightArrow,Enter").unwrap();
    assert_eq!(s.model, "fable", "from the Opus pin to the next stronger");
    assert_eq!(s.row, 2);
}

#[test]
fn cyrillic_d_steps_on_any_layout() {
    let s = drive("DownArrow,\u{0432},Enter").unwrap();
    assert_eq!(s.remote, "off");
}

#[test]
fn ctrl_r_resets_the_tab() {
    let mut r = four();
    let v = view();
    let p = Prefs::default();
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let mut st = r.new_state();
    st.effort = "max".into();
    st.restored = vec!["Effort".into()];
    let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
    let ctrl_r = KeyPress {
        vk: vk::R,
        ctrl: true,
        ch: '\u{12}',
        ..KeyPress::default()
    };
    let mut t = PreviewTerm::new((100, 30), [ctrl_r, KeyPress::named(vk::ENTER)].into(), true);
    let out = run(&mut s, &mut t).unwrap().unwrap();
    assert_eq!(out.effort, "default");
    assert_eq!(out.restored, [] as [std::string::String; 0]);
}

#[test]
fn caller_hook_sees_u_first() {
    let mut r = four();
    let v = view();
    let p = Prefs::default();
    let mut opened = 0;
    let mut hook = |k: &KeyPress, _: &mut dyn Term| {
        if crate::input::is_hotkey(k, 'u') {
            opened += 1;
            return true;
        }
        false
    };
    let st = r.new_state();
    let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
    let mut t = PreviewTerm::new((100, 30), crate::ui::parse_preview_keys("u,Enter").unwrap(), true);
    assert!(run(&mut s, &mut t).unwrap().is_some());
    drop(s);
    assert_eq!(opened, 1);
}

#[test]
fn clicking_an_option_cell_selects_that_value_and_hover_bands_it() {
    let mut r = four();
    let v = LaunchView {
        style: Style {
            color: false,
            ascii: false,
        },
        ..view()
    };
    let p = Prefs::default();
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = r.new_state();
    let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
    // Effort row is y = 3 (box) + 1 (blank) + 3; 'max' sits after "   effort      ● [default] ○ low ...".
    let frame = s.draw((100, 30));
    let effort = frame.map.rows.iter().find(|h| h.name == "Effort").unwrap();
    let max = effort.cells.iter().find(|c| c.value == "max").unwrap();
    let mv = Event::Mouse(MouseEvent {
        x: max.start,
        y: effort.y,
        is_move: true,
        ..MouseEvent::default()
    });
    let press = Event::Mouse(MouseEvent {
        x: max.start,
        y: effort.y,
        left: true,
        buttons: 1,
        ..MouseEvent::default()
    });
    let mut t = crate::ui::script_term::ScriptTerm::new((100, 30), [mv, press, Event::Key(KeyPress::named(vk::ENTER))]);
    let out = run(&mut s, &mut t).unwrap().unwrap();
    assert_eq!(out.effort, "max");
    assert_eq!(out.row, 3);
    let frames = t.inner.frames.unwrap();
    assert!(
        frames.get(1).unwrap().iter().any(|l| l.contains("max")),
        "the hovered frame still draws the row"
    );
}

/// Config `rows` in memory. `pending` lands on the second load: what maintenance `l` saved.
#[derive(Default)]
struct FakeStore {
    disk: RowsConfig,
    pending: Option<RowsConfig>,
    loads: usize,
}

impl RowsStore for FakeStore {
    fn load(&mut self) -> RowsConfig {
        self.loads += 1;
        if self.loads > 1
            && let Some(p) = self.pending.take()
        {
            self.disk = p;
        }
        self.disk.clone()
    }
}

/// Runs the screen over `r` with `store`; the hook takes `u` as maintenance would.
fn edit(r: &mut Roster, st: LaunchState, store: &mut FakeStore, keys: &str) -> (Option<LaunchState>, Vec<Vec<String>>) {
    let v = view();
    let p = Prefs::default();
    let mut hook = |k: &KeyPress, _: &mut dyn Term| crate::input::is_hotkey(k, 'u');
    let mut s = LaunchScreen::new(r, st, &v, &p, 0, &mut hook).with_rows(store);
    let mut t = PreviewTerm::new((100, 30), crate::ui::parse_preview_keys(keys).unwrap(), true);
    let out = run(&mut s, &mut t).unwrap();
    (out, t.frames.unwrap())
}

fn names(r: &Roster) -> Vec<RowName> {
    r.rows.iter().map(|x| x.name).collect()
}

const ALL: [RowName; 7] = crate::launch::CONFIG_ROWS;

#[test]
fn row_keys_change_no_rows_on_the_launch_screen() {
    let mut r = four();
    let mut store = FakeStore::default();
    let st = r.new_state();
    // One key per run: a move and its reverse in one run would cancel out.
    for keys in [
        "DownArrow,DownArrow,Ctrl+S,Enter",
        "DownArrow,DownArrow,Ctrl+W,Enter",
        "DownArrow,DownArrow,Ctrl+X,Enter",
    ] {
        let st = r.new_state();
        let (out, _) = edit(&mut r, st, &mut store, keys);
        assert_eq!(out.unwrap().row, 2, "{keys}: the cursor stays");
        assert_eq!(names(&r), ALL, "{keys}");
    }
    let (_, frames) = edit(&mut r, st, &mut store, "o,Enter");
    assert!(
        !frames.iter().any(|f| f.iter().any(|l| l.contains("launch rows"))),
        "no editor here: maintenance l has it"
    );
}

#[test]
fn rows_saved_in_maintenance_apply_on_return() {
    let mut r = four();
    let mut store = FakeStore {
        pending: Some(RowsConfig {
            rows: Some(vec![RowName::Account, RowName::Mode]),
            ..RowsConfig::default()
        }),
        ..FakeStore::default()
    };
    let st = r.new_state();
    let (out, _) = edit(&mut r, st, &mut store, "u,Enter");
    assert!(out.is_some());
    assert_eq!(names(&r), [RowName::Account, RowName::Mode]);
}

#[test]
fn a_row_hidden_mid_run_never_takes_a_value_from_another_tabs_stash() {
    // work: effort -> low, advisor -> fable; tab to personal; maintenance hides both (effort
    // pinned high); tab back to work: the stash must not bring low or fable back.
    let keys = "DownArrow,DownArrow,DownArrow,RightArrow,DownArrow,RightArrow,UpArrow,UpArrow,UpArrow,UpArrow,RightArrow,u,LeftArrow,Enter";
    let mut r = four();
    let mut store = FakeStore {
        pending: Some(RowsConfig {
            rows: Some(vec![RowName::Account, RowName::Model]),
            hidden_values: BTreeMap::from([(RowName::Effort, "high".to_owned())]),
            ..RowsConfig::default()
        }),
        ..FakeStore::default()
    };
    let st = r.new_state();
    let out = edit(&mut r, st, &mut store, keys).0.unwrap();
    assert_eq!(out.account, "work", "back on the first tab");
    assert_eq!((out.effort.as_str(), out.advisor.as_str()), ("high", "default"));
    assert!(!out.is_restored(RowName::Effort) && !out.is_restored(RowName::Advisor));
    assert_eq!(
        super::super::launch_args(&r, &out, None, false, None, "", "").join(" "),
        "--model opus[1m] --effort high"
    );
}

#[test]
fn hidden_values_saved_in_maintenance_apply_on_return() {
    let mut r = four();
    let mut store = FakeStore {
        pending: Some(RowsConfig {
            rows: Some(vec![RowName::Account, RowName::Mode]),
            hidden_values: BTreeMap::from([(RowName::Effort, "high".to_owned())]),
            ..RowsConfig::default()
        }),
        ..FakeStore::default()
    };
    let st = r.new_state();
    let out = edit(&mut r, st, &mut store, "u,Enter").0.unwrap();
    assert_eq!(out.effort, "high", "the hidden effort launches with its pin");
    assert_eq!(out.model, "opus1m", "an unpinned hidden row resolves its default");
}
