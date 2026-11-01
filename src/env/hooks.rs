//! `Invoke-LaunchHooks`: config-listed scripts after the account choice, all at once, each
//! best-effort, printed in config order. A hook prints what it wants; a failure is one line;
//! nothing stops the session.

use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use super::{CREATE_NO_WINDOW, Note};
use crate::out::Fg;
use crate::timing::Timings;
use crate::types::ChildEnv;

/// The interpreter per extension. Never the file itself: an unlisted extension would
/// `ShellExecute` its desktop association.
fn command_for(ext: &str, hook: &str) -> Option<Command> {
    let (exe, pre): (&str, &[&str]) = match ext {
        ".ps1" => ("pwsh", &["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]),
        ".js" | ".mjs" => ("node", &[]),
        ".cmd" => ("cmd", &["/c"]),
        _ => return None,
    };
    let mut c = Command::new(exe);
    c.args(pre).arg(hook);
    Some(c)
}

/// One hook's outcome: its lines and, when it ran to an end, its exit code.
struct Ran {
    notes: Vec<Note>,
    exit: Option<i32>,
}

impl From<Vec<Note>> for Ran {
    fn from(notes: Vec<Note>) -> Self {
        Self { notes, exit: None }
    }
}

/// One hook's wall time, measured on its chain's thread.
struct Clock {
    leaf: String,
    start: Instant,
    end: Instant,
    exit: Option<i32>,
}

/// One entry's wall time and its hooks'.
struct ChainClock {
    start: Instant,
    end: Instant,
    hooks: Vec<Clock>,
}

/// One hook: its lines, or the one line that says why not.
fn run_one(hook: &str, env: &ChildEnv) -> Ran {
    let leaf = leaf(hook);
    let warn = |t: String| Ran::from(vec![Note::new(t, Fg::DarkYellow)]);
    if !Path::new(hook).exists() {
        return warn(format!("  hook {leaf} not found at {hook}"));
    }
    let ext = Path::new(hook)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    let Some(mut cmd) = command_for(&ext, hook) else {
        return warn(format!(
            "  hook {leaf}: unsupported extension '{ext}' (use .ps1, .js, .mjs or .cmd)"
        ));
    };
    env.apply(&mut cmd);
    // The console's stdin, as PowerShell gave its hooks: `output()` alone would hand them a
    // closed stream.
    let out = match cmd.creation_flags(CREATE_NO_WINDOW).stdin(Stdio::inherit()).output() {
        Ok(o) => o,
        Err(e) => {
            return warn(format!(
                "  hook {leaf} failed: {}",
                e.to_string().replace("\r\n", " | ").replace('\n', " | ")
            ));
        }
    };
    let text: Vec<String> = [&out.stdout, &out.stderr]
        .iter()
        .flat_map(|b| String::from_utf8_lossy(b).lines().map(str::to_owned).collect::<Vec<_>>())
        .filter(|l| !l.trim().is_empty())
        .collect();
    let code = out.status.code().unwrap_or(1);
    let notes = match code {
        0 => text.into_iter().map(|t| Note::new(t, Fg::DarkGray)).collect(),
        code => {
            let tail = text
                .iter()
                .skip(text.len().saturating_sub(2))
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ");
            vec![Note::new(format!("  hook {leaf} exit {code}: {tail}"), Fg::DarkYellow)]
        }
    };
    Ran { notes, exit: Some(code) }
}

/// Every hook; the environment the session will get is the hook's too. With `tm` on, each
/// hook and each multi-hook chain becomes a span under the open one (`hooks`).
pub(crate) fn run(chains: &[Vec<String>], env: &ChildEnv, tm: &Timings) -> Vec<Note> {
    run_with(chains, env, tm, run_one)
}

/// `run` with the per-hook runner as a seam.
fn run_with<F, R>(chains: &[Vec<String>], env: &ChildEnv, tm: &Timings, one: F) -> Vec<Note>
where
    F: Fn(&str, &ChildEnv) -> R + Sync,
    R: Into<Ran>,
{
    let (notes, clocks) = run_clocked(chains, env, tm.is_on(), &one);
    record(tm, &clocks);
    notes
}

/// Every chain starts at once on its own thread and runs its hooks in order (hooks that touch
/// the same files share a chain), the phase returns only when all have ended, and the lines come
/// out in config order. `timed` off reads no clock and returns no clocks.
fn run_clocked<F, R>(chains: &[Vec<String>], env: &ChildEnv, timed: bool, one: &F) -> (Vec<Note>, Vec<ChainClock>)
where
    F: Fn(&str, &ChildEnv) -> R + Sync,
    R: Into<Ran>,
{
    std::thread::scope(|s| {
        let running: Vec<_> = chains
            .iter()
            .map(|chain| s.spawn(move || run_chain(chain, env, timed, one)))
            .collect();
        let mut notes = Vec::new();
        let mut clocks = Vec::new();
        for t in running {
            let (n, c) = t.join().unwrap_or_default();
            notes.extend(n);
            clocks.extend(c);
        }
        (notes, clocks)
    })
}

fn run_chain<F, R>(chain: &[String], env: &ChildEnv, timed: bool, one: &F) -> (Vec<Note>, Option<ChainClock>)
where
    F: Fn(&str, &ChildEnv) -> R + Sync,
    R: Into<Ran>,
{
    let start = timed.then(Instant::now);
    let mut notes = Vec::new();
    let mut hooks = Vec::new();
    for h in chain.iter().filter(|h| !h.is_empty()) {
        let t0 = timed.then(Instant::now);
        let ran = guarded(h, env, one);
        if let Some(t0) = t0 {
            hooks.push(Clock {
                leaf: leaf(h),
                start: t0,
                end: Instant::now(),
                exit: ran.exit,
            });
        }
        notes.extend(ran.notes);
    }
    let clock = start.map(|start| ChainClock {
        start,
        end: Instant::now(),
        hooks,
    });
    (notes, clock)
}

/// One hook on its own thread, so a panic is one line and the rest of its chain still runs.
fn guarded<F, R>(hook: &str, env: &ChildEnv, one: &F) -> Ran
where
    F: Fn(&str, &ChildEnv) -> R + Sync,
    R: Into<Ran>,
{
    std::thread::scope(|s| s.spawn(|| one(hook, env).into()).join()).unwrap_or_else(|_| {
        Ran::from(vec![Note::new(
            format!("  hook {} failed: it panicked", leaf(hook)),
            Fg::DarkYellow,
        )])
    })
}

/// `hooks:<file>` per hook; a chain of two or more also gets `hooks:chain:<a>+<b>` holding its
/// hooks. A single hook's chain would only repeat it.
fn record(tm: &Timings, chains: &[ChainClock]) {
    for c in chains {
        let parent = (c.hooks.len() > 1).then(|| {
            let names: Vec<&str> = c.hooks.iter().map(|h| h.leaf.as_str()).collect();
            tm.add(&["hooks", "chain", &names.join("+")], None, c.start, c.end, None)
        });
        for h in &c.hooks {
            tm.add(&["hooks", &h.leaf], parent, h.start, h.end, h.exit);
        }
    }
}

fn leaf(hook: &str) -> String {
    Path::new(hook)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
