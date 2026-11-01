//! `Invoke-LaunchHooks`: config-listed scripts after the account choice, all at once, each
//! best-effort, printed in config order. A hook prints what it wants; a failure is one line;
//! nothing stops the session.

use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

use super::{CREATE_NO_WINDOW, Note};
use crate::out::Fg;
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

/// One hook: its lines, or the one line that says why not.
fn run_one(hook: &str, env: &ChildEnv) -> Vec<Note> {
    let leaf = leaf(hook);
    let warn = |t: String| vec![Note::new(t, Fg::DarkYellow)];
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
    match out.status.code().unwrap_or(1) {
        0 => text.into_iter().map(|t| Note::new(t, Fg::DarkGray)).collect(),
        code => {
            let tail = text
                .iter()
                .skip(text.len().saturating_sub(2))
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ");
            warn(format!("  hook {leaf} exit {code}: {tail}"))
        }
    }
}

/// Every hook; the environment the session will get is the hook's too.
pub(crate) fn run(chains: &[Vec<String>], env: &ChildEnv) -> Vec<Note> {
    run_with(chains, env, run_one)
}

/// `run` with the per-hook runner as a seam. Every chain starts at once on its own thread and
/// runs its hooks in order (hooks that touch the same files share a chain), the phase returns
/// only when all have ended, and the lines come out in config order.
fn run_with<F>(chains: &[Vec<String>], env: &ChildEnv, one: F) -> Vec<Note>
where
    F: Fn(&str, &ChildEnv) -> Vec<Note> + Sync,
{
    let one = &one;
    std::thread::scope(|s| {
        let running: Vec<_> = chains
            .iter()
            .map(|chain| s.spawn(move || chain.iter().filter(|h| !h.is_empty()).flat_map(|h| guarded(h, env, one)).collect()))
            .collect();
        running
            .into_iter()
            .flat_map(|t| t.join().unwrap_or_else(|_| Vec::<Note>::new()))
            .collect()
    })
}

/// One hook on its own thread, so a panic is one line and the rest of its chain still runs.
fn guarded<F>(hook: &str, env: &ChildEnv, one: &F) -> Vec<Note>
where
    F: Fn(&str, &ChildEnv) -> Vec<Note> + Sync,
{
    std::thread::scope(|s| s.spawn(|| one(hook, env)).join())
        .unwrap_or_else(|_| vec![Note::new(format!("  hook {} failed: it panicked", leaf(hook)), Fg::DarkYellow)])
}

fn leaf(hook: &str) -> String {
    Path::new(hook)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_run_in_order_and_a_failure_is_one_line() {
        let dir = std::env::temp_dir().join(format!("claude-auto-rs-hooks-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let put = |n: &str, body: &str| {
            let p = dir.join(n);
            std::fs::write(&p, body).unwrap();
            p.to_string_lossy().into_owned()
        };
        let one = put("one.ps1", "Write-Output \"one ran\"; Write-Output \"acct=$env:CLAUDE_CONFIG_DIR\"");
        let two = put("two.ps1", "Write-Output \"two broke\"; exit 3");
        let three = put("three.ps1", "Write-Output \"three ran\"");
        let four = put("four.hook", "not a script");
        let missing = dir.join("missing.ps1").to_string_lossy().into_owned();
        let mut env = ChildEnv::default();
        env.set("CLAUDE_CONFIG_DIR", r"C:\fixture-root");
        let lines: Vec<String> = run(&singles(&[&one, &two, &missing, &four, &three]), &env)
            .into_iter()
            .map(|n| n.text)
            .collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(lines.contains(&"one ran".to_owned()), "hook stdout is printed verbatim: {lines:?}");
        assert!(
            lines.contains(&r"acct=C:\fixture-root".to_owned()),
            "a hook sees the session's environment"
        );
        assert_eq!(
            lines.iter().filter(|l| l.trim_start().starts_with("hook two.ps1 exit 3")).count(),
            1
        );
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.contains("missing.ps1") && l.contains("not found"))
                .count(),
            1
        );
        assert_eq!(
            lines.iter().filter(|l| l.contains("four.hook")).count(),
            1,
            "an unknown extension is refused, one line"
        );
        assert!(lines.iter().any(|l| l.contains("four.hook") && l.contains("unsupported extension")));
        assert!(lines.contains(&"three ran".to_owned()), "later hooks still run");
        assert!(run(&[], &env).is_empty(), "an empty list prints nothing");
    }

    /// Two-party rendezvous: each fake hook checks in and waits for the other. Hooks run one
    /// after another never meet, so the first gives up after the bound and says so.
    #[derive(Default)]
    struct Meet {
        arrived: std::sync::Mutex<usize>,
        cv: std::sync::Condvar,
    }

    impl Meet {
        fn arrive(&self) -> bool {
            let mut n = self.arrived.lock().unwrap();
            *n += 1;
            self.cv.notify_all();
            let (n, _) = self
                .cv
                .wait_timeout_while(n, std::time::Duration::from_millis(250), |n| *n < 2)
                .unwrap();
            *n >= 2
        }
    }

    fn note(t: &str) -> Vec<Note> {
        vec![Note::new(t, Fg::DarkGray)]
    }

    /// Each path its own top-level entry, as a plain string in the config gives.
    fn singles(hooks: &[&str]) -> Vec<Vec<String>> {
        hooks.iter().map(|h| vec![(*h).to_owned()]).collect()
    }

    #[test]
    fn a_chain_runs_in_order_while_other_entries_run_beside_it() {
        // `c` meets `a`: the chain runs beside the other entry. `b` starts only after `a` ended.
        let meet = Meet::default();
        let events = std::sync::Mutex::new(Vec::<String>::new());
        let chains = vec![vec!["a".to_owned(), "b".to_owned()], vec!["c".to_owned()]];
        let notes = run_with(&chains, &ChildEnv::default(), |h, _| {
            events.lock().unwrap().push(format!("start {h}"));
            let outcome = if h == "b" {
                "-"
            } else if meet.arrive() {
                "met"
            } else {
                "alone"
            };
            events.lock().unwrap().push(format!("end {h}"));
            note(&format!("{h} {outcome}"))
        });
        let lines: Vec<String> = notes.into_iter().map(|n| n.text).collect();
        assert_eq!(lines, ["a met", "b -", "c met"], "config order, flattened");
        let ev = events.into_inner().unwrap();
        let at = |e: &str| ev.iter().position(|x| x == e).unwrap();
        assert!(at("end a") < at("start b"), "b waited for a: {ev:?}");
    }

    #[test]
    fn hooks_run_at_the_same_time() {
        let meet = Meet::default();
        let hooks = singles(&["a.ps1", "b.ps1"]);
        let lines: Vec<String> = run_with(&hooks, &ChildEnv::default(), |h, _| {
            note(&format!("{h} {}", if meet.arrive() { "met" } else { "alone" }))
        })
        .into_iter()
        .map(|n| n.text)
        .collect();
        assert_eq!(lines, ["a.ps1 met", "b.ps1 met"], "both hooks were running at once");
    }

    #[test]
    fn output_keeps_config_order_whoever_finishes_first() {
        // The first hook finishes only after the last has returned its lines.
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let done_tx = std::sync::Mutex::new(done_tx);
        let done_rx = std::sync::Mutex::new(done_rx);
        let hooks = singles(&["first.ps1", "", "last.ps1"]);
        let notes = run_with(&hooks, &ChildEnv::default(), |h, _| {
            if h == "first.ps1" {
                let got = done_rx.lock().unwrap().recv_timeout(std::time::Duration::from_millis(250));
                return note(if got.is_ok() { "first 1" } else { "first ran alone" })
                    .into_iter()
                    .chain(note("first 2"))
                    .collect();
            }
            done_tx.lock().unwrap().send(()).unwrap();
            note("last 1")
        });
        let lines: Vec<String> = notes.into_iter().map(|n| n.text).collect();
        assert_eq!(lines, ["first 1", "first 2", "last 1"]);
    }

    #[test]
    fn a_hook_that_panics_is_one_line_and_the_others_still_print() {
        let hooks = vec![vec!["ok.ps1".to_owned()], vec!["boom.ps1".to_owned(), "after.ps1".to_owned()]];
        let notes = run_with(&hooks, &ChildEnv::default(), |h, _| {
            assert_ne!(h, "boom.ps1", "fake hook failure");
            note(&format!("{h} ran"))
        });
        let lines: Vec<(String, Fg)> = notes.into_iter().map(|n| (n.text, n.fg)).collect();
        assert_eq!(lines.first(), Some(&("ok.ps1 ran".to_owned(), Fg::DarkGray)));
        assert_eq!(lines.get(1).map(|l| &l.1), Some(&Fg::DarkYellow));
        assert!(
            lines
                .get(1)
                .is_some_and(|l| l.0.contains("hook boom.ps1") && l.0.contains("failed"))
        );
        assert_eq!(lines.get(2), Some(&("after.ps1 ran".to_owned(), Fg::DarkGray)));
        assert_eq!(lines.len(), 3);
    }

    /// Points this process's stdin at `file` until dropped: what `Stdio::inherit` hands a child.
    struct StdinSwap(windows_sys::Win32::Foundation::HANDLE);

    impl StdinSwap {
        #[expect(unsafe_code, reason = "GetStdHandle/SetStdHandle FFI")]
        fn to(file: &std::fs::File) -> Self {
            use std::os::windows::io::AsRawHandle as _;
            use windows_sys::Win32::System::Console::{GetStdHandle, STD_INPUT_HANDLE, SetStdHandle};
            // SAFETY: reads the process's current stdin handle.
            let old = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
            // SAFETY: `file` outlives the swap, which restores `old` on drop.
            assert_ne!(unsafe { SetStdHandle(STD_INPUT_HANDLE, file.as_raw_handle()) }, 0);
            Self(old)
        }
    }

    impl Drop for StdinSwap {
        #[expect(unsafe_code, reason = "SetStdHandle FFI")]
        fn drop(&mut self) {
            use windows_sys::Win32::System::Console::{STD_INPUT_HANDLE, SetStdHandle};
            // SAFETY: puts back the handle `to` read.
            unsafe { SetStdHandle(STD_INPUT_HANDLE, self.0) };
        }
    }

    #[test]
    fn a_hook_reads_the_launchers_stdin() {
        let dir = std::env::temp_dir().join(format!("claude-auto-rs-hook-stdin-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.txt");
        std::fs::write(&input, "typed-into-the-console\n").unwrap();
        let hook = dir.join("reads.ps1");
        std::fs::write(&hook, "$l = [Console]::In.ReadLine(); Write-Output \"got=$l\"").unwrap();
        let file = std::fs::File::open(&input).unwrap();
        let lines: Vec<String> = {
            let _swap = StdinSwap::to(&file);
            run(&[vec![hook.to_string_lossy().into_owned()]], &ChildEnv::default())
                .into_iter()
                .map(|n| n.text)
                .collect()
        };
        drop(file);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            lines,
            vec!["got=typed-into-the-console".to_owned()],
            "the hook got the line piped in"
        );
    }
}
