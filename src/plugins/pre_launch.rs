//! `preLaunch {choice}` -> `{env?, args?, cancel?}`: what claude starts with on top of the screen's
//! choices, or the reason it does not start. Output is checked before any of it is used: one bad
//! name or value refuses that plugin's whole answer. Exports `VERB`, `Extra`, `Outcome`, `run`.

use serde_json::Value;

use super::run::Runner;
use super::{Kind, Plugins, Verb};

pub const VERB: Verb = Verb {
    name: "preLaunch",
    timeout_ms: 10_000,
    kind: Kind::Hook,
};

const MAX_VARS: usize = 64;
const MAX_NAME: usize = 128;
const MAX_VALUE: usize = 8 * 1024;
const MAX_ARGS: usize = 32;
const MAX_ARG: usize = 4 * 1024;
const MAX_REASON: usize = 200;

/// What every plugin added, in config order (a later plugin's variable wins).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extra {
    pub env: Vec<(String, String)>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Go(Extra),
    Cancel { plugin: String, reason: String },
}

enum Answer {
    Go(Extra),
    Cancel(String),
}

/// Variables a plugin may never set: they pick the account or its credentials, which the launch
/// screen owns - the config dir, an API key or OAuth token, another provider (Bedrock, Vertex,
/// Foundry), or the home directory `~/.claude` is found from. An entry ending in `*` is a prefix
/// (case-insensitive, as Windows names are).
const REFUSED_ENV: [&str; 9] = [
    "CLAUDE_CONFIG_DIR",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_USE_*",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_*",
    "USERPROFILE",
    "HOME",
];

fn refused_env(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    REFUSED_ENV
        .iter()
        .any(|r| r.strip_suffix('*').map_or(up == *r, |prefix| up.starts_with(prefix)))
}

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= MAX_NAME && !n.contains('=') && !n.chars().any(char::is_control)
}

fn valid_text(v: &str, max: usize) -> bool {
    v.len() <= max && !v.contains('\0')
}

fn check(v: &Value) -> Result<Answer, String> {
    let o = v.as_object().ok_or("not an object")?;
    match o.get("cancel") {
        None | Some(Value::Null | Value::Bool(false)) => {}
        Some(Value::String(r)) if !r.trim().is_empty() => {
            let r: String = r.chars().filter(|c| !c.is_control()).take(MAX_REASON).collect();
            return Ok(Answer::Cancel(r));
        }
        Some(Value::Bool(true)) => return Ok(Answer::Cancel(String::new())),
        Some(_) => return Err("cancel must be a reason or true".into()),
    }
    let mut extra = Extra::default();
    if let Some(env) = o.get("env").filter(|e| !e.is_null()) {
        let env = env.as_object().ok_or("env must be an object")?;
        if env.len() > MAX_VARS {
            return Err(format!("env over {MAX_VARS} variables"));
        }
        for (k, v) in env {
            let v = v.as_str().ok_or_else(|| format!("env {k}: not a string"))?;
            if !valid_name(k) || !valid_text(v, MAX_VALUE) {
                return Err(format!("env {k}: bad name or value"));
            }
            if refused_env(k) {
                return Err(format!("env {k}: the account and its credentials are the launcher's"));
            }
            extra.env.push((k.clone(), v.to_owned()));
        }
    }
    if let Some(args) = o.get("args").filter(|a| !a.is_null()) {
        let args = args.as_array().ok_or("args must be a list")?;
        if args.len() > MAX_ARGS {
            return Err(format!("args over {MAX_ARGS}"));
        }
        for a in args {
            let a = a.as_str().filter(|a| valid_text(a, MAX_ARG)).ok_or("args: a bad argument")?;
            extra.args.push(a.to_owned());
        }
    }
    Ok(Answer::Go(extra))
}

/// Every plugin with the hook; `choice(i)` is plugin `i`'s request. The first `cancel` in config
/// order stops the launch; a failed or refused answer is a warning and adds nothing.
pub fn run(plugins: &Plugins, choice: &(dyn Fn(usize) -> Value + Sync), runner: &dyn Runner, warnings: &mut Vec<String>) -> Outcome {
    let mut all = Extra::default();
    for (i, answer) in plugins.call_each(&VERB, &|i| serde_json::json!({ "choice": choice(i) }), runner) {
        let name = plugins.list.get(i).map(|l| l.spec.name.clone()).unwrap_or_default();
        match answer.and_then(|v| check(&v).map_err(|e| format!("plugin {name} {}: {e}; skipped", VERB.name))) {
            Ok(Answer::Cancel(reason)) => return Outcome::Cancel { plugin: name, reason },
            Ok(Answer::Go(extra)) => {
                all.env.extend(extra.env);
                all.args.extend(extra.args);
            }
            Err(w) => warnings.push(w),
        }
    }
    Outcome::Go(all)
}
