//! Config `plugins`: the only processes the plugin protocol ever starts, each by an absolute path
//! (never a PATH lookup, never a folder scan). Exports `PluginSpec` and `parse_plugins`.

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
        path: ctx.expand(raw_path),
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

    fn ctx() -> PathCtx {
        PathCtx {
            home: PathBuf::from(r"C:\Users\x"),
            cwd: PathBuf::from(r"C:\work"),
        }
    }

    #[test]
    fn a_full_entry_parses_and_tilde_expands() {
        let (p, w) = parse_plugins(
            &json!([{"name":"jira","path":"~/bin/jira.exe","args":["--x"],"timeoutMs":500}]),
            &ctx(),
        );
        assert_eq!(w, Vec::<String>::new());
        assert_eq!(
            p,
            [PluginSpec {
                name: "jira".into(),
                path: r"C:\Users\x\bin\jira.exe".into(),
                args: vec!["--x".into()],
                timeout_ms: Some(500),
            }]
        );
    }

    #[test]
    fn a_bare_name_or_relative_path_is_refused() {
        for path in ["jira.exe", r"bin\jira.exe"] {
            let (p, w) = parse_plugins(&json!([{"name":"jira","path":path}]), &ctx());
            assert_eq!(p, Vec::new(), "{path} would be a PATH or cwd lookup");
            assert_eq!(w.len(), 1);
        }
    }

    #[test]
    fn bad_names_duplicates_args_and_timeouts_are_skipped_with_a_warning() {
        let (p, w) = parse_plugins(
            &json!([
                {"name":"a b","path":"C:\\p.exe"},
                {"name":"ok","path":"C:\\p.exe"},
                {"name":"OK","path":"C:\\q.exe"},
                {"name":"n1","path":"C:\\p.exe","args":[1]},
                {"name":"n2","path":"C:\\p.exe","timeoutMs":0},
                "x"
            ]),
            &ctx(),
        );
        assert_eq!(p.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["ok"]);
        assert_eq!(w.len(), 5, "{w:?}");
    }
}
