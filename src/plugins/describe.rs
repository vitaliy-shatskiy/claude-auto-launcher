//! `describe`: what a plugin adds - launch-screen rows, action keys, the hooks it wants called and
//! the lists it sources. Untrusted output, checked here once: bad parts are skipped with a warning.
//! Exports `VERB`, `Description`, `Row`, `Action`, `parse`.

use serde_json::Value;

use super::{Kind, VERBS, Verb};
use crate::theme;

pub const VERB: Verb = Verb {
    name: "describe",
    timeout_ms: 2_000,
    kind: Kind::Describe,
};

/// The protocol this launcher speaks.
pub const PROTOCOL: u64 = 1;

const MAX_ROWS: usize = 8;
const MAX_VALUES: usize = 32;
const MAX_TEXT: usize = 64;
const MAX_ACTIONS: usize = 8;

/// The screens a plugin action may sit on, with the keys that screen keeps for itself.
pub const SCREENS: [(&str, &[char]); 1] = [("launch", &['u', 'w', 'a', 's', 'd'])];

/// One row on the launch screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub label: String,
    pub values: Vec<String>,
    pub default: String,
}

/// One key on one screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub key: char,
    pub label: String,
    pub screen: &'static str,
}

/// A checked `describe` answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Description {
    /// The protocol the plugin answered with: 1, or a newer one read for its v1 fields only.
    pub protocol: u64,
    pub rows: Vec<Row>,
    pub actions: Vec<Action>,
    /// Hook verbs it asked for (`preLaunch`, `postLaunch`).
    pub hooks: Vec<&'static str>,
    /// Source verbs it asked for (`projects`, `sessions`).
    pub sources: Vec<&'static str>,
}

impl Description {
    pub fn wants(&self, verb: &Verb) -> bool {
        self.hooks.contains(&verb.name) || self.sources.contains(&verb.name)
    }
}

/// Printable, bounded text: control characters dropped (they would move the cursor on screen).
fn text(v: Option<&Value>) -> Option<String> {
    let s = theme::strip_ctl(v?.as_str()?);
    let s = s.trim();
    (!s.is_empty() && s.chars().count() <= MAX_TEXT).then(|| s.to_owned())
}

fn ident(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_TEXT && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn row(v: &Value) -> Result<Row, String> {
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| ident(n))
        .ok_or("a row needs a name of letters, digits, '-', '_'")?;
    let values: Vec<String> = v
        .get("values")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|x| text(Some(x))).collect::<Option<Vec<_>>>())
        .unwrap_or_default()
        .filter(|vals| !vals.is_empty() && vals.len() <= MAX_VALUES)
        .ok_or_else(|| format!("row {name}: values must be 1-{MAX_VALUES} short strings"))?;
    let default = match v.get("default") {
        None | Some(Value::Null) => values.first().cloned().unwrap_or_default(),
        Some(d) => text(Some(d))
            .filter(|d| values.contains(d))
            .ok_or_else(|| format!("row {name}: default is not one of its values"))?,
    };
    Ok(Row {
        name: name.to_owned(),
        label: text(v.get("label")).unwrap_or_else(|| name.to_owned()),
        values,
        default,
    })
}

fn action(v: &Value) -> Result<Action, String> {
    let raw = v.get("key").and_then(Value::as_str).unwrap_or_default().to_lowercase();
    let mut chars = raw.chars();
    let (Some(key), None) = (chars.next(), chars.next()) else {
        return Err(format!("action '{raw}': key must be one letter or digit"));
    };
    if !key.is_ascii_alphanumeric() {
        return Err(format!("action '{raw}': key must be one letter or digit"));
    }
    let screen_name = v.get("screen").and_then(Value::as_str).unwrap_or("launch");
    let Some((screen, reserved)) = SCREENS.iter().find(|(s, _)| s.eq_ignore_ascii_case(screen_name)) else {
        return Err(format!("action '{key}': no screen '{screen_name}' (protocol 1: launch)"));
    };
    if reserved.contains(&key) {
        return Err(format!("action '{key}': the {screen} screen keeps that key for itself"));
    }
    Ok(Action {
        key,
        label: text(v.get("label")).unwrap_or_else(|| key.to_string()),
        screen,
    })
}

/// The verbs of `kind` a list names; an unknown name is a warning.
fn verbs(v: Option<&Value>, kind: Kind, field: &str, warnings: &mut Vec<String>) -> Vec<&'static str> {
    let mut out = Vec::new();
    for n in v.and_then(Value::as_array).into_iter().flatten() {
        let name = n.as_str().unwrap_or_default();
        match VERBS.iter().find(|x| x.kind == kind && x.name == name) {
            Some(x) if !out.contains(&x.name) => out.push(x.name),
            Some(_) => {}
            None => warnings.push(format!("{field}: unknown '{name}'")),
        }
    }
    out
}

/// A `describe` answer, checked. `Err` = the whole answer is unusable (no protocol or one below 1,
/// not an object); a bad row or action is dropped and named in the warnings. A newer protocol is
/// read for the fields protocol 1 knows: v1 only gains optional fields, and unknown ones are ignored.
pub fn parse(v: &Value) -> Result<(Description, Vec<String>), String> {
    if !v.is_object() {
        return Err("describe: not an object".into());
    }
    let raw = v.get("protocol").and_then(Value::as_u64);
    let Some(protocol) = raw.filter(|p| *p >= PROTOCOL) else {
        return Err(format!("describe: protocol {raw:?}, this launcher speaks {PROTOCOL}"));
    };
    let mut warnings = Vec::new();
    let mut d = Description {
        protocol,
        ..Description::default()
    };
    for r in v.get("rows").and_then(Value::as_array).into_iter().flatten().take(MAX_ROWS) {
        match row(r) {
            Ok(r) if d.rows.iter().any(|x| x.name.eq_ignore_ascii_case(&r.name)) => warnings.push(format!("row {}: duplicate", r.name)),
            Ok(r) => d.rows.push(r),
            Err(e) => warnings.push(e),
        }
    }
    for a in v.get("actions").and_then(Value::as_array).into_iter().flatten().take(MAX_ACTIONS) {
        match action(a) {
            Ok(a) => d.actions.push(a),
            Err(e) => warnings.push(e),
        }
    }
    d.hooks = verbs(v.get("hooks"), Kind::Hook, "hooks", &mut warnings);
    d.sources = verbs(v.get("sources"), Kind::Source, "sources", &mut warnings);
    Ok((d, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_full_answer_parses() {
        let (d, w) = parse(&json!({
            "protocol": 1,
            "rows": [{"name":"ticket","label":"ticket","values":["none","ABC-1"],"default":"none"}],
            "actions": [{"key":"J","label":"open jira","screen":"launch"}],
            "hooks": ["preLaunch","postLaunch"],
            "sources": ["projects","sessions"]
        }))
        .unwrap();
        assert_eq!(w, Vec::<String>::new());
        assert_eq!(d.rows.first().map(|r| r.default.as_str()), Some("none"));
        assert_eq!(d.actions.first().map(|a| (a.key, a.screen)), Some(('j', "launch")));
        assert_eq!(d.hooks, ["preLaunch", "postLaunch"]);
        assert_eq!(d.sources, ["projects", "sessions"]);
    }

    #[test]
    fn no_protocol_or_one_below_1_is_refused_whole() {
        for bad in [json!({"rows": []}), json!({"protocol": 0}), json!({"protocol": "1"}), json!([])] {
            assert!(parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_newer_protocol_parses_and_says_which() {
        let (d, w) = parse(&json!({"protocol": 3, "hooks": ["preLaunch"]})).unwrap();
        assert_eq!((d.protocol, d.hooks, w), (3, vec!["preLaunch"], Vec::<String>::new()));
    }

    #[test]
    fn a_built_in_key_is_refused_with_a_warning() {
        let (d, w) = parse(&json!({"protocol":1,"actions":[{"key":"u","label":"x"},{"key":"d"},{"key":"x","screen":"nowhere"}]})).unwrap();
        assert_eq!(d.actions, Vec::new());
        assert_eq!(w.len(), 3, "{w:?}");
        assert!(w.first().is_some_and(|l| l.contains("keeps that key")), "{w:?}");
    }

    #[test]
    fn a_default_outside_the_values_and_control_text_are_refused() {
        let (d, w) = parse(&json!({"protocol":1,"rows":[
            {"name":"a","values":["x"],"default":"y"},
            {"name":"b","values":["ok","\u{1b}[2J"]},
            {"name":"c d","values":["x"]},
            {"name":"e","values":["x","y"]}
        ]}))
        .unwrap();
        assert_eq!(d.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["b", "e"]);
        assert_eq!(
            d.rows.first().and_then(|r| r.values.get(1)).map(String::as_str),
            Some("[2J"),
            "escape dropped"
        );
        assert_eq!(d.rows.get(1).map(|r| r.default.as_str()), Some("x"), "no default = the first value");
        assert_eq!(w.len(), 2, "{w:?}");
    }

    #[test]
    fn unknown_hooks_and_sources_are_named() {
        let (d, w) = parse(&json!({"protocol":1,"hooks":["preLaunch","describe","x"],"sources":["projects","preLaunch"]})).unwrap();
        assert_eq!((d.hooks, d.sources), (vec!["preLaunch"], vec!["projects"]));
        assert_eq!(w.len(), 3, "{w:?}");
    }
}
