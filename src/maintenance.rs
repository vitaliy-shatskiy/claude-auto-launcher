//! Maintenance screen, Claude Code versions and the update check (`Maintenance.ps1`,
//! `Invoke-MaintenanceScreen`, `Get-MaintenanceFrame`). Entry points for `app.rs`; the install
//! logic lives in `install`, the release channel in `release`, the screen in `screen`.

mod install;
mod release;
mod screen;
mod win;

use std::path::Path;
use std::process::{Command, Stdio};

use crate::config::{MaintenanceAction, PathCtx};
use crate::frame::Style;
use crate::out::{self, Fg};
use crate::types::VersionInfo;
use crate::ui::{Exhausted, Term};
use install::{Install, InstallInfo, Pruned};
use screen::{MaintScreen, Ops, ScriptRun};

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

/// The machine behind the screen's keys. A preview reads the install and changes nothing.
#[derive(Debug)]
struct RealOps {
    install: Install,
    preview: bool,
}

impl Ops for RealOps {
    fn info(&mut self) -> InstallInfo {
        self.install.info()
    }

    fn update(&mut self) -> String {
        if self.preview {
            return PREVIEW_NOTE.into();
        }
        self.install.update(|| claude_output(&["update"])).message
    }

    fn rename_swap(&mut self) -> String {
        if self.preview {
            return PREVIEW_NOTE.into();
        }
        self.install.rename_swap().message
    }

    fn command_text(&mut self, args: &[&str]) -> String {
        if self.preview {
            return PREVIEW_NOTE.into();
        }
        match claude_output(args) {
            Ok(text) => screen::status_text(&text),
            Err(e) => format!("claude {} could not be started: {e}", args.join(" ")),
        }
    }

    fn prune(&mut self) -> Pruned {
        if self.preview {
            return Pruned::default();
        }
        self.install.prune(2)
    }

    fn run(&mut self, script: &Path) -> Result<ScriptRun, String> {
        if self.preview {
            return Err(PREVIEW_NOTE.into());
        }
        let p = script.to_string_lossy();
        captured(Path::new("pwsh"), &["-NoProfile", "-File", &p]).map(|(output, exit_code)| ScriptRun { output, exit_code })
    }
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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The pre-UI update check: ask the release channel (cached 30 minutes) and install a newer
/// build now, so the model catalog read after it describes the build that will run. Never in
/// preview, never with `CLAUDE_AUTO_NO_UPDATE=1` (the caller checks both).
pub fn update_before_launch(version: &mut VersionInfo) {
    let home = PathCtx::from_env().home;
    let claude_dir = home.join(".claude");
    let channel = release::auto_updates_channel(&claude_dir.join("settings.json"));
    let cache = claude_dir.join("claude-auto-update.json");
    let remote = release::remote_version(&channel, &cache, now_ms(), &mut |url| win::https_get(url, 5000));
    if release::update_available(version.installed.as_deref(), remote.as_deref()) {
        out::line(
            &format!(
                "  Claude Code {} is out (installed {}) - updating so the model list is current...",
                remote.as_deref().unwrap_or_default(),
                version.installed.as_deref().unwrap_or_default()
            ),
            Fg::DarkGray,
        );
        let upd = Install::from_env(false).update(|| claude_output(&["update"]));
        *version = read_version();
        if version.installed == remote {
            out::line(
                &format!("  updated to {}", version.installed.as_deref().unwrap_or_default()),
                Fg::DarkGray,
            );
        } else {
            out::line(
                &format!("  update did not finish: {} - press u for maintenance", upd.message),
                Fg::DarkYellow,
            );
        }
    }
    // The header names the channel's version when nothing newer has been downloaded yet.
    if let Some(r) = remote
        && (version.newest.is_none() || release::update_available(version.newest.as_deref(), Some(&r)))
    {
        version.newest = Some(r);
    }
}

/// `Invoke-MaintenanceScreen`, opened by `u` from the launch screen and returning to it.
pub fn run_screen(term: &mut dyn Term, actions: &[MaintenanceAction], style: Style) -> Result<(), Exhausted> {
    let preview = is_preview();
    let mut ops = RealOps {
        install: Install::from_env(preview),
        preview,
    };
    let mut s = MaintScreen::new(&mut ops, actions, style);
    crate::ui::screen_loop::run(&mut s, term).map(|_| ())
}
