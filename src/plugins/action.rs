//! `action {key, context}` -> `{message?}`: a plugin's key pressed on its screen. Exports `VERB`,
//! `find` and `run`.

use serde_json::{Value, json};

use super::run::Runner;
use super::{Kind, Plugins, Verb, warning};
use crate::input::{KeyPress, is_hotkey};

pub const VERB: Verb = Verb {
    name: "action",
    timeout_ms: 10_000,
    kind: Kind::Action,
};

const MAX_MESSAGE: usize = 200;

/// The plugin (index) and key a press on `screen` belongs to. A modifier combo is never a plugin key.
pub fn find(plugins: &Plugins, screen: &str, key: &KeyPress) -> Option<(usize, char)> {
    if key.ctrl || key.alt {
        return None;
    }
    plugins.list.iter().enumerate().find_map(|(i, l)| {
        l.desc
            .actions
            .iter()
            .find(|a| a.screen == screen && is_hotkey(key, a.key))
            .map(|a| (i, a.key))
    })
}

/// Runs the action; the line to show: the plugin's message, or the warning.
pub fn run(plugins: &Plugins, plugin: usize, key: char, context: &Value, runner: &dyn Runner) -> Option<String> {
    let l = plugins.list.get(plugin)?;
    let input = json!({ "key": key.to_string(), "context": context });
    match runner.call(&l.spec, &VERB, &input) {
        Ok(v) => v.get("message").and_then(Value::as_str).map(|m| {
            format!(
                "{}: {}",
                l.spec.name,
                m.chars().filter(|c| !c.is_control()).take(MAX_MESSAGE).collect::<String>()
            )
        }),
        Err(e) => Some(warning(&l.spec, &VERB, &e)),
    }
}
