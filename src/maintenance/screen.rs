//! The maintenance screen (`Invoke-MaintenanceScreen`, `Get-MaintenanceFrame`): install info, a
//! status box, the five built-in keys and the configured actions. Every action reports through the
//! status line, slow ones draw a "running ..." frame first, and confirm-twice actions gate on the
//! input RECORD time so a paste is never two presses.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use unicode_general_category::{GeneralCategory, get_general_category};

use super::install::{InstallInfo, Pruned};
use crate::config::MaintenanceAction;
use crate::frame::{
    FooterLight, Frame, Hint, MIN_HEIGHT, MIN_WIDTH, RowMap, Style, complete_frame, footer_hover, frame_width, new_hint_footer,
    too_small_frame,
};
use crate::input::{KeyPress, is_hotkey};
use crate::layout::{new_box, split_output_lines};
use crate::theme::{glyphs, strip_ctl};
use crate::ui::Term;
use crate::ui::screen_loop::{Cursor, Screen};

/// Two presses closer than this are one paste, not a confirmation.
pub(crate) const CONFIRM_MIN_MS: u64 = 150;
const BUILT_IN: [char; 5] = ['u', 'r', 'd', 'm', 'p'];
const GIB: u64 = 1024 * 1024 * 1024;

static CSI: LazyLock<Regex> = LazyLock::new(|| crate::theme::re(r"\x1b\[[0-9;?]*[A-Za-z]"));

/// `ConvertTo-StatusText`: CSI sequences and every other control character out (line breaks and
/// tabs kept for the wrapper downstream), trimmed. An escape byte would make every width wrong.
pub(crate) fn status_text(output: &str) -> String {
    let text = CSI.replace_all(output, "");
    let kept: String = text
        .chars()
        .filter(|c| {
            matches!(c, '\r' | '\n' | '\t')
                || !matches!(
                    get_general_category(*c),
                    GeneralCategory::Control
                        | GeneralCategory::Format
                        | GeneralCategory::Surrogate
                        | GeneralCategory::PrivateUse
                        | GeneralCategory::Unassigned
                )
        })
        .collect();
    kept.trim().to_owned()
}

/// What a configured script printed and how it exited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptRun {
    pub(crate) output: String,
    pub(crate) exit_code: i32,
}

/// `Invoke-MaintenanceScript`'s verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptResult {
    pub(crate) ran: bool,
    pub(crate) ok: bool,
    pub(crate) message: String,
}

/// `Invoke-MaintenanceScript`: the existence check runs before any runner; exit 2 is "could not
/// run", never a pass; the last three non-blank lines follow the verdict.
pub(crate) fn run_script(script: &Path, label: &str, runner: &mut dyn FnMut(&Path) -> Result<ScriptRun, String>) -> ScriptResult {
    if !script.exists() {
        return ScriptResult {
            ran: false,
            ok: false,
            message: format!("{label} script not found at {} - nothing was run.", script.display()),
        };
    }
    let r = match runner(script) {
        Ok(r) => r,
        Err(e) => {
            return ScriptResult {
                ran: false,
                ok: false,
                message: format!("{label} could not start: {e}"),
            };
        }
    };
    let verdict = match r.exit_code {
        0 => format!("{label} finished green"),
        2 => format!("{label} COULD NOT RUN (exit 2) - not a pass"),
        c => format!("{label} FAILED (exit {c})"),
    };
    let text = status_text(&r.output);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines.get(lines.len().saturating_sub(3)..).unwrap_or_default().join(" | ");
    ScriptResult {
        ran: true,
        ok: r.exit_code == 0,
        message: if tail.is_empty() { verdict } else { format!("{verdict} - {tail}") },
    }
}

/// Bytes as GiB with one decimal, rounded half away from zero (`'{0:N1}' -f ($b / 1GB)`).
pub(crate) fn gib(bytes: u64) -> String {
    let tenths = (u128::from(bytes) * 10 + u128::from(GIB) / 2) / u128::from(GIB);
    let whole = tenths / 10;
    let grouped = whole
        .to_string()
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(",");
    format!("{grouped}.{}", tenths % 10)
}

fn prefix16(hash: Option<&str>, none: &str) -> String {
    hash.map_or_else(|| none.to_owned(), |h| h.chars().take(16).collect())
}

/// `Get-MaintenanceFrame`. `hover` is a clickable footer-button index, -1 for none.
pub(crate) fn maintenance_frame(
    info: &InstallInfo,
    size: (usize, usize),
    status: &str,
    style: Style,
    actions: &[MaintenanceAction],
    hover: i32,
) -> Frame {
    let (width, height) = size;
    if width < MIN_WIDTH || height < MIN_HEIGHT {
        return Frame {
            lines: too_small_frame(width, height, style.ascii),
            map: RowMap::default(),
        };
    }
    let g = glyphs(style.ascii);
    let fw = frame_width(width);
    let box_width = fw.min(100);
    let verdict = if info.matches {
        "yes - newest downloaded build"
    } else {
        "NO - older; press u to install it"
    };
    let mut hints = vec![
        Hint::ch("u", "update", 'u'),
        Hint::ch("r", "rename swap", 'r'),
        Hint::ch("d", "doctor", 'd'),
        Hint::ch("m", "mcp list", 'm'),
        Hint::ch("p", "prune", 'p'),
    ];
    for a in actions {
        if let Some(c) = a.key.chars().next() {
            hints.push(Hint::ch(&strip_ctl(&a.key), &strip_ctl(&a.label), c));
        }
    }
    hints.push(Hint::key("esc", "back", "Escape"));
    let footer = new_hint_footer(&hints, &g, fw, !style.color);
    let light = FooterLight {
        hover: footer_hover(&footer, hover),
        selected: Vec::new(),
    };
    let mut body = vec![
        format!("  installed   {}", strip_ctl(&info.bin_path)),
        format!("  hash        {}", prefix16(info.installed_hash.as_deref(), "missing")),
        format!("  newest      {}", strip_ctl(info.newest_version.as_deref().unwrap_or_default())),
        format!("  hash        {}", prefix16(info.newest_hash.as_deref(), "none downloaded")),
        format!("  match       {verdict}"),
        format!("  versions    {} builds, {} GB", info.version_count, gib(info.versions_bytes)),
    ];
    if !status.is_empty() {
        // 10 = box borders, six info rows, the blank separator and one headroom row: a frame that
        // fills the terminal exactly scrolls the alternate buffer by one.
        let room = height.saturating_sub(10 + footer.lines.len()).max(1);
        body.push(String::new());
        let clean = status.replace(['\u{1}', '\u{2}', '\u{4}', '\u{5}'], "");
        body.extend(
            split_output_lines(&clean, box_width.saturating_sub(4), room)
                .into_iter()
                .map(|l| format!("  {l}")),
        );
    }
    let lines = new_box(&body, box_width, "maintenance", &g, style.ascii);
    let mut map = RowMap::default();
    let painter = |l: &str, e: bool| crate::list::add_picker_color(l, e, &g);
    let lines = complete_frame(&lines, &footer, fw, style, &painter, &light, &mut map);
    Frame { lines, map }
}

/// What the screen's keys do to the machine; the seam that keeps tests off the real install.
pub(crate) trait Ops {
    /// `Get-ClaudeInstallInfo`, refreshed by every draw.
    fn info(&mut self) -> InstallInfo;
    /// `(Invoke-ClaudeUpdate).Message`.
    fn update(&mut self) -> String;
    /// `(Repair-ClaudeBinaryByRename).Message`.
    fn rename_swap(&mut self) -> String;
    /// `Invoke-ClaudeCommandText`: `claude <args>` reduced to status text, never a failure.
    fn command_text(&mut self, args: &[&str]) -> String;
    /// `Remove-OldClaudeVersions -Keep 2`.
    fn prune(&mut self) -> Pruned;
    /// The script runner behind `Invoke-MaintenanceScript`.
    fn run(&mut self, script: &Path) -> Result<ScriptRun, String>;
}

/// The screen's state between keys.
pub(crate) struct MaintScreen<'a> {
    pub(crate) cursor: Cursor,
    pub(crate) ops: &'a mut dyn Ops,
    pub(crate) actions: &'a [MaintenanceAction],
    pub(crate) style: Style,
    pub(crate) info: InstallInfo,
    pub(crate) status: String,
    /// The key whose confirm is armed, and the record time it was armed at. State, never a parse
    /// of the status text: any press consumes it.
    pending: Option<char>,
    pending_at: Option<u64>,
}

impl std::fmt::Debug for MaintScreen<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaintScreen")
            .field("status", &self.status)
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

impl<'a> MaintScreen<'a> {
    pub(crate) fn new(ops: &'a mut dyn Ops, actions: &'a [MaintenanceAction], style: Style) -> Self {
        Self {
            cursor: Cursor::at(0),
            ops,
            actions,
            style,
            info: InstallInfo::default(),
            status: String::new(),
            pending: None,
            pending_at: None,
        }
    }

    /// Repaints the frame on screen with the current status, before a slow child runs.
    fn paint_running(&mut self, term: &mut dyn Term, text: String) {
        self.status = text;
        let size = term.size();
        let f = maintenance_frame(&self.info, size, &self.status, self.style, self.actions, self.cursor.hover);
        term.paint(&f.lines);
    }

    fn arm(&mut self, key: char, now: Option<u64>, text: String) {
        self.pending = Some(key);
        self.pending_at = now;
        self.status = text;
    }

    fn built_in(&mut self, c: char, term: &mut dyn Term, confirmed: bool, now: Option<u64>) {
        match c {
            'u' => {
                self.paint_running(term, "running claude update...".into());
                self.status = self.ops.update();
                term.drain();
            }
            'r' => {
                self.status = self.ops.rename_swap();
                term.drain();
            }
            'd' => {
                self.paint_running(term, "running claude doctor...".into());
                self.status = self.ops.command_text(&["doctor"]);
                term.drain();
            }
            'm' => {
                self.paint_running(term, "running claude mcp list...".into());
                self.status = self.ops.command_text(&["mcp", "list"]);
                term.drain();
            }
            _ if confirmed => {
                let p = self.ops.prune();
                self.status = format!("deleted {} builds, freed {} GB", p.deleted.len(), gib(p.freed_bytes));
            }
            _ => self.arm('p', now, "confirm: press p again to delete all but the 2 newest builds".into()),
        }
    }

    fn action(&mut self, a: &MaintenanceAction, key: char, term: &mut dyn Term, confirmed: bool, now: Option<u64>) {
        if a.confirm_twice && !confirmed {
            self.arm(
                key,
                now,
                format!(
                    "confirm: press {key} again to run {} (the screen will sit still while it runs)",
                    a.label
                ),
            );
            return;
        }
        self.paint_running(term, format!("running {}...", a.label));
        let ops = &mut *self.ops;
        self.status = run_script(Path::new(&a.script), &a.label, &mut |p| ops.run(p)).message;
        term.drain();
    }
}

impl Screen for MaintScreen<'_> {
    type Output = ();

    fn name(&self) -> &'static str {
        "maintenance"
    }

    fn cursor(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    fn draw(&mut self, size: (usize, usize)) -> Frame {
        self.info = self.ops.info();
        maintenance_frame(&self.info, size, &self.status, self.style, self.actions, self.cursor.hover)
    }

    /// Every key passes here first: the confirm bookkeeping, then the built-in keys and the
    /// configured actions (a configured action never takes a built-in letter). Escape and Ctrl+C
    /// fall through to the loop. No rows and no left/right, so no WASD alias shadows a menu key.
    fn on_key(&mut self, key: &KeyPress, term: &mut dyn Term) -> bool {
        let was_pending = self.pending.take();
        let armed_at = self.pending_at.take();
        let now = term.record_time();
        let too_fast = matches!((now, armed_at), (Some(n), Some(a)) if n.checked_sub(a).is_none_or(|d| d < CONFIRM_MIN_MS));
        if was_pending.is_some_and(|p| !is_hotkey(key, p)) {
            self.status.clear();
        }
        if let Some(c) = BUILT_IN.into_iter().find(|c| is_hotkey(key, *c)) {
            let confirmed = was_pending == Some(c) && !too_fast;
            self.built_in(c, term, confirmed, now);
            return true;
        }
        let actions = self.actions;
        let hit = actions.iter().find_map(|a| {
            let c = a.key.chars().next()?;
            (!BUILT_IN.contains(&c) && is_hotkey(key, c)).then_some((a, c))
        });
        if let Some((a, c)) = hit {
            let confirmed = was_pending == Some(c) && !too_fast;
            self.action(a, c, term, confirmed, now);
            return true;
        }
        false
    }

    fn silent(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use super::*;
    use crate::input::{Event, MouseEvent, vk};
    use crate::layout::display_width;
    use crate::ui::screen_loop::run;
    use crate::ui::{Activation, Exhausted, PreviewTerm};

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
    fn frame_fills_the_frame_width_and_shows_the_newest_version() {
        let f = maintenance_frame(&info(false, Some("A"), Some("B")), (80, 24), "", plain(), &[], -1);
        assert_eq!(widest(&f.lines), frame_width(80));
        assert_eq!(count(&f.lines, "2.1.230"), 1);
    }

    #[test]
    fn width_60_keeps_hash_version_and_size_whole() {
        let (a, b) = ("a1".repeat(32), "b2".repeat(32));
        let mut i = info(false, Some(&a), Some(&b));
        i.version_count = 12;
        i.versions_bytes = 3_650_722_201;
        let f = maintenance_frame(&i, (60, MIN_HEIGHT), "", plain(), &[], -1);
        assert_eq!(widest(&f.lines), frame_width(60));
        assert_eq!(count(&f.lines, a.get(..16).unwrap()), 1);
        assert_eq!(count(&f.lines, "12 builds, 3.4 GB"), 1);
    }

    #[test]
    fn short_or_missing_hashes_render() {
        for i in [
            info(false, Some("ab12c"), Some("de34f")),
            InstallInfo {
                bin_path: BIN.into(),
                ..InstallInfo::default()
            },
        ] {
            let f = maintenance_frame(&i, (78, 24), "", plain(), &[], -1);
            assert_eq!(count(&f.lines, "Exception") + count(&f.lines, "error"), 0);
        }
        let f = maintenance_frame(&InstallInfo::default(), (78, 24), "", plain(), &[], -1);
        assert_eq!(count(&f.lines, "missing") + count(&f.lines, "none downloaded"), 2);
    }

    #[test]
    fn a_multi_line_status_lands_one_row_per_line() {
        let s = "Running: native (2.1.229)\nPlatform: win32-x64\n\nNo installation issues found.";
        let f = maintenance_frame(&info(false, None, None), (78, 24), s, plain(), &[], -1);
        assert_eq!(count(&f.lines, "\n"), 0);
        for needle in ["Running: native", "Platform: win32-x64", "No installation issues found"] {
            assert_eq!(count(&f.lines, needle), 1, "{needle}");
        }
    }

    #[test]
    fn a_long_status_wraps_and_a_tall_one_is_capped_with_headroom() {
        let one = "the updater could not overwrite the running claude.exe, so it was installed by rename: now 2.1.230, verified by hash";
        let f = maintenance_frame(&info(false, None, None), (78, 24), one, plain(), &[], -1);
        assert_eq!(count(&f.lines, "verified by hash"), 1);
        let long: Vec<String> = (1..=40).map(|i| format!("status line {i}")).collect();
        let long = long.join("\n");
        for h in [16, 24, 50] {
            let f = maintenance_frame(&info(false, None, None), (78, h), &long, plain(), &[], -1);
            assert!(f.lines.len() < h, "a spare row at height {h}");
        }
        let f = maintenance_frame(&info(false, None, None), (78, 24), &long, plain(), &[], -1);
        assert_eq!(count(&f.lines, "status line 1 "), 1, "the cap keeps the head");
        assert_eq!(count(&f.lines, "status line 40"), 0);
        assert_eq!(count(&f.lines, "more lines"), 1);
    }

    #[test]
    fn both_verdicts_survive_the_minimum_width() {
        for w in [50, 60] {
            let f = maintenance_frame(&info(false, None, None), (w, MIN_HEIGHT), "", plain(), &[], -1);
            assert_eq!(count(&f.lines, "press u to install it"), 1, "width {w}");
            let f = maintenance_frame(&info(true, None, None), (w, MIN_HEIGHT), "", plain(), &[], -1);
            assert_eq!(count(&f.lines, "newest downloaded build"), 1, "width {w}");
        }
    }

    fn footer_keys(map: &RowMap) -> String {
        map.footer
            .iter()
            .map(|s| s.ch.map_or_else(|| s.key.clone(), |c| c.to_string()))
            .collect::<Vec<_>>()
            .join(",")
    }

    #[test]
    fn every_hint_is_clickable_even_at_50_columns_with_a_cjk_label() {
        let f = maintenance_frame(&info(true, None, None), (100, 24), "", plain(), &reindex(), -1);
        assert_eq!(footer_keys(&f.map), "u,r,d,m,p,i,Escape");
        let mut cjk = reindex();
        cjk[0].label = "\u{5168}\u{91CF}\u{91CD}\u{5EFA}\u{7D22}\u{5F15}\u{FF08}\u{6162}\u{FF09}".into();
        let f = maintenance_frame(&info(true, None, None), (50, 24), "", plain(), &cjk, -1);
        assert_eq!(footer_keys(&f.map), "u,r,d,m,p,i,Escape");
        assert!(widest(&f.lines) <= 49);
        assert!(f.lines.join("\n").contains(&cjk[0].label));
    }

    #[test]
    fn at_50_columns_every_hint_is_readable_and_the_frame_has_headroom() {
        let long: Vec<String> = (1..=40).map(|i| format!("status line {i} with some words in it")).collect();
        for h in [MIN_HEIGHT, 24, 30] {
            let f = maintenance_frame(&info(false, None, None), (50, h), &long.join("\n"), plain(), &reindex(), -1);
            let text = f.lines.join("\n");
            for hint in [
                "[u] update",
                "[r] rename swap",
                "[d] doctor",
                "[m] mcp list",
                "[p] prune",
                "[i] full reindex",
                "[esc] back",
            ] {
                assert!(text.contains(hint), "50x{h}: {hint}");
            }
            assert!(f.lines.len() < h);
        }
        let f = maintenance_frame(&info(false, None, None), (50, 24), "", plain(), &reindex(), -1);
        assert!(f.map.footer_lines >= 2, "at 50 columns the footer wraps");
    }

    #[test]
    fn no_action_no_hint() {
        let f = maintenance_frame(&info(false, None, None), (78, 24), "", plain(), &[], -1);
        assert!(!f.lines.join("\n").contains("full reindex"));
        let f = maintenance_frame(&info(false, None, None), (78, 24), "", plain(), &reindex(), -1);
        assert!(f.lines.join("\n").contains("[i] full reindex"));
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
        fn update(&mut self) -> String {
            self.log.borrow_mut().updates += 1;
            "updated".into()
        }
        fn rename_swap(&mut self) -> String {
            "swapped".into()
        }
        fn command_text(&mut self, args: &[&str]) -> String {
            self.log.borrow_mut().doctor += 1;
            format!("doctor report for {}", args.join(" "))
        }
        fn prune(&mut self) -> Pruned {
            self.log.borrow_mut().prunes += 1;
            Pruned::default()
        }
        fn run(&mut self, script: &Path) -> Result<ScriptRun, String> {
            self.log.borrow_mut().runs.push(script.display().to_string());
            Ok(ScriptRun {
                output: "fleet: 48/48 ok\nartifacts refreshed".into(),
                exit_code: self.exit_code,
            })
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
        fn wait(&mut self, _s: (usize, usize)) -> Result<Event, Exhausted> {
            if !self.clock.is_empty() {
                self.now = self.clock.remove(0);
            }
            self.q.pop_front().ok_or(Exhausted)
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
    ) -> (Log, String, T, Result<Option<()>, Exhausted>) {
        let log = Rc::new(RefCell::new(Log::default()));
        let mut ops = FakeOps {
            log: Rc::clone(&log),
            exit_code,
        };
        let mut t = T {
            inner: PreviewTerm::new(size, VecDeque::new(), true),
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
        let status = t.frames.last().cloned().unwrap_or_default();
        let log = std::mem::take(&mut *log.borrow_mut());
        (log, status, t, r)
    }

    fn map_at(size: (usize, usize), actions: &[MaintenanceAction]) -> RowMap {
        let mut o = FakeOps {
            log: Rc::default(),
            exit_code: 0,
        };
        maintenance_frame(&o.info(), size, "", plain(), actions, -1).map
    }

    fn span(map: &RowMap, c: char) -> crate::frame::FooterSpan {
        map.footer.iter().find(|s| s.ch == Some(c)).unwrap().clone()
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
    fn clicking_esc_back_leaves_and_one_column_past_a_hint_does_not() {
        let m = map_at((100, 24), &reindex());
        let e = m.footer.iter().find(|s| s.key == "Escape").unwrap().clone();
        let y = m.footer_y.unwrap();
        let fy = |l: usize| y + i32::try_from(l).unwrap();
        assert_eq!(
            go(vec![click(e.start, fy(e.line), false)], &reindex(), vec![], 0, (100, 24)).3,
            Ok(None)
        );
        let d = span(&m, 'd');
        assert_eq!(d.line, 0, "doctor sits on the first footer line");
        assert_eq!(
            go(vec![click(d.end + 1, fy(d.line), false)], &reindex(), vec![], 0, (100, 24)).3,
            Err(Exhausted)
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
            vec![mv(&span(&m, 'd')), mv(&span(&m, 'p')), esc()],
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
    fn one_press_only_warns_the_second_runs_once() {
        let (log, status, _, _) = go(vec![key('i'), esc()], &reindex(), vec![], 0, (100, 24));
        assert_eq!(log.runs.len(), 0);
        assert!(status.contains("confirm: press i"));
        let (log, status, t, _) = go(vec![key('i'), key('i'), esc()], &reindex(), vec![], 0, (100, 24));
        assert_eq!(log.runs.len(), 1);
        assert!(status.contains("green"));
        assert_eq!(t.drains, 1, "the input queue is drained after a command that owned the screen");
        assert!(
            t.frames.iter().any(|f| f.contains("running full reindex...")),
            "a running frame first"
        );
    }

    /// Where hint `c` sits in the frame drawn with `status`: the footer moves down as a status
    /// appears, and a reader clicks where it is.
    fn hint_at(size: (usize, usize), status: &str, c: char) -> (i32, i32) {
        let mut o = FakeOps {
            log: Rc::default(),
            exit_code: 0,
        };
        let m = maintenance_frame(&o.info(), size, status, plain(), &reindex(), -1).map;
        let s = span(&m, c);
        (s.start, m.footer_y.unwrap() + i32::try_from(s.line).unwrap())
    }

    const ARMED_I: &str = "confirm: press i again to run full reindex (the screen will sit still while it runs)";

    #[test]
    fn clicking_the_hint_twice_runs_once_but_a_double_click_does_not_confirm() {
        let (x1, y1) = hint_at((100, 24), "", 'i');
        let (x2, y2) = hint_at((100, 24), ARMED_I, 'i');
        let (log, ..) = go(
            vec![click(x1, y1, false), click(x2, y2, false), esc()],
            &reindex(),
            vec![],
            0,
            (100, 24),
        );
        assert_eq!(log.runs.len(), 1);
        let (log, ..) = go(
            vec![click(x1, y1, false), click(x2, y2, true), esc()],
            &reindex(),
            vec![],
            0,
            (100, 24),
        );
        assert_eq!(log.runs.len(), 0);
    }

    #[test]
    fn a_hint_on_a_wrapped_footer_line_is_clicked_at_its_own_line() {
        let m = map_at((50, 24), &reindex());
        assert!(span(&m, 'i').line >= 1);
        let (x1, y1) = hint_at((50, 24), "", 'i');
        let (x2, y2) = hint_at((50, 24), ARMED_I, 'i');
        let (log, ..) = go(
            vec![click(x1, y1, false), click(x2, y2, false), esc()],
            &reindex(),
            vec![],
            0,
            (50, 24),
        );
        assert_eq!(log.runs.len(), 1);
        let first = m.footer_y.unwrap();
        let (log, ..) = go(
            vec![click(x1, first, false), click(x1, first, false), esc()],
            &reindex(),
            vec![],
            0,
            (50, 24),
        );
        assert_eq!(log.runs.len(), 0, "the same column on the first footer line is not that hint");
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
        assert!(t.frames.iter().any(|f| f.contains("running claude update...")));
        assert_eq!(t.drains, 1);
    }
}
