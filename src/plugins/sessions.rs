//! `sessions {project}` -> `[{id, title?, updated?}]`: a plugin as a session source. Its entries
//! join the picker tagged with the plugin's name, the way another account's sessions are. Exports
//! `VERB` and `collect`.

use serde_json::{Value, json};

use super::run::Runner;
use super::{Kind, Plugins, Verb};
use crate::sessions::listing::Session;

pub const VERB: Verb = Verb {
    name: "sessions",
    timeout_ms: 2_000,
    kind: Kind::Source,
};

const MAX_ENTRIES: usize = 200;
const MAX_TITLE: usize = 200;

/// A resume id becomes an argument of `claude`: one leading `-` and it would be read as an option.
pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.starts_with('-') && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn session(plugin: &str, project: &str, v: &Value) -> Option<Session> {
    let id = v.get("id").and_then(Value::as_str).filter(|id| valid_session_id(id))?;
    let title: String = v
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(id)
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE)
        .collect();
    Some(Session {
        session_id: id.to_owned(),
        project: project.to_owned(),
        modified_ms: v.get("updated").and_then(Value::as_i64).unwrap_or(0),
        title,
        account: plugin.to_owned(),
        ..Session::default()
    })
}

/// Every source's sessions for `project` (empty = every project); bad entries are dropped.
pub fn collect(plugins: &Plugins, project: &str, runner: &dyn Runner, warnings: &mut Vec<String>) -> Vec<Session> {
    let mut out = Vec::new();
    for (i, answer) in plugins.call_each(&VERB, &|_| json!({ "project": project }), runner) {
        let name = plugins.list.get(i).map(|l| l.spec.name.as_str()).unwrap_or_default();
        match answer {
            Ok(Value::Array(items)) => out.extend(items.iter().take(MAX_ENTRIES).filter_map(|v| session(name, project, v))),
            Ok(_) => warnings.push(format!("plugin {name} {}: not a list; skipped", VERB.name)),
            Err(w) => warnings.push(w),
        }
    }
    out
}
