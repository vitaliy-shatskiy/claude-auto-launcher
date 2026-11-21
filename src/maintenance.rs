//! Maintenance screen, Claude Code versions and the Claude Code update check.
//! Entry points for `app.rs`; the install
//! logic lives in `install`, the release channel in `release`, the screen in `screen`.

mod install;
mod mcp_seen;
mod release;
mod rows;
mod screen;
pub(crate) mod watch;
pub(crate) mod win;

pub(crate) use release::FetchFailed;
pub(crate) use watch::{InstallGate, UpdateFound, UpdateWatch};

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::config::{Config, PathCtx, RowsConfig};
use crate::env::Note;
use crate::frame::Style;
use crate::launch::RowName;
use crate::out::Fg;
use crate::types::VersionInfo;
use crate::ui::{Term, UiStop};
use install::{Install, InstallInfo, Pruned};
use screen::{MaintScreen, Ops, ScriptRun, Verdict};

const PREVIEW_NOTE: &str = "preview - nothing was run";

fn is_preview() -> bool {
    std::env::var("CLAUDE_AUTO_PREVIEW").as_deref() == Ok("1")
}

/// Runs `exe args` with stdout and stderr captured (`2>&1`): the text and the exit code.
fn captured(exe: &Path, args: &[&str]) -> Result<(String, i32), String> {
    let o = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
    let err = String::from_utf8_lossy(&o.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&err);
    }
    Ok((text, o.status.code().unwrap_or(1)))
}

/// `claude <args>` captured; `Err` when `claude` cannot be found or started.
fn claude_output(args: &[&str]) -> Result<String, String> {
    let exe = crate::invocation::resolve_claude()?;
    captured(&exe, args).map(|(t, _)| t)
}

/// How long the pipes are still read once the child has exited.
const AFTER_EXIT: std::time::Duration = std::time::Duration::from_secs(1);
const TICK: std::time::Duration = std::time::Duration::from_millis(250);

/// Runs `exe args` with stdout and stderr read line by line: every line goes to `step` as it
/// arrives, a `""` tick every 250 ms. The read ends one second after the child EXITS, not at the
/// pipes' end: a grandchild that inherited them (an MCP server `claude mcp list` started) can hold
/// them open long after, and `output()` waited for it with the screen stuck.
pub(crate) fn captured_live(exe: &Path, args: &[&str], step: screen::Step<'_>) -> Result<(String, i32), String> {
    use std::io::BufRead as _;
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let pipes: [Option<Box<dyn std::io::Read + Send>>; 2] = [
        child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    ];
    for pipe in pipes.into_iter().flatten() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut r = std::io::BufReader::new(pipe);
            let mut buf = Vec::new();
            while r.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                if tx.send(String::from_utf8_lossy(&buf).into_owned()).is_err() {
                    break;
                }
                buf.clear();
            }
        });
    }
    drop(tx);
    let mut text = String::new();
    let mut exit: Option<(i32, std::time::Instant)> = None;
    loop {
        match rx.recv_timeout(TICK) {
            Ok(line) => {
                step(&line);
                text.push_str(&line);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => step(""),
        }
        if exit.is_none()
            && let Ok(Some(status)) = child.try_wait()
        {
            exit = Some((status.code().unwrap_or(1), std::time::Instant::now()));
        }
        if exit.is_some_and(|(_, at)| at.elapsed() >= AFTER_EXIT) {
            break;
        }
    }
    let code = match exit {
        Some((c, _)) => c,
        None => child.wait().map_or(1, |s| s.code().unwrap_or(1)),
    };
    Ok((text, code))
}

/// The machine behind the screen's keys. A preview reads the install and changes nothing.
#[derive(Debug)]
struct RealOps {
    install: Install,
    preview: bool,
    config_path: String,
    launcher: crate::self_update::Local,
}

impl Ops for RealOps {
    fn info(&mut self) -> InstallInfo {
        self.install.info()
    }

    fn update(&mut self, step: screen::Step<'_>) -> Verdict {
        if self.preview {
            return Verdict::done(PREVIEW_NOTE);
        }
        let u = self.install.update(|| {
            let exe = crate::invocation::resolve_claude()?;
            let out = captured_live(&exe, &["update"], &mut *step).map(|(t, _)| t);
            step("verifying the installed build");
            out
        });
        Verdict {
            ok: u.matches,
            text: u.message,
        }
    }

    fn rename_swap(&mut self) -> Verdict {
        if self.preview {
            return Verdict::done(PREVIEW_NOTE);
        }
        let o = self.install.rename_swap();
        Verdict { ok: o.ok, text: o.message }
    }

    fn command_text(&mut self, args: &[&str], step: screen::Step<'_>) -> Verdict {
        if self.preview {
            return Verdict::done(PREVIEW_NOTE);
        }
        let ran = crate::invocation::resolve_claude()
            .map_err(str::to_owned)
            .and_then(|exe| captured_live(&exe, args, step));
        match ran {
            Ok((text, 0)) => Verdict::done(screen::status_text(&text)),
            Ok((text, code)) => Verdict::failed(format!("claude {} exited {code}\n{}", args.join(" "), screen::status_text(&text))),
            Err(e) => Verdict::failed(format!("claude {} could not be started: {e}", args.join(" "))),
        }
    }

    fn prune(&mut self) -> Pruned {
        if self.preview {
            return Pruned::default();
        }
        self.install.prune(2)
    }

    fn run(&mut self, script: &Path, step: screen::Step<'_>) -> Result<ScriptRun, String> {
        if self.preview {
            return Err(PREVIEW_NOTE.into());
        }
        let p = script.to_string_lossy();
        captured_live(Path::new("pwsh"), &["-NoProfile", "-File", &p], step).map(|(output, exit_code)| ScriptRun { output, exit_code })
    }

    fn load_rows(&mut self) -> RowsConfig {
        crate::config::read_config(&self.config_path, &PathCtx::from_env()).rows_config()
    }

    fn save_rows(&mut self, rows: &[RowName], hidden: &BTreeMap<RowName, String>) -> Result<(), String> {
        if self.preview {
            return Err(PREVIEW_NOTE.into());
        }
        crate::config::save_rows(&self.config_path, rows, hidden).map_err(|e| e.to_string())
    }

    fn launcher_entries(&mut self) -> Vec<(char, String)> {
        self.launcher.entries()
    }

    fn launcher_key(&mut self, key: char) -> Verdict {
        if self.preview {
            return Verdict::done(PREVIEW_NOTE);
        }
        let text = self.launcher.act(key).unwrap_or_default();
        // The module's own words: a success says "updated" or "rolled back".
        if text.contains("fail") || text.contains("could not") {
            Verdict::failed(text)
        } else {
            Verdict::done(text)
        }
    }

    fn launcher_state(&mut self) -> String {
        self.launcher.state_text()
    }

    fn remember_mcp(&mut self, connected: usize, total: usize) {
        if !self.preview {
            mcp_seen::remember(&mcp_seen::path(&PathCtx::from_env().home), connected, total, now_ms());
        }
    }
}

/// The last MCP check maintenance ran, for the launch page's Claude Code panel; empty when none.
pub fn last_mcp_check(home: &std::path::Path, dot: char) -> String {
    mcp_seen::last_check(&mcp_seen::path(home), now_ms(), dot)
}

/// What the maintenance screen shows besides the install: collected warnings and the versions
/// the launch screen read.
#[derive(Debug, Clone, Copy)]
pub struct Shown<'a> {
    pub warnings: &'a [String],
    pub version: &'a VersionInfo,
}

/// `claude --version` reduced to its bare version; `None` when it cannot run.
fn installed_version() -> Option<String> {
    let exe = crate::invocation::resolve_claude().ok()?;
    let o = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&o.stdout);
    Some(release::version_token(text.lines().next().unwrap_or_default().trim_end())).filter(|v| !v.is_empty())
}

/// The installed version (`claude --version`) and the newest downloaded build, for the header.
/// A preview starts no process, so it knows only the newest build.
pub fn read_version() -> VersionInfo {
    let preview = is_preview();
    let install = Install::from_env(preview);
    VersionInfo {
        installed: if preview { None } else { installed_version() },
        newest: install::newest_build_name(&install.versions),
    }
}

/// The newest downloaded build's name: what `claude --version` says on a native install, without
/// starting a process (`--launcher-options` may not start one).
pub fn newest_build() -> Option<String> {
    install::newest_build_name(&Install::from_env(true).versions)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The update check, on its own thread while the launch screen is up: ask the release channel
/// (cached 30 minutes) and install a newer build, then re-read the model catalog so it describes
/// the build that will run. Its lines are printed after the screen closes. Never in preview,
/// never with `CLAUDE_AUTO_NO_UPDATE=1` (the caller checks both).
pub(crate) fn check_update(mut version: VersionInfo, gate: &InstallGate) -> UpdateFound {
    let start = std::time::Instant::now();
    let mut lines = Vec::new();
    let labels = check_into(&mut version, gate, &mut lines);
    UpdateFound {
        version,
        labels,
        defaults: None,
        lines,
        span: (start, std::time::Instant::now()),
    }
}

/// The check itself; `Some` labels when an install ran and the catalog was re-read.
fn check_into(version: &mut VersionInfo, gate: &InstallGate, lines: &mut Vec<Note>) -> Option<BTreeMap<String, String>> {
    let home = PathCtx::from_env().home;
    let claude_dir = home.join(".claude");
    let channel = release::auto_updates_channel(&claude_dir.join("settings.json"));
    let cache = claude_dir.join("claude-auto-update.json");
    let remote = match release::remote_version(&channel, &cache, now_ms(), &mut |url| win::https_get(url, 5000)) {
        Ok(r) => r,
        Err(e) => {
            lines.extend(release::skipped_line(&e).map(|l| Note::new(l, Fg::DarkYellow)));
            return None;
        }
    };
    let mut labels = None;
    // A launcher already quitting never starts an install it would then have to wait for.
    if release::update_available(version.installed.as_deref(), remote.as_deref()) && gate.begin_install() {
        lines.push(Note::new(
            format!(
                "  Claude Code {} is out (installed {}) - updating so the model list is current...",
                remote.as_deref().unwrap_or_default(),
                version.installed.as_deref().unwrap_or_default()
            ),
            Fg::DarkGray,
        ));
        let upd = Install::from_env(false).update(|| claude_output(&["update"]));
        *version = read_version();
        if version.installed == remote {
            lines.push(Note::new(
                format!("  updated to {}", version.installed.as_deref().unwrap_or_default()),
                Fg::DarkGray,
            ));
        } else {
            lines.push(Note::new(
                format!("  update did not finish: {} - press u for maintenance", upd.message),
                Fg::DarkYellow,
            ));
        }
        let (read, notes) = crate::env::read_model_catalog(false);
        labels = read;
        lines.extend(notes);
    }
    // The header names the channel's version when nothing newer has been downloaded yet.
    if let Some(r) = remote
        && (version.newest.is_none() || release::update_available(version.newest.as_deref(), Some(&r)))
    {
        version.newest = Some(r);
    }
    labels
}

/// The maintenance screen, opened by `u` from the launch screen and returning to it.
/// `launcher`: the self-update of this process, shared with the launch screen's check.
pub fn run_screen(
    term: &mut dyn Term,
    cfg: &Config,
    style: Style,
    launcher: &crate::self_update::Local,
    shown: &Shown<'_>,
) -> Result<(), UiStop> {
    let tm = term.logger().timings.clone();
    let load = tm.begin(&["load", "maintenance"]);
    let preview = is_preview();
    let mut ops = RealOps {
        install: Install::from_env(preview),
        preview,
        config_path: cfg.path.clone(),
        launcher: launcher.clone(),
    };
    let mut s = MaintScreen::new(&mut ops, &cfg.maintenance_actions, style).with_facts(shown.warnings, shown.version, &cfg.path);
    tm.end(load);
    crate::ui::screen_loop::run(&mut s, term).map(|_| ())
}
