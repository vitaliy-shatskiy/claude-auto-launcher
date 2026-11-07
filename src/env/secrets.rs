//! Session secrets from `vsec launch-env --cwd <dir>`: vsec resolves the tiers (shared -> org ->
//! project) and writes `{vars:[{name,value}], loaded:["NAME(tier)"], notes:[...]}` to stdout.
//! Values go to the CHILD environment only and are never printed. vsec not found: the file walk
//! under `secretsRoot` (`files`). vsec found but failing: one line, no secrets, never the files
//! (two sources are never mixed silently).

mod files;

use std::ffi::OsString;
use std::io;
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde::Deserialize;

use super::{CREATE_NO_WINDOW, Note};
use crate::out::Fg;

/// Names the shell owns: a secret called PATH is a mistake, never an instruction. vsec refuses
/// them too; the launcher checks again because these values land in its child's environment.
const RESERVED: [&str; 12] = [
    "PATH",
    "PATHEXT",
    "COMSPEC",
    "SYSTEMROOT",
    "WINDIR",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "OS",
    "PSMODULEPATH",
];

/// Claude Code auth names: one in the store would move every launched session to API billing or
/// another account.
const CLAUDE_AUTH: [&str; 4] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
];

/// Why a valid variable name is never exported, as the note's tail; matched case-insensitively,
/// as Windows matches environment names.
fn refusal(name: &str) -> Option<&'static str> {
    let up = name.to_ascii_uppercase();
    if RESERVED.contains(&up.as_str()) {
        Some("that name is reserved by the system")
    } else if CLAUDE_AUTH.contains(&up.as_str()) {
        Some("Claude Code auth stays with the profile login")
    } else {
        None
    }
}

/// What one import decided: the variables to set (in order), the `NAME(tier)` list, the lines.
#[derive(Debug, Default)]
pub(crate) struct Import {
    pub(crate) vars: Vec<(String, String)>,
    pub(crate) loaded: Vec<String>,
    pub(crate) notes: Vec<Note>,
}

#[derive(Deserialize)]
struct LaunchEnv {
    vars: Vec<Var>,
    #[serde(default)]
    loaded: Vec<String>,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Deserialize)]
struct Var {
    name: String,
    value: String,
}

fn is_env_name(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// Where vsec is: config `vsecPath`, else `CLAUDE_AUTO_VSEC`, else `vsec.exe` or the `vsec.cmd`
/// shim on PATH. An explicit path is returned even when missing: the user asked for vsec, so
/// `run_vsec` reports it instead of falling back to files.
pub(crate) fn find_vsec(configured: &str, env: Option<OsString>, path: Option<OsString>) -> Option<PathBuf> {
    if !configured.is_empty() {
        return Some(PathBuf::from(configured));
    }
    if let Some(e) = env.filter(|e| !e.is_empty()) {
        return Some(PathBuf::from(e));
    }
    let dirs: Vec<PathBuf> = std::env::split_paths(&path?).collect();
    ["vsec.exe", "vsec.cmd"]
        .iter()
        .flat_map(|n| dirs.iter().map(move |d| d.join(n)))
        .find(|p| p.is_file())
}

/// The session's secrets: vsec when found, else the file walk under `secrets_root`.
pub(crate) fn load(cwd: &str, vsec: Option<&Path>, secrets_root: &str) -> Import {
    let Some(vsec) = vsec else {
        let mut got = files::import(cwd, secrets_root);
        // Silent when the walk found nothing: a user with neither vsec nor a store sees no line.
        if !got.notes.is_empty() {
            got.notes
                .insert(0, Note::new("  secrets from files (vsec not found)".to_owned(), Fg::DarkGray));
        }
        return got;
    };
    import(cwd, |dir| run_vsec(vsec, dir))
}

/// `vsec launch-env --cwd <dir>`, windowless, stdin closed.
pub(crate) fn run_vsec(vsec: &Path, cwd: &str) -> io::Result<Output> {
    if !vsec.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("vsec not found at {}", vsec.display()),
        ));
    }
    Command::new(vsec)
        .args(["launch-env", "--cwd", cwd])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
}

fn one_line(s: &str) -> String {
    s.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" | ")
}

/// The decision, with the vsec call injected so the suite runs on a fake `launch-env`.
pub(crate) fn import(working_directory: &str, launch_env: impl FnOnce(&str) -> io::Result<Output>) -> Import {
    let mut out = Import::default();
    let warn = |t: String| Note::new(format!("  secrets not loaded: {t}"), Fg::DarkYellow);
    let got = match launch_env(working_directory) {
        Ok(o) => o,
        Err(e) => {
            out.notes.push(warn(one_line(&e.to_string())));
            return out;
        }
    };
    let stderr = String::from_utf8_lossy(&got.stderr);
    if !got.status.success() {
        let code = got.status.code().map_or_else(|| "?".to_owned(), |c| c.to_string());
        out.notes.push(warn(format!("vsec launch-env exit {code}: {}", one_line(&stderr))));
        return out;
    }
    let parsed: LaunchEnv = match serde_json::from_slice(&got.stdout) {
        Ok(p) => p,
        Err(e) => {
            // The serde message names a position, never the text, so no value can leak here.
            out.notes.push(warn(format!("vsec launch-env output unreadable ({e})")));
            return out;
        }
    };
    for v in parsed.vars {
        if !is_env_name(&v.name) {
            out.notes.push(Note::new(
                format!("  secret '{}' skipped: not a valid environment variable name", v.name),
                Fg::DarkYellow,
            ));
        } else if let Some(why) = refusal(&v.name) {
            out.notes
                .push(Note::new(format!("  secret '{}' skipped: {why}", v.name), Fg::DarkYellow));
        } else {
            out.vars.push((v.name, v.value));
        }
    }
    out.loaded = parsed.loaded;
    if !out.loaded.is_empty() {
        out.notes
            .push(Note::new(format!("  secrets loaded: {}", out.loaded.join(", ")), Fg::DarkGray));
    }
    let lines = parsed.notes.iter().map(String::as_str).chain(stderr.lines());
    out.notes.extend(
        lines
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| Note::new(format!("  {l}"), Fg::DarkGray)),
    );
    out
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::os::windows::process::ExitStatusExt as _;
    use std::process::ExitStatus;

    use super::*;

    #[expect(clippy::unnecessary_wraps, reason = "the fake returns what the import seam takes")]
    fn output(code: u32, stdout: &str, stderr: &str) -> io::Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(code),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        })
    }

    /// A fake `launch-env`: the JSON vsec writes for the given vars and loaded list.
    fn launch_env(vars: &[(&str, &str)], loaded: &[&str], notes: &[&str]) -> String {
        serde_json::json!({
            "vars": vars.iter().map(|(n, v)| serde_json::json!({"name": n, "value": v})).collect::<Vec<_>>(),
            "loaded": loaded,
            "notes": notes,
        })
        .to_string()
    }

    fn var<'a>(i: &'a Import, n: &str) -> Option<&'a str> {
        i.vars.iter().rev().find(|(k, _)| k == n).map(|(_, v)| v.as_str())
    }

    fn texts(i: &Import) -> Vec<&str> {
        i.notes.iter().map(|n| n.text.as_str()).collect()
    }

    /// What vsec resolves for the tiered fixture the file walk used: project beats org beats shared.
    fn tiered() -> String {
        launch_env(
            &[
                ("SHARED_ONLY", "from-shared"),
                ("OVERRIDDEN", "from-proj"),
                ("ORG_BEATS_THIS", "from-org"),
                ("ORG_ONLY", "from-org"),
                ("PROJ_ONLY", "from-proj"),
                ("PAIR", "pair-value"),
                ("PAIR_FILE", "a-real-secret-not-a-path"),
            ],
            &[
                "SHARED_ONLY(shared)",
                "OVERRIDDEN(project)",
                "ORG_BEATS_THIS(org)",
                "ORG_ONLY(org)",
                "PROJ_ONLY(project)",
                "PAIR(project)",
                "PAIR_FILE(project)",
            ],
            &["read by path, not exported: by-path-key.json"],
        )
    }

    #[test]
    fn launch_env_output_sets_tier_vars_without_name_file() {
        let seen = RefCell::new(String::new());
        let i = import(r"C:\test\.proj", |cwd| {
            seen.replace(cwd.to_owned());
            output(0, &tiered(), "")
        });
        assert_eq!(seen.into_inner(), r"C:\test\.proj", "vsec gets the session's directory");
        let names: Vec<&str> = i.vars.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "SHARED_ONLY",
                "OVERRIDDEN",
                "ORG_BEATS_THIS",
                "ORG_ONLY",
                "PROJ_ONLY",
                "PAIR",
                "PAIR_FILE"
            ],
            "exactly vsec's vars, in order, no derived <NAME>_FILE"
        );
        assert_eq!(var(&i, "OVERRIDDEN"), Some("from-proj"));
        assert_eq!(
            var(&i, "PAIR_FILE"),
            Some("a-real-secret-not-a-path"),
            "a real _FILE secret passes as is"
        );
        let joined = i.loaded.join(",");
        assert!(joined.contains("(shared)") && joined.contains("(org)") && joined.contains("(project)"));
        let text = texts(&i);
        assert!(text.iter().any(|t| t.starts_with("  secrets loaded: SHARED_ONLY(shared)")));
        assert!(text.iter().any(|t| t.contains("read by path") && t.contains("by-path-key.json")));
        assert!(!text.iter().any(|t| t.contains("from-")), "a value is never printed");
    }

    #[test]
    fn an_empty_launch_env_sets_and_prints_nothing() {
        let i = import(r"C:\test\unknown", |_| output(0, &launch_env(&[], &[], &[]), ""));
        assert!(i.loaded.is_empty() && i.vars.is_empty() && i.notes.is_empty());
    }

    #[test]
    fn a_name_that_is_no_variable_name_costs_one_line() {
        let json = launch_env(
            &[
                ("PATH", "x"),
                ("A=B", "x"),
                ("1DIGIT", "x"),
                ("HAS SPACE", "x"),
                ("GOOD_NAME", "good"),
            ],
            &["GOOD_NAME(shared)"],
            &[],
        );
        let i = import(r"C:\test\.names", |_| output(0, &json, ""));
        let text = texts(&i);
        assert_eq!(var(&i, "PATH"), None, "a secret named PATH does not replace PATH");
        assert_eq!(text.iter().filter(|t| t.starts_with("  secret '")).count(), 4);
        assert!(text.iter().any(|t| t.contains("secret 'PATH'") && t.contains("reserved")));
        assert_eq!(var(&i, "GOOD_NAME"), Some("good"));
        assert!(!text.iter().any(|t| t.contains("good")), "a value is never printed");
    }

    #[test]
    fn a_claude_auth_name_from_vsec_is_skipped_with_a_line_naming_only_it() {
        let names = [
            "ANTHROPIC_API_KEY",
            "anthropic_auth_token",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
        ];
        let mut vars: Vec<(&str, &str)> = names.iter().map(|n| (*n, "sk-ant-leak")).collect();
        vars.push(("GOOD_NAME", "good"));
        let i = import(r"C:\p", |_| output(0, &launch_env(&vars, &[], &[]), ""));
        assert_eq!(i.vars, [("GOOD_NAME".to_owned(), "good".to_owned())]);
        let text = texts(&i);
        for n in names {
            assert_eq!(
                text.iter().filter(|t| t.contains(&format!("secret '{n}' skipped"))).count(),
                1,
                "{n}"
            );
        }
        assert!(!text.iter().any(|t| t.contains("sk-ant-leak")), "a value is never printed");
    }

    #[test]
    fn a_bracket_directory_reaches_vsec_verbatim() {
        let seen = RefCell::new(String::new());
        import(r"C:\test\br[x]dir", |cwd| {
            seen.replace(cwd.to_owned());
            output(0, &launch_env(&[], &[], &[]), "")
        });
        assert_eq!(seen.into_inner(), r"C:\test\br[x]dir");
    }

    #[test]
    fn an_explicit_vsec_path_that_is_missing_costs_a_line_and_never_falls_back() {
        let store = std::env::temp_dir().join(format!("claude-auto-rs-vsec-nofb-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(store.join("shared")).unwrap();
        std::fs::write(store.join(r"shared\FROM_FILE"), "file-value").unwrap();
        let i = load(r"C:\p", Some(Path::new(r"C:\does-not-exist\vsec.cmd")), &store.to_string_lossy());
        std::fs::remove_dir_all(&store).unwrap();
        assert!(i.vars.is_empty() && i.loaded.is_empty(), "the file store is not read");
        assert_eq!(texts(&i), [r"  secrets not loaded: vsec not found at C:\does-not-exist\vsec.cmd"]);
    }

    #[test]
    fn vsec_not_found_falls_back_to_the_file_walk_with_one_gray_line() {
        let store = std::env::temp_dir().join(format!("claude-auto-rs-vsec-fb-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(store.join("shared")).unwrap();
        std::fs::write(store.join(r"shared\FROM_FILE"), "file-value").unwrap();
        let i = load(r"C:\p", None, &store.to_string_lossy());
        let empty = load(r"C:\p", None, &store.join("none").to_string_lossy());
        std::fs::remove_dir_all(&store).unwrap();
        assert_eq!(var(&i, "FROM_FILE"), Some("file-value"));
        assert_eq!(texts(&i).first(), Some(&"  secrets from files (vsec not found)"));
        assert_eq!(i.notes.first().map(|n| n.fg), Some(Fg::DarkGray));
        assert!(empty.notes.is_empty() && empty.vars.is_empty(), "no vsec and no store: no line");
    }

    #[test]
    fn a_vsec_shim_is_spawned_with_launch_env_and_the_directory() {
        let root = std::env::temp_dir().join(format!("claude-auto-rs-vsec-run-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&root).unwrap();
        let shim = root.join("vsec.cmd");
        // A forward-slash directory: the echo builds JSON by hand, where a backslash would escape.
        std::fs::write(&shim, "@echo {\"vars\":[{\"name\":\"ARGS\",\"value\":\"%*\"}]}\r\n").unwrap();
        let i = load("C:/test/proj", Some(&shim), r"C:\no-store");
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(i.vars, [("ARGS".to_owned(), "launch-env --cwd C:/test/proj".to_owned())]);
    }

    #[test]
    fn vsec_is_found_by_override_then_exe_then_shim_on_path() {
        let root = std::env::temp_dir().join(format!("claude-auto-rs-vsec-{}", crate::log::new_run_id()));
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("vsec.cmd"), "").unwrap();
        let path = std::env::join_paths([&a, &b]).unwrap();
        assert_eq!(
            find_vsec("", None, Some(path.clone())),
            Some(a.join("vsec.cmd")),
            "the shim on PATH"
        );
        std::fs::write(b.join("vsec.exe"), "").unwrap();
        assert_eq!(
            find_vsec("", None, Some(path.clone())),
            Some(b.join("vsec.exe")),
            "an exe beats a shim"
        );
        assert_eq!(
            find_vsec("", Some(r"C:\x\vsec.exe".into()), Some(path.clone())),
            Some(PathBuf::from(r"C:\x\vsec.exe")),
            "CLAUDE_AUTO_VSEC beats PATH"
        );
        assert_eq!(
            find_vsec(r"C:\cfg\vsec.exe", Some(r"C:\x\vsec.exe".into()), Some(path.clone())),
            Some(PathBuf::from(r"C:\cfg\vsec.exe")),
            "config vsecPath beats CLAUDE_AUTO_VSEC"
        );
        assert_eq!(
            find_vsec("", Some("".into()), Some(path)),
            Some(b.join("vsec.exe")),
            "an empty override is unset"
        );
        assert_eq!(find_vsec("", None, Some(root.join("none").into_os_string())), None);
        assert_eq!(find_vsec("", None, None), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn vsec_exit_2_starts_the_session_without_secrets_and_names_the_reason() {
        let i = import(r"C:\p", |_| {
            output(2, &launch_env(&[("A", "leak")], &["A(shared)"], &[]), "vsec: no cache\r\n")
        });
        assert!(
            i.vars.is_empty() && i.loaded.is_empty(),
            "a failed run sets nothing, even with stdout"
        );
        assert_eq!(texts(&i), ["  secrets not loaded: vsec launch-env exit 2: vsec: no cache"]);
    }

    #[test]
    fn unreadable_output_sets_nothing_and_never_echoes_it() {
        let i = import(r"C:\p", |_| output(0, "not json SECRET_VALUE", ""));
        assert_eq!(i.vars, [] as [(String, String); 0]);
        let text = texts(&i);
        assert_eq!(text.len(), 1);
        assert!(text[0].contains("output unreadable") && !text[0].contains("SECRET_VALUE"));
    }

    #[test]
    fn vsec_stderr_lines_on_success_are_shown() {
        let i = import(r"C:\p", |_| {
            output(0, &launch_env(&[], &[], &[]), "vsec: shared\\X read from file (not in Bitwarden)\n")
        });
        assert_eq!(texts(&i), [r"  vsec: shared\X read from file (not in Bitwarden)"]);
    }
}
