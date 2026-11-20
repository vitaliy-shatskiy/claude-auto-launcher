//! Config `plugins`: the only processes the plugin protocol ever starts, each by an absolute path
//! (never a PATH lookup, never a folder scan). Exports `PluginSpec`, `parse_plugins` and
//! `path_problem`.

use std::path::Path;

use serde_json::Value;

use super::{PathCtx, is_rooted_raw};

/// One listed plugin: started as `<path> <args...> <verb>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpec {
    /// Tags its rows, entries and warnings; letters, digits, `-` and `_`.
    pub name: String,
    /// A full path to an executable.
    pub path: String,
    pub args: Vec<String>,
    /// Overrides every verb's default timeout.
    pub timeout_ms: Option<u64>,
}

const MAX_NAME: usize = 32;
const MAX_TIMEOUT_MS: u64 = 120_000;

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= MAX_NAME && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Why a listed path would not run exactly as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathProblem {
    /// Not an existing file: nothing else is ever tried in its place.
    Missing,
    /// `<path>.exe` exists: Windows process start (Rust's `Command`) tries that name first for a
    /// path not ending in `.exe`, so it would run instead of the listed file.
    ExeTwin,
}

impl std::fmt::Display for PathProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "is not an existing file"),
            Self::ExeTwin => write!(f, "has a twin with .exe added beside it, which would start instead"),
        }
    }
}

/// Checked at config load and again before each start (the files may change in between).
pub fn path_problem(path: &str) -> Option<PathProblem> {
    let p = Path::new(path);
    if !p.is_file() {
        return Some(PathProblem::Missing);
    }
    let ends_exe = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    (!ends_exe && Path::new(&format!("{path}.exe")).exists()).then_some(PathProblem::ExeTwin)
}

/// One entry, or the warning that skips it.
fn parse_one(item: &Value, ctx: &PathCtx, taken: &[PluginSpec]) -> Result<PluginSpec, String> {
    let o = item.as_object().ok_or("plugins: an entry is not an object; skipped")?;
    let name = o.get("name").and_then(Value::as_str).unwrap_or_default().trim().to_owned();
    if !valid_name(&name) {
        return Err(format!(
            "plugins: name '{name}' must be 1-{MAX_NAME} letters, digits, '-' or '_'; skipped"
        ));
    }
    if taken.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
        return Err(format!(
            "plugins: duplicate name '{name}'; the first one wins, this entry is skipped"
        ));
    }
    let raw_path = o.get("path").and_then(Value::as_str).unwrap_or_default().trim();
    if !is_rooted_raw(raw_path) {
        return Err(format!("plugins '{name}': path must be an absolute path; skipped"));
    }
    let path = ctx.expand(raw_path);
    if let Some(why) = path_problem(&path) {
        return Err(format!("plugins '{name}': path {why}; skipped"));
    }
    let args = match o.get("args") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) if a.iter().all(Value::is_string) => a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        Some(_) => return Err(format!("plugins '{name}': args must be a list of strings; skipped")),
    };
    let timeout_ms = match o.get("timeoutMs") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .filter(|t| (1..=MAX_TIMEOUT_MS).contains(t))
                .ok_or_else(|| format!("plugins '{name}': timeoutMs must be 1-{MAX_TIMEOUT_MS}; skipped"))?,
        ),
    };
    Ok(PluginSpec {
        name,
        path,
        args,
        timeout_ms,
    })
}

/// `plugins`: a list of `{name, path, args?, timeoutMs?}`; a bad entry is skipped with a warning.
pub fn parse_plugins(raw: &Value, ctx: &PathCtx) -> (Vec<PluginSpec>, Vec<String>) {
    let Value::Array(items) = raw else {
        return (Vec::new(), vec!["plugins: not a list; no plugin runs".into()]);
    };
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for item in items {
        match parse_one(item, ctx, &out) {
            Ok(p) => out.push(p),
            Err(w) => warnings.push(w),
        }
    }
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    /// A home of its own holding `bin\<file>` for each of `files`.
    fn ctx(name: &str, files: &[&str]) -> PathCtx {
        let home = std::env::temp_dir().join(format!("cars-cfg-plugins-{name}-{}", std::process::id()));
        if home.exists() {
            std::fs::remove_dir_all(&home).unwrap();
        }
        std::fs::create_dir_all(home.join("bin")).unwrap();
        for f in files {
            std::fs::write(home.join("bin").join(f), "x").unwrap();
        }
        PathCtx {
            home,
            cwd: PathBuf::from(r"C:\work"),
        }
    }

    #[test]
    fn a_full_entry_parses_and_tilde_expands() {
        let ctx = ctx("full", &["jira.exe"]);
        let (p, w) = parse_plugins(
            &json!([{"name":"jira","path":"~/bin/jira.exe","args":["--x"],"timeoutMs":500}]),
            &ctx,
        );
        assert_eq!(w, Vec::<String>::new());
        assert_eq!(
            p,
            [PluginSpec {
                name: "jira".into(),
                path: ctx.home.join("bin").join("jira.exe").to_string_lossy().into_owned(),
                args: vec!["--x".into()],
                timeout_ms: Some(500),
            }]
        );
        std::fs::remove_dir_all(&ctx.home).unwrap();
    }

    #[test]
    fn a_bare_name_or_relative_path_is_refused() {
        let ctx = ctx("relative", &["jira.exe"]);
        for path in ["jira.exe", r"bin\jira.exe"] {
            let (p, w) = parse_plugins(&json!([{"name":"jira","path":path}]), &ctx);
            assert_eq!(p, Vec::new(), "{path} would be a PATH or cwd lookup");
            assert_eq!(w.len(), 1);
        }
        std::fs::remove_dir_all(&ctx.home).unwrap();
    }

    #[test]
    fn bad_names_duplicates_args_and_timeouts_are_skipped_with_a_warning() {
        let ctx = ctx("bad", &["p.exe", "q.exe"]);
        let (p, w) = parse_plugins(
            &json!([
                {"name":"a b","path":"~/bin/p.exe"},
                {"name":"ok","path":"~/bin/p.exe"},
                {"name":"OK","path":"~/bin/q.exe"},
                {"name":"n1","path":"~/bin/p.exe","args":[1]},
                {"name":"n2","path":"~/bin/p.exe","timeoutMs":0},
                "x"
            ]),
            &ctx,
        );
        assert_eq!(p.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["ok"]);
        assert_eq!(w.len(), 5, "{w:?}");
        std::fs::remove_dir_all(&ctx.home).unwrap();
    }

    #[test]
    fn a_path_that_is_not_a_file_exactly_as_written_is_refused() {
        let ctx = ctx("missing", &["tool.exe"]);
        for path in ["~/bin/tool", "~/bin/none.exe", "~/bin"] {
            let (p, w) = parse_plugins(&json!([{"name":"tool","path":path}]), &ctx);
            assert_eq!(p, Vec::new(), "{path}: only the file as written may run");
            assert!(w.first().is_some_and(|w| w.contains("not an existing file")), "{path}: {w:?}");
        }
        std::fs::remove_dir_all(&ctx.home).unwrap();
    }

    #[test]
    fn a_path_with_the_same_name_plus_exe_beside_it_is_refused() {
        let ctx = ctx("shadow", &["tick.cmd", "tick.cmd.exe", "tool", "tool.exe", "alone.cmd"]);
        for path in ["~/bin/tick.cmd", "~/bin/TOOL"] {
            let (p, w) = parse_plugins(&json!([{"name":"tick","path":path}]), &ctx);
            assert_eq!(p, Vec::new(), "{path}: its .exe twin would start instead");
            assert!(w.first().is_some_and(|w| w.contains(".exe")), "{path}: {w:?}");
        }
        let (p, w) = parse_plugins(&json!([{"name":"alone","path":"~/bin/alone.cmd"}]), &ctx);
        assert_eq!((p.len(), w.len()), (1, 0), "a file with no twin runs: {w:?}");
        std::fs::remove_dir_all(&ctx.home).unwrap();
    }
}
