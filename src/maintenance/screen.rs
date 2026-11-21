//! The maintenance screen: a list of actions (built-in, claude-auto's own, configured) beside the
//! facts about the install. Every action shows a live step line while it runs and ends done or
//! failed; confirm-twice actions gate on the input RECORD time so a paste is never two presses.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use unicode_general_category::{GeneralCategory, get_general_category};

use super::install::{InstallInfo, Pruned};
use super::rows::RowsEditor;
use crate::config::{MaintenanceAction, RowsConfig};
use crate::frame::{Frame, Style};
use crate::input::{KeyPress, is_hotkey_exact, vk};
use crate::launch::RowName;
use crate::theme::{glyphs, strip_ctl};
use crate::types::VersionInfo;
use crate::ui::screen_loop::{Cursor, Flow, Screen};
use crate::ui::{Hit, Term};

mod view;
pub(crate) use view::{Entry, Group, Page, Phase, maintenance_frame};

/// Two presses closer than this are one paste, not a confirmation.
pub(crate) const CONFIRM_MIN_MS: u64 = 150;
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

/// A maintenance script's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptResult {
    pub(crate) ran: bool,
    pub(crate) ok: bool,
    pub(crate) message: String,
}

/// The existence check runs before any runner; exit 2 is "could not
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

/// How an action ended: done or failed, and what it reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) ok: bool,
    pub(crate) text: String,
}

impl Verdict {
    pub(crate) fn done(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
        }
    }

    pub(crate) fn failed(text: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: text.into(),
        }
    }
}

/// A step sink: every line a long action prints, `""` as a tick that only advances the clock.
pub(crate) type Step<'s> = &'s mut dyn FnMut(&str);

/// What the screen's keys do to the machine; the seam that keeps tests off the real install.
pub(crate) trait Ops {
    /// The install info, refreshed by every draw.
    fn info(&mut self) -> InstallInfo;
    /// The Claude Code update.
    fn update(&mut self, step: Step<'_>) -> Verdict;
    /// The rename swap.
    fn rename_swap(&mut self) -> Verdict;
    /// `claude <args>` reduced to status text; ok when it exited 0.
    fn command_text(&mut self, args: &[&str], step: Step<'_>) -> Verdict;
    /// Prunes old Claude Code builds, keeping two.
    fn prune(&mut self) -> Pruned;
    /// The maintenance script runner.
    fn run(&mut self, script: &Path, step: Step<'_>) -> Result<ScriptRun, String>;
    /// Config `rows`, `hiddenValues` and the keys shaping them, as the file holds them now.
    fn load_rows(&mut self) -> RowsConfig;
    /// Writes config `rows` and `hiddenValues`, every other key untouched.
    fn save_rows(&mut self, rows: &[RowName], hidden: &BTreeMap<RowName, String>) -> Result<(), String>;
    /// The launcher's own update entries (key, label), shown as rows like the configured actions.
    fn launcher_entries(&mut self) -> Vec<(char, String)> {
        Vec::new()
    }
    /// Runs a launcher entry's key.
    fn launcher_key(&mut self, _key: char) -> Verdict {
        Verdict::done("")
    }
    /// Keeps an MCP check's counts for the launch page.
    fn remember_mcp(&mut self, _connected: usize, _total: usize) {}
    /// claude-auto's self-update mode and state in words (`notify · up to date`).
    fn launcher_state(&mut self) -> String {
        String::new()
    }
}

/// Opens the launch-rows editor. Lower case and matched exactly: a Shift + upper-case letter is
/// what a split mouse report looks like. Config reserves it, so no action takes it.
const ROWS_KEY: char = 'l';

/// The built-in rows: key, name, and the short hint the selected row shows when the name alone
/// cannot carry it.
const BUILT_IN_ROWS: [(char, &str, &str, Group); 6] = [
    ('u', "update", "", Group::ClaudeCode),
    ('r', "update anyway (rename)", "while claude.exe is in use", Group::ClaudeCode),
    ('p', "delete old builds", "keeps the 2 newest + the running one", Group::ClaudeCode),
    ('d', "doctor", "", Group::Checks),
    ('m', "MCP servers", "checks each connection", Group::Checks),
    (ROWS_KEY, "launch rows", "", Group::Launcher),
];

/// The rows in drawing order (by group): built-ins, the launcher's own entries, the configured
/// actions (a configured action never takes a key already listed).
fn entries(launcher: &[(char, String)], actions: &[MaintenanceAction]) -> Vec<Entry> {
    let mut out: Vec<Entry> = BUILT_IN_ROWS
        .iter()
        .map(|(key, name, about, group)| Entry {
            key: *key,
            name: (*name).into(),
            about: (*about).into(),
            group: *group,
        })
        .collect();
    for (key, label) in launcher {
        out.push(Entry {
            key: *key,
            name: label.clone(),
            about: String::new(),
            group: Group::ClaudeAuto,
        });
    }
    for a in actions {
        if let Some(key) = a.key.chars().next()
            && !out.iter().any(|e| e.key == key)
        {
            out.push(Entry {
                key,
                name: strip_ctl(&a.label),
                about: String::new(),
                group: Group::Custom,
            });
        }
    }
    // Stable: a group keeps its rows' order.
    out.sort_by_key(|e| e.group);
    out
}

/// `claude mcp list` as a count and the per-server lines; a run with no server line is left as it is.
fn mcp_summary(v: Verdict) -> Verdict {
    if !v.ok {
        return v;
    }
    let servers: Vec<&str> = v
        .text
        .lines()
        .map(str::trim)
        .filter(|l| l.contains(": ") && l.contains(" - "))
        .collect();
    if servers.is_empty() {
        return v;
    }
    let connected = servers
        .iter()
        .filter(|l| l.ends_with("Connected") && !l.contains("Not connected"))
        .count();
    let head = format!("{connected} of {} connected", servers.len());
    let text = std::iter::once(head.as_str())
        .chain(servers.iter().copied())
        .collect::<Vec<_>>()
        .join("\n");
    Verdict {
        ok: connected == servers.len(),
        text,
    }
}

/// The counts of an `mcp_summary` head (`1 of 2 connected`); `None` for any other report.
fn mcp_counts(v: &Verdict) -> Option<(usize, usize)> {
    let head = v.text.lines().next()?.strip_suffix(" connected")?;
    let (c, t) = head.split_once(" of ")?;
    Some((c.parse().ok()?, t.parse().ok()?))
}

/// The screen's state between keys.
pub(crate) struct MaintScreen<'a> {
    pub(crate) cursor: Cursor,
    pub(crate) ops: &'a mut dyn Ops,
    pub(crate) actions: &'a [MaintenanceAction],
    pub(crate) style: Style,
    /// What the page draws; `status` is the last action's report.
    pub(crate) page: Page,
    /// The key whose confirm is armed, and the record time it was armed at. State, never a parse
    /// of the status text: any press consumes it.
    pending: Option<char>,
    pending_at: Option<u64>,
}

impl std::fmt::Debug for MaintScreen<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaintScreen")
            .field("page", &self.page)
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
            page: Page::default(),
            pending: None,
            pending_at: None,
        }
    }

    /// The facts that do not change while the screen is up: warnings, the installed version, the
    /// config path the warnings point at.
    #[must_use]
    pub(crate) fn with_facts(mut self, warnings: &[String], version: &VersionInfo, config_path: &str) -> Self {
        self.page.warnings = warnings.to_vec();
        self.page.installed.clone_from(&version.installed);
        self.page.newest.clone_from(&version.newest);
        config_path.clone_into(&mut self.page.config_path);
        self
    }

    fn arm(&mut self, key: char, now: Option<u64>, name: &str, text: String) {
        self.pending = Some(key);
        self.pending_at = now;
        name.clone_into(&mut self.page.last);
        self.page.phase = Phase::Armed;
        self.page.status = text;
    }

    /// Runs `act` with a step line repainted on every line it prints and every tick, then
    /// records done or failed.
    fn live(&mut self, term: &mut dyn Term, name: &str, doing: &str, act: impl FnOnce(&mut dyn Ops, Step<'_>) -> Verdict) {
        name.clone_into(&mut self.page.last);
        self.page.status.clear();
        let started = std::time::Instant::now();
        let (page, style) = (&self.page, self.style);
        let mut line = String::new();
        let mut paint = |text: &str| {
            if let Some(l) = status_text(text).lines().rev().find(|l| !l.trim().is_empty()) {
                l.trim().clone_into(&mut line);
            }
            let secs = started.elapsed().as_secs();
            let mut step = doing.to_owned();
            if secs > 0 {
                step = format!("{step} {} {secs} s", glyphs(style.ascii).dot);
            }
            if !line.is_empty() {
                step = format!("{step} {} {line}", glyphs(style.ascii).dot);
            }
            let shown = Page {
                phase: Phase::Running(step),
                ..page.clone()
            };
            let area = crate::frame::Area::of(term.size());
            term.paint(&maintenance_frame(&shown, area.size(), style).lines);
        };
        paint("");
        let v = act(&mut *self.ops, &mut paint);
        if v.ok {
            self.page.phase = Phase::Done;
            self.page.status = v.text;
        } else {
            // The head of the first line is the reason; the rest of the report stays below it.
            let first = v.text.lines().next().unwrap_or_default();
            let reason = first.split(" - ").next().unwrap_or_default();
            let rest: Vec<&str> = v.text.lines().skip(usize::from(reason == first)).collect();
            let reason = if reason.trim().is_empty() { "no reason given" } else { reason };
            self.page.phase = Phase::Failed(reason.to_owned());
            self.page.status = rest.join("\n");
        }
        term.drain();
    }

    /// The rows editor, opened on the config as the file holds it now; a save is reported in the
    /// status line. An editor that ends in `Err` (a lost console) is "unchanged" here: the error
    /// is a state, so this screen's own next wait returns it to the caller.
    fn edit_rows(&mut self, term: &mut dyn Term) {
        let current = self.ops.load_rows();
        let mut editor = RowsEditor::new(&current, self.style);
        "launch rows".clone_into(&mut self.page.last);
        let (ok, text) = match crate::ui::screen_loop::run(&mut editor, term) {
            Ok(Some((rows, hidden))) => match self.ops.save_rows(&rows, &hidden) {
                Ok(()) => {
                    let mut parts = vec![rows.iter().map(|r| r.as_str().to_lowercase()).collect::<Vec<_>>().join(", ")];
                    parts.extend(hidden.iter().map(|(r, v)| format!("hidden {} -> {v}", r.as_str().to_lowercase())));
                    (
                        true,
                        format!("launch rows saved: {} - shown on return to the launch screen", parts.join("; ")),
                    )
                }
                Err(e) => (false, format!("launch rows not saved: {e}")),
            },
            Ok(None) | Err(_) => (true, "launch rows unchanged".into()),
        };
        self.page.phase = if ok { Phase::Done } else { Phase::Failed("not saved".into()) };
        self.page.status = text;
    }

    /// One row's key: the built-ins, the launcher's entries, the configured actions.
    fn press(&mut self, key: char, term: &mut dyn Term, confirmed: bool, now: Option<u64>) {
        let ell = crate::layout::ellipsis(self.style.ascii);
        match key {
            ROWS_KEY => self.edit_rows(term),
            'u' => self.live(term, "update", "updating", |ops, s| ops.update(s)),
            'r' => self.live(term, "update anyway (rename)", "swapping", |ops, _| ops.rename_swap()),
            'd' => self.live(term, "doctor", "running claude doctor", |ops, s| ops.command_text(&["doctor"], s)),
            'm' => self.live(term, "MCP servers", &format!("checking{ell}"), |ops, s| {
                let v = mcp_summary(ops.command_text(&["mcp", "list"], s));
                if let Some((connected, total)) = mcp_counts(&v) {
                    ops.remember_mcp(connected, total);
                }
                v
            }),
            'p' if confirmed => {
                let p = self.ops.prune();
                "delete old builds".clone_into(&mut self.page.last);
                if p.message.is_empty() {
                    self.page.phase = Phase::Done;
                    self.page.status = format!("deleted {} builds, freed {} GB", p.deleted.len(), gib(p.freed_bytes));
                } else {
                    self.page.phase = Phase::Failed(p.message);
                    self.page.status.clear();
                }
            }
            'p' => self.arm(
                'p',
                now,
                "delete old builds",
                "confirm: press p again to delete all but the 2 newest builds".into(),
            ),
            _ => self.press_other(key, term, confirmed, now),
        }
    }

    fn press_other(&mut self, key: char, term: &mut dyn Term, confirmed: bool, now: Option<u64>) {
        let launcher = self.ops.launcher_entries();
        if let Some((_, label)) = launcher.iter().find(|(k, _)| *k == key) {
            self.live(term, label, label, |ops, _| ops.launcher_key(key));
            return;
        }
        let actions = self.actions;
        let Some(a) = actions.iter().find(|a| a.key.starts_with(key)) else {
            return;
        };
        let label = strip_ctl(&a.label);
        if a.confirm_twice && !confirmed {
            let text = format!("confirm: press {key} again to run {label}");
            self.arm(key, now, &label, text);
            return;
        }
        self.live(term, &label, "running", |ops, s| {
            let r = run_script(Path::new(&a.script), &label, &mut |p| ops.run(p, &mut *s));
            Verdict { ok: r.ok, text: r.message }
        });
    }

    /// Confirm bookkeeping, then `key` pressed: two presses closer than `CONFIRM_MIN_MS` are a paste.
    fn press_checked(&mut self, key: char, term: &mut dyn Term, was_pending: Option<char>, armed_at: Option<u64>) {
        let now = term.record_time();
        let too_fast = matches!((now, armed_at), (Some(n), Some(a)) if n.checked_sub(a).is_none_or(|d| d < CONFIRM_MIN_MS));
        self.press(key, term, was_pending == Some(key) && !too_fast, now);
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

    fn before(&mut self) {
        let last = self.page.entries.len().saturating_sub(1);
        self.page.selected = usize::try_from(self.cursor.index).unwrap_or(0).min(last);
        self.page.hover = self.cursor.hover;
        self.page.hover_row = self.cursor.hover_row;
    }

    fn draw(&mut self, size: (usize, usize)) -> Frame {
        self.page.info = self.ops.info();
        self.page.launcher = self.ops.launcher_state();
        self.page.entries = entries(&self.ops.launcher_entries(), self.actions);
        maintenance_frame(&self.page, size, self.style)
    }

    fn rows(&self) -> Option<i32> {
        Some(i32::try_from(self.page.entries.len().max(1)).unwrap_or(1))
    }

    /// A click selects a row; enter, its key or a second click on it runs it.
    fn click(&mut self, hit: &Hit) -> Flow<()> {
        if let Hit::LaunchRow { row, .. } = hit {
            self.cursor.index = i32::try_from(*row).unwrap_or(0);
        }
        Flow::Continue
    }

    /// The second click on the selected row is its key: the same confirm bookkeeping.
    fn activate(&mut self, hit: &Hit, term: &mut dyn Term) -> Option<Flow<()>> {
        let Hit::LaunchRow { row, .. } = hit else { return None };
        if *row != self.page.selected {
            return None;
        }
        let key = self.page.entries.get(*row)?.key;
        let (was_pending, armed_at) = (self.pending.take(), self.pending_at.take());
        self.press_checked(key, term, was_pending, armed_at);
        Some(Flow::Continue)
    }

    /// Every key passes here first: the confirm bookkeeping, then Enter on the selected row, then
    /// a row's own key (a configured action never takes a built-in letter). Escape and Ctrl+C
    /// fall through to the loop, and so do w/s and the arrows when no row owns the letter.
    fn on_key(&mut self, key: &KeyPress, term: &mut dyn Term) -> bool {
        let was_pending = self.pending.take();
        let armed_at = self.pending_at.take();
        let row_key = if key.is(vk::ENTER) {
            self.page.entries.get(self.page.selected).map(|e| e.key)
        } else {
            self.page.entries.iter().map(|e| e.key).find(|c| is_hotkey_exact(key, *c))
        };
        if was_pending.is_some_and(|p| row_key != Some(p)) {
            self.page.phase = Phase::Idle;
            self.page.status.clear();
        }
        let Some(c) = row_key else { return false };
        if let Some(i) = self.page.entries.iter().position(|e| e.key == c) {
            self.cursor.index = i32::try_from(i).unwrap_or(0);
            self.page.selected = i;
        }
        self.press_checked(c, term, was_pending, armed_at);
        true
    }

    fn silent(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
