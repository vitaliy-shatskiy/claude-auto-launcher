//! A claude-auto plugin (protocol 1) that tells claude which git branch it starts on. `preLaunch`
//! sets `CLAUDE_AUTO_GIT_BRANCH` to the launch directory's branch, read from `.git/HEAD` (no git
//! process); launch-screen key `g` shows the branch and how many entries `git status --porcelain`
//! lists. It is also a `projects` source that adds none. Build it with
//! `cargo build --example plugin_git_branch` and list the exe in config `plugins` (README, Plugins).

use std::io::Read as _;
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// `CREATE_NO_WINDOW`: git runs without flashing a console.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The git directory of `dir` or its nearest parent: a `.git` folder, or the folder a `.git` file
/// (a worktree, a submodule) names with `gitdir: <path>`.
fn git_dir(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find_map(|d| {
        let dot = d.join(".git");
        if dot.is_dir() {
            return Some(dot);
        }
        let text = std::fs::read_to_string(&dot).ok()?;
        let target = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
        Some(if target.is_absolute() { target } else { d.join(target) })
    })
}

/// The branch HEAD points at, or the short commit id of a detached HEAD.
fn branch(dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir(dir)?.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: ") {
        Some(r) => Some(r.strip_prefix("refs/heads/").unwrap_or(r).to_owned()),
        None => (!head.is_empty()).then(|| head.chars().take(7).collect()),
    }
}

/// How many entries `git status --porcelain` lists in `dir`; `None` = git did not run or failed.
fn dirty_count(dir: &Path) -> Option<usize> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.is_empty()).count())
}

/// The directory a request names, else the plugin's own working directory.
fn launch_dir(path: Option<&str>) -> PathBuf {
    path.filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default()
}

fn report(dir: &Path) -> String {
    match (branch(dir), dirty_count(dir)) {
        (None, _) => format!("{} is not in a git repository", dir.display()),
        (Some(b), Some(n)) => format!("{b}, {n} dirty file(s)"),
        (Some(b), None) => format!("{b} (git status failed)"),
    }
}

/// One answer per verb; an unknown verb exits 2 (the launcher warns and goes on).
fn answer(verb: &str, input: &Value) -> Option<Value> {
    Some(match verb {
        "describe" => json!({
            "protocol": 1,
            "actions": [{ "key": "g", "label": "git branch", "screen": "launch" }],
            "hooks": ["preLaunch"],
            "sources": ["projects"],
        }),
        "preLaunch" => match branch(&launch_dir(input["choice"]["project"].as_str())) {
            Some(b) => json!({ "env": { "CLAUDE_AUTO_GIT_BRANCH": b } }),
            None => json!({}),
        },
        "action" => json!({ "message": report(&launch_dir(input["context"]["cwd"].as_str())) }),
        "projects" => json!([]),
        _ => return None,
    })
}

#[expect(clippy::print_stdout, reason = "stdout is the plugin's answer")]
fn main() -> std::process::ExitCode {
    let verb = std::env::args().next_back().unwrap_or_default();
    let mut raw = String::new();
    let input = std::io::stdin()
        .read_to_string(&mut raw)
        .ok()
        .and_then(|_| serde_json::from_str(&raw).ok())
        .unwrap_or(Value::Null);
    match answer(&verb, &input) {
        Some(v) => {
            println!("{v}");
            std::process::ExitCode::SUCCESS
        }
        None => std::process::ExitCode::from(2),
    }
}
