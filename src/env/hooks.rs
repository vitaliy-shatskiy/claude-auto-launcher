//! `Invoke-LaunchHooks`: config-listed scripts after the account choice, in order, each
//! best-effort. A hook prints what it wants; a failure is one line; nothing stops the session.

use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::Command;

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
    let leaf = Path::new(hook)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
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
    for (k, v) in &env.vars {
        match v {
            Some(val) => cmd.env(k, val),
            None => cmd.env_remove(k),
        };
    }
    let out = match cmd.creation_flags(CREATE_NO_WINDOW).output() {
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

/// Every hook in order; the environment the session will get is the hook's too.
pub(crate) fn run(hooks: &[String], env: &ChildEnv) -> Vec<Note> {
    hooks.iter().filter(|h| !h.is_empty()).flat_map(|h| run_one(h, env)).collect()
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
        let lines: Vec<String> = run(&[one, two, missing, four, three], &env).into_iter().map(|n| n.text).collect();
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
}
