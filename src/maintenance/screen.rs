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
use crate::input::{KeyPress, is_hotkey_exact};
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
        if was_pending.is_some_and(|p| !is_hotkey_exact(key, p)) {
            self.status.clear();
        }
        if let Some(c) = BUILT_IN.into_iter().find(|c| is_hotkey_exact(key, *c)) {
            let confirmed = was_pending == Some(c) && !too_fast;
            self.built_in(c, term, confirmed, now);
            return true;
        }
        let actions = self.actions;
        let hit = actions.iter().find_map(|a| {
            let c = a.key.chars().next()?;
            (!BUILT_IN.contains(&c) && is_hotkey_exact(key, c)).then_some((a, c))
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
mod tests;
