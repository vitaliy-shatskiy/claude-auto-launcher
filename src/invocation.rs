//! What gets handed to `claude` and how it runs (`Env.ps1` Get-ClaudeInvocation,
//! Resolve-ClaudeExecutable, the launch marker; `claude-auto.ps1` the exec): same console, wait,
//! the child's exit code.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::types::ChildEnv;

/// `claude --help` commands plus the background-session CLI: administrative calls, not sessions.
const SUBCOMMANDS: [&str; 21] = [
    "agents",
    "auth",
    "auto-mode",
    "doctor",
    "gateway",
    "import",
    "install",
    "mcp",
    "plugin",
    "plugins",
    "project",
    "setup-token",
    "ultrareview",
    "update",
    "upgrade",
    "attach",
    "stop",
    "config",
    "login",
    "logout",
    "migrate-installer",
];

/// `Test-ClaudeSubcommand` (`-in`: case-insensitive).
pub fn is_subcommand(token: &str) -> bool {
    !token.is_empty() && SUBCOMMANDS.iter().any(|s| s.eq_ignore_ascii_case(token))
}

/// `Get-ClaudeInvocation`: the single source of truth for argv. `--mcp-config` goes LAST (it is
/// variadic); a subcommand gets no MCP flags at all.
pub fn claude_invocation(launch_args: &[String], mcp_configs: &[String]) -> Vec<String> {
    let mut out = launch_args.to_vec();
    if out.first().is_some_and(|t| is_subcommand(t)) {
        return out;
    }
    let configs: Vec<&String> = mcp_configs.iter().filter(|c| !c.is_empty()).collect();
    if !configs.is_empty() {
        out.push("--mcp-config".into());
        out.extend(configs.into_iter().cloned());
    }
    out
}

/// `Get-Command <name> -CommandType Application`: PATH order, then PATHEXT order per directory.
pub fn find_on_path(name: &str, path: &str, pathext: &str) -> Option<PathBuf> {
    let exts: Vec<&str> = pathext.split(';').filter(|e| !e.is_empty()).collect();
    for dir in path.split(';').filter(|d| !d.trim().is_empty()) {
        let dir = Path::new(dir.trim().trim_matches('"'));
        for ext in &exts {
            let p = dir.join(format!("{name}{}", ext.to_ascii_lowercase()));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// `Resolve-ClaudeExecutable`: the path, or the message the launcher prints before `exit 1`.
pub fn resolve_claude() -> Result<PathBuf, &'static str> {
    let path = std::env::var("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    find_on_path("claude", &path, &pathext).ok_or("claude is not on PATH - install Claude Code first")
}

/// Runs the child in this console and waits. `CLAUDE_AUTO_LAUNCHED=1` is set for the child only,
/// so a bare `claude` typed later in the same terminal is never marked.
pub fn run_child(exe: &Path, argv: &[String], env: &ChildEnv) -> std::io::Result<i32> {
    let mut cmd = Command::new(exe);
    cmd.args(argv);
    for (k, v) in &env.vars {
        match v {
            Some(val) => cmd.env(k, val),
            None => cmd.env_remove(k),
        };
    }
    cmd.env("CLAUDE_AUTO_LAUNCHED", "1");
    // Ctrl+C belongs to the child while it runs; this process only waits for it.
    crate::sys::ignore_ctrl_c();
    let status = cmd.status()?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn mcp_config_goes_last_and_whole() {
        assert_eq!(
            claude_invocation(&v(&["--resume", "abc"]), &v(&[r"C:\a.json", "", r"C:\b.json"])),
            v(&["--resume", "abc", "--mcp-config", r"C:\a.json", r"C:\b.json"])
        );
    }

    #[test]
    fn one_config_is_one_argument_not_characters() {
        assert_eq!(claude_invocation(&[], &v(&[r"C:\Users\x\mcp.json"])).len(), 2);
    }

    #[test]
    fn subcommands_get_no_mcp_flags_but_prompts_do() {
        assert_eq!(claude_invocation(&v(&["mcp", "list"]), &v(&["c.json"])), v(&["mcp", "list"]));
        assert_eq!(claude_invocation(&v(&["Doctor"]), &v(&["c.json"])), v(&["Doctor"]));
        assert_eq!(claude_invocation(&v(&["fix the test"]), &v(&["c.json"])).len(), 3);
        assert_eq!(claude_invocation(&v(&["--version"]), &[]), v(&["--version"]));
    }

    #[test]
    fn path_search_honours_dir_then_ext_order() {
        let base = std::env::temp_dir().join(format!("claude-auto-rs-path-{}", crate::log::new_run_id()));
        let (a, b) = (base.join("a"), base.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("claude.exe"), b"").unwrap();
        std::fs::write(a.join("claude.cmd"), b"").unwrap();
        let path = format!("{};{}", a.display(), b.display());
        assert_eq!(find_on_path("claude", &path, ".EXE;.CMD"), Some(a.join("claude.cmd")));
        assert_eq!(find_on_path("nope", &path, ".EXE"), None);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
