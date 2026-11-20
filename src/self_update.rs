//! Self-update from the launcher's GitHub releases (config `selfUpdate`, `updateRepo`): a daily
//! background check, one notice line on the launch screen, update and roll-back keys on the
//! maintenance screen. Exports `Mode`, `Repo`, `Watch`, `Local`; the check lives in
//! `check`, download and swap in `swap`.

mod check;
mod swap;
mod tidy;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::background::Background;
use crate::maintenance::FetchFailed;
use check::{DAY_MS, Failed, Release, State, Version};
use swap::{Files, RealFiles};

/// Where releases come from unless config `updateRepo` names a fork.
pub const DEFAULT_REPO: &str = "vitaliy-shatskiy/claude-auto-launcher";
const STATE_FILE: &str = "claude-auto-self-update.json";
/// The maintenance keys; config reserves both, so no configured action takes them.
pub const UPDATE_KEY: char = 'c';
pub const ROLLBACK_KEY: char = 'b';
const CHECK_TIMEOUT_MS: i32 = 5000;
const DOWNLOAD_TIMEOUT_MS: i32 = 30_000;

/// Config `selfUpdate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Check daily and show a line; the user updates from maintenance.
    Notify,
    /// Check daily and update in the background.
    Auto,
    /// No check, no network.
    Off,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Self> {
        [("notify", Self::Notify), ("auto", Self::Auto), ("off", Self::Off)]
            .into_iter()
            .find_map(|(k, m)| k.eq_ignore_ascii_case(s.trim()).then_some(m))
    }
}

/// Config `updateRepo`: a GitHub `owner/name`, checked once here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo(String);

impl Repo {
    /// `owner/name` of letters, digits, `-`, `_` and `.`; anything else is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (owner, name) = s.split_once('/')?;
        let part = |p: &str| {
            !p.is_empty() && p.len() <= 100 && p != "." && p != ".." && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        };
        (part(owner) && part(name)).then(|| Self(s.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn api_url(&self) -> String {
        format!("https://api.github.com/repos/{}/releases/latest", self.0)
    }

    /// Lower case: GitHub serves the repo's own casing, the config may differ.
    fn download_prefix(&self) -> String {
        format!("https://github.com/{}/releases/download/", self.0.to_ascii_lowercase())
    }
}

impl Default for Repo {
    fn default() -> Self {
        Self(DEFAULT_REPO.to_owned())
    }
}

/// An HTTPS GET with a size cap; the seam that keeps tests off the network.
pub(crate) trait Net: Send + Sync {
    fn get(&self, url: &str, cap: usize) -> Result<Vec<u8>, FetchFailed>;
}

/// `WinHTTP`, through the maintenance module's request.
#[derive(Debug, Clone, Copy)]
struct WinNet;

impl Net for WinNet {
    fn get(&self, url: &str, cap: usize) -> Result<Vec<u8>, FetchFailed> {
        let timeout = if cap > 1024 * 1024 { DOWNLOAD_TIMEOUT_MS } else { CHECK_TIMEOUT_MS };
        crate::maintenance::win::https_bytes(url, timeout, cap)
    }
}

/// What kind of start this is: a preview makes no request and writes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    Launch,
    Preview,
}

/// The exe, the state file and the mode: what both the check and the maintenance keys act on.
#[derive(Debug, Clone)]
pub struct Local {
    pub(crate) mode: Mode,
    pub(crate) repo: Repo,
    pub(crate) exe: PathBuf,
    pub(crate) state: PathBuf,
    /// Set while an update or a roll back runs; shared by every clone, so the background update
    /// and the maintenance keys of one process never swap at the same time.
    pub(crate) busy: Arc<AtomicBool>,
}

impl Local {
    /// This process: `mode` forced off by `CLAUDE_AUTO_NO_SELF_UPDATE=1`, a preview, or an exe
    /// path that cannot be read.
    pub fn from_env(mode: Mode, repo: &Repo, home: &Path, start: Start) -> Self {
        let exe = std::env::current_exe().ok();
        let off = start == Start::Preview || std::env::var("CLAUDE_AUTO_NO_SELF_UPDATE").as_deref() == Ok("1") || exe.is_none();
        Self {
            mode: if off { Mode::Off } else { mode },
            repo: repo.clone(),
            exe: exe.unwrap_or_default(),
            state: home.join(".claude").join(STATE_FILE),
            busy: Arc::default(),
        }
    }

    /// The same exe and state with no check: a start that shows no screen.
    pub(crate) fn off(&self) -> Self {
        Self {
            mode: Mode::Off,
            ..self.clone()
        }
    }

    /// A release newer than this build, from the state file only (no network).
    fn offered(&self) -> Option<Release> {
        let state = State::read(&self.state);
        check::newer(state.latest, Version::current()).filter(|r| state.installed != Some(r.version))
    }

    /// The maintenance entries: update when a newer release is known (not with `selfUpdate` off),
    /// roll back when a previous build is kept.
    pub(crate) fn entries(&self) -> Vec<(char, String)> {
        let mut out = Vec::new();
        if self.mode != Mode::Off
            && let Some(r) = self.offered()
        {
            out.push((UPDATE_KEY, format!("update claude-auto to {}", r.version)));
        }
        if swap::sibling(&self.exe, ".old").exists() {
            let to = State::read(&self.state)
                .previous
                .map_or_else(|| "the previous build".to_owned(), |v| v.to_string());
            out.push((ROLLBACK_KEY, format!("roll back to {to}")));
        }
        out
    }

    /// A maintenance key: the status line it leaves, `None` when it is no key of this module.
    pub(crate) fn act(&self, key: char) -> Option<String> {
        self.act_with(key, &WinNet, &RealFiles)
    }

    fn act_with(&self, key: char, net: &dyn Net, files: &dyn Files) -> Option<String> {
        match key {
            UPDATE_KEY => {
                let r = self.offered()?;
                Some(match update(self, &r, net, files, &Mutex::new(false)) {
                    Ok(()) => format!("updated to {} - takes effect next start", r.version),
                    Err(e) => format!("claude-auto update failed: {e}"),
                })
            }
            ROLLBACK_KEY => Some(
                match Busy::take(&self.busy)
                    .ok_or_else(|| ALREADY_RUNNING.to_owned())
                    .and_then(|_busy| swap::roll_back(&self.exe, files))
                {
                    Ok(()) => {
                        let mut s = State::read(&self.state);
                        let was = s.installed.or_else(Version::current);
                        s.installed = s.previous;
                        s.previous = was;
                        // A roll back from a newer build holds it out of `auto`; rolling forward lifts it.
                        s.rolled_back_from = was.filter(|w| s.installed.is_none_or(|i| *w > i));
                        s.write(&self.state);
                        "rolled back - takes effect next start".to_owned()
                    }
                    Err(e) => format!("roll back failed: {e}"),
                },
            ),
            _ => None,
        }
    }
}

const ALREADY_RUNNING: &str = "an update is already running";
const QUITTING: &str = "the launcher is quitting";

/// The one update or roll back of this process in flight; dropping it frees the slot.
struct Busy<'a>(&'a AtomicBool);

impl<'a> Busy<'a> {
    /// None when another update or roll back holds the slot.
    fn take(flag: &'a AtomicBool) -> Option<Self> {
        // Never `then_some(Self(flag))`: the value built for a refusal is dropped, and its drop would
        // free the slot the other caller holds.
        if flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            Some(Self(flag))
        } else {
            None
        }
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Download, verify and swap, then record what the exe and `.old` now hold. `closed` set = the
/// launcher is quitting: the swap does not start, so an exit never cuts it in half.
fn update(local: &Local, release: &Release, net: &dyn Net, files: &dyn Files, closed: &Mutex<bool>) -> Result<(), String> {
    let _busy = Busy::take(&local.busy).ok_or(ALREADY_RUNNING)?;
    let new = swap::download(&local.exe, release, net)?;
    let quitting = closed.lock().unwrap_or_else(PoisonError::into_inner);
    if *quitting {
        drop(std::fs::remove_file(&new));
        return Err(QUITTING.into());
    }
    swap::swap_in(&local.exe, &new, files)?;
    let mut s = State::read(&local.state);
    s.previous = Version::current();
    s.installed = Some(release.version);
    s.rolled_back_from = None;
    s.failed = None;
    s.write(&local.state);
    Ok(())
}

/// What the background check found, as the launch screen's line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Notice {
    Available(Version),
    Updated(Version),
    Failed(Version, String),
}

impl Notice {
    pub(crate) fn line(&self) -> String {
        match self {
            Self::Available(v) => format!("claude-auto {v} available · u → update"),
            Self::Updated(v) => format!("claude-auto updated to {v} - takes effect next start"),
            Self::Failed(v, why) => format!("claude-auto {v} available · u → update (auto update failed: {why})"),
        }
    }
}

/// The check (and in `auto` the update) for one start. `Off` makes no request.
fn run(local: &Local, now_ms: i64, net: &dyn Net, files: &dyn Files, closed: &Mutex<bool>) -> Option<Notice> {
    let auto = match local.mode {
        Mode::Off => return None,
        Mode::Notify => false,
        Mode::Auto => true,
    };
    let r = check::newer(check::latest(&local.state, &local.repo, now_ms, net), Version::current())?;
    let state = State::read(&local.state);
    if state.installed == Some(r.version) {
        // Already swapped in by an earlier start's background update; it waits for this restart.
        return Some(Notice::Updated(r.version));
    }
    // The user rolled this build back: only the update key installs it again.
    let held = state.rolled_back_from.is_some_and(|v| r.version <= v);
    if !auto || held {
        return Some(Notice::Available(r.version));
    }
    // A failure that would repeat (a bad checksum, a locked folder) costs one download a day, not
    // ~10 MB on every start.
    if let Some(f) = state
        .failed
        .filter(|f| f.version == r.version && (0..DAY_MS).contains(&now_ms.saturating_sub(f.at_ms)))
    {
        return Some(Notice::Failed(r.version, f.why));
    }
    Some(match update(local, &r, net, files, closed) {
        Ok(()) => Notice::Updated(r.version),
        Err(e) => {
            if e != QUITTING && e != ALREADY_RUNNING {
                let mut s = State::read(&local.state);
                s.failed = Some(Failed {
                    version: r.version,
                    at_ms: now_ms,
                    why: e.clone(),
                });
                s.write(&local.state);
            }
            Notice::Failed(r.version, e)
        }
    })
}

/// The check behind the launch screen: started at process start, read without blocking. A quit
/// never waits for a download, and never cuts a swap in half (the swap holds `closed`).
#[derive(Debug)]
pub struct Watch {
    job: Option<Background<Option<Notice>>>,
    closed: Arc<Mutex<bool>>,
    told: bool,
}

impl Watch {
    /// Starts the check for this process on its own thread; nothing at all with mode `Off`.
    pub fn start(local: Local, now_ms: i64) -> Self {
        Self::start_with(local, now_ms, Arc::new(WinNet), Arc::new(RealFiles))
    }

    pub(crate) fn start_with(local: Local, now_ms: i64, net: Arc<dyn Net>, files: Arc<dyn Files>) -> Self {
        let closed = Arc::new(Mutex::new(false));
        let job = (local.mode != Mode::Off).then(|| {
            let theirs = Arc::clone(&closed);
            Background::start("self-update", move || {
                tidy::sweep_partials(&local.exe, now_ms, &tidy::alive_since);
                tidy::forget_stale_install(&local, Version::current(), tidy::started_ms());
                run(&local, now_ms, net.as_ref(), files.as_ref(), &theirs)
            })
            .unjoined()
        });
        Self { job, closed, told: false }
    }

    /// `true` once, when a notice first comes in: the screen redraws.
    pub(crate) fn news(&mut self) -> bool {
        if self.told {
            return false;
        }
        self.told = matches!(self.job.as_mut().and_then(Background::poll), Some(Ok(Some(_))));
        self.told
    }

    /// The launch screen's line, once the check has answered.
    pub(crate) fn line(&self, ascii: bool) -> Option<String> {
        match self.job.as_ref().and_then(Background::done) {
            Some(Ok(Some(n))) if ascii => Some(n.line().replace('·', "-").replace('→', "->")),
            Some(Ok(Some(n))) => Some(n.line()),
            _ => None,
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        *self.closed.lock().unwrap_or_else(PoisonError::into_inner) = true;
    }
}

#[cfg(test)]
mod tests;
