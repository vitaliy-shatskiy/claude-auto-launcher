//! `projects {}` -> `[{path, name?, lastUsed?}]`: a plugin as a project source. Its entries merge
//! into the project list, named with the plugin's tag; a directory the registry already knows keeps
//! its own row. Exports `VERB`, `Entry`, `collect` and `merge`.

use serde_json::{Value, json};

use super::run::Runner;
use super::{Kind, Plugins, Verb};
use crate::config::is_rooted;
use crate::projects::{Project, project_key};

pub const VERB: Verb = Verb {
    name: "projects",
    timeout_ms: 2_000,
    kind: Kind::Source,
};

const MAX_ENTRIES: usize = 200;
const MAX_TEXT: usize = 260;

/// One project a plugin offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub plugin: String,
    pub path: String,
    pub name: String,
    pub last_used_ms: Option<i64>,
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect()
}

fn entry(plugin: &str, v: &Value) -> Option<Entry> {
    let path = v
        .get("path")
        .and_then(Value::as_str)
        .filter(|p| is_rooted(p) && p.len() <= MAX_TEXT && !p.contains('\0'))?;
    let leaf = std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
    let name = v.get("name").and_then(Value::as_str).map_or(leaf, clean);
    Some(Entry {
        plugin: plugin.to_owned(),
        path: clean(path),
        name,
        last_used_ms: v.get("lastUsed").and_then(Value::as_i64),
    })
}

/// Every source's entries; a bad entry is dropped, a failed call is a warning.
pub fn collect(plugins: &Plugins, runner: &dyn Runner, warnings: &mut Vec<String>) -> Vec<Entry> {
    let mut out = Vec::new();
    for (i, answer) in plugins.call_each(&VERB, &|_| json!({}), runner) {
        let name = plugins.list.get(i).map(|l| l.spec.name.as_str()).unwrap_or_default();
        match answer {
            Ok(Value::Array(items)) => {
                let before = out.len();
                out.extend(items.iter().take(MAX_ENTRIES).filter_map(|v| entry(name, v)));
                if out.len() - before < items.len().min(MAX_ENTRIES) {
                    warnings.push(format!("plugin {name} {}: entries without an absolute path; skipped", VERB.name));
                }
            }
            Ok(_) => warnings.push(format!("plugin {name} {}: not a list; skipped", VERB.name)),
            Err(w) => warnings.push(w),
        }
    }
    out
}

/// The folder name Claude Code stores a directory's transcripts under: every character but an
/// ASCII letter or digit becomes `-`.
pub fn claude_slug(path: &str) -> String {
    path.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// The registry's list with the plugin entries in, newest first; the tag shows whose row it is.
/// A directory with sessions is already a registry row (that row wins), so a plugin-only row gets
/// the one slug Claude would use for it: resume there lists that directory's sessions or none,
/// never the whole account (empty slugs would mean "every project").
pub fn merge(list: &[Project], extra: &[Entry]) -> Vec<Project> {
    let mut out = list.to_vec();
    for e in extra {
        let key = project_key(&e.path);
        if out.iter().any(|p| project_key(&p.path) == key) {
            continue;
        }
        let slug = claude_slug(&e.path);
        out.push(Project {
            name: format!("{} [{}]", e.name, e.plugin),
            path: e.path.clone(),
            slug: slug.clone(),
            slugs: vec![slug],
            worktree: None,
            last_activity_ms: e.last_used_ms,
            hidden: false,
        });
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.last_activity_ms));
    out
}
