use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use super::script::GIB;
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

mod input;
mod page;
mod rows_editor;
mod running;
mod walks;
