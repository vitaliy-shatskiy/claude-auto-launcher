//! `schema/claude-auto.schema.json` against the parser: the same top-level keys, enums and defaults
//! as the code, and the shipped configs validate. No JSON Schema crate: a validator for the keywords
//! the schema uses, which refuses any other keyword so the schema cannot outgrow it unnoticed.

use super::*;

const SCHEMA: &str = include_str!("../../schema/claude-auto.schema.json");
const SHIPPED: [(&str, &str); 2] = [
    ("config.example.json", include_str!("../../config.example.json")),
    ("config-four.json", include_str!("../../tests/fixtures/config-four.json")),
];

/// The keywords `errors` understands; annotations (`title`, `description`, `default`) included.
const KEYWORDS: [&str; 17] = [
    "$schema",
    "$id",
    "title",
    "description",
    "default",
    "type",
    "enum",
    "properties",
    "additionalProperties",
    "required",
    "items",
    "anyOf",
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "pattern",
];

fn schema() -> Value {
    serde_json::from_str(SCHEMA).unwrap()
}

fn type_ok(t: &str, v: &Value) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "integer" => v.is_i64() || v.is_u64(),
        "number" => v.is_number(),
        "null" => v.is_null(),
        _ => false,
    }
}

/// Every unsupported keyword anywhere in `s`, by its path.
fn unsupported(s: &Value, at: &str, out: &mut Vec<String>) {
    let Some(o) = s.as_object() else {
        out.push(format!("{at}: a schema must be an object"));
        return;
    };
    out.extend(
        o.keys()
            .filter(|k| !KEYWORDS.contains(&k.as_str()))
            .map(|k| format!("{at}: keyword {k}")),
    );
    for (k, sub) in o.get("properties").and_then(Value::as_object).into_iter().flatten() {
        unsupported(sub, &format!("{at}.{k}"), out);
    }
    if let Some(items) = o.get("items") {
        unsupported(items, &format!("{at}[]"), out);
    }
    for (i, sub) in o.get("anyOf").and_then(Value::as_array).into_iter().flatten().enumerate() {
        unsupported(sub, &format!("{at}|{i}"), out);
    }
}

/// Where `v` breaks schema `s`, by path.
fn errors(s: &Value, v: &Value, at: &str, out: &mut Vec<String>) {
    let kw = |k: &str| s.get(k);
    if let Some(t) = kw("type") {
        let types: Vec<&str> = t.as_str().map_or_else(
            || t.as_array().into_iter().flatten().filter_map(Value::as_str).collect(),
            |t| vec![t],
        );
        if !types.iter().any(|t| type_ok(t, v)) {
            out.push(format!("{at}: {v} is not {t}"));
        }
    }
    if let Some(e) = kw("enum").and_then(Value::as_array).filter(|e| !e.contains(v)) {
        out.push(format!("{at}: {v} is not one of {}", Value::Array(e.clone())));
    }
    if let Some(n) = v.as_f64()
        && (kw("minimum").and_then(Value::as_f64).is_some_and(|m| n < m) || kw("maximum").and_then(Value::as_f64).is_some_and(|m| n > m))
    {
        out.push(format!("{at}: {n} out of range"));
    }
    if let Some(t) = v.as_str() {
        let len = u64::try_from(t.chars().count()).unwrap();
        if kw("minLength").and_then(Value::as_u64).is_some_and(|m| len < m)
            || kw("maxLength").and_then(Value::as_u64).is_some_and(|m| len > m)
        {
            out.push(format!("{at}: '{t}' length out of range"));
        }
        if let Some(p) = kw("pattern").and_then(Value::as_str)
            && !regex::Regex::new(p).unwrap().is_match(t)
        {
            out.push(format!("{at}: '{t}' does not match {p}"));
        }
    }
    if let Some(o) = v.as_object() {
        let props = kw("properties").and_then(Value::as_object);
        for (k, x) in o {
            match props.and_then(|p| p.get(k)) {
                Some(sub) => errors(sub, x, &format!("{at}.{k}"), out),
                None if kw("additionalProperties") == Some(&Value::Bool(false)) => out.push(format!("{at}: unknown key {k}")),
                None => {}
            }
        }
        for r in kw("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !o.contains_key(r) {
                out.push(format!("{at}: {r} is required"));
            }
        }
    }
    if let (Some(a), Some(items)) = (v.as_array(), kw("items")) {
        for (i, x) in a.iter().enumerate() {
            errors(items, x, &format!("{at}[{i}]"), out);
        }
    }
    if let Some(any) = kw("anyOf").and_then(Value::as_array) {
        let fits = |alt: &Value| {
            let mut e = Vec::new();
            errors(alt, v, at, &mut e);
            e.is_empty()
        };
        if !any.iter().any(fits) {
            out.push(format!("{at}: {v} matches no anyOf branch"));
        }
    }
}

fn validate(config: &Value) -> Vec<String> {
    let mut out = Vec::new();
    errors(&schema(), config, "$", &mut out);
    out
}

fn sorted(keys: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = keys.into_iter().collect();
    v.sort();
    v
}

#[test]
fn the_schema_names_exactly_the_keys_the_parser_knows() {
    // A module key is in KNOWN_KEYS only in a build with that module; the schema lists it always.
    let parser = KNOWN_KEYS
        .iter()
        .copied()
        .chain(MODULE_KEYS.iter().map(|m| m.key))
        .map(str::to_owned);
    let mut parser = sorted(parser);
    parser.dedup();
    let schema = sorted(schema()["properties"].as_object().unwrap().keys().cloned());
    assert_eq!(schema, parser);
}

#[test]
fn the_schema_uses_only_keywords_the_check_understands_and_documents_every_key() {
    let s = schema();
    let mut out = Vec::new();
    unsupported(&s, "$", &mut out);
    for (k, sub) in s["properties"].as_object().unwrap() {
        if sub.get("description").and_then(Value::as_str).is_none_or(str::is_empty) {
            out.push(format!("{k}: no description"));
        }
        if let Some(d) = sub.get("default") {
            errors(sub, d, &format!("{k} default"), &mut out);
        }
    }
    assert_eq!(out, Vec::<String>::new());
}

#[test]
fn the_shipped_configs_validate_and_parse_without_warnings() {
    for (name, text) in SHIPPED {
        let v: Value = serde_json::from_str(text).unwrap();
        assert_eq!(validate(&v), Vec::<String>::new(), "{name}");
    }
    let (name, example) = SHIPPED[0];
    let cfg = parse_config(example, name, &ctx_for_schema());
    let module_off = |w: &String| w.contains("this build has no");
    assert_eq!(
        cfg.warnings.iter().filter(|w| !module_off(w)).collect::<Vec<_>>(),
        Vec::<&String>::new()
    );
}

#[test]
fn the_check_finds_an_unknown_key_a_wrong_type_and_a_bad_value() {
    let bad = serde_json::json!({
        "acounts": [],
        "sharing": "yes",
        "riderMcp": "sometimes",
        "plugins": [{ "name": "a b", "path": "C:/p.exe", "timeoutMs": 0 }],
        "launchHooks": [1]
    });
    let e = validate(&bad);
    for want in [
        "unknown key acounts",
        "$.sharing",
        "$.riderMcp",
        "$.plugins[0].name",
        "$.plugins[0].timeoutMs",
        "$.launchHooks[0]",
    ] {
        assert!(e.iter().any(|x| x.contains(want)), "{want} in {e:#?}");
    }
}

#[test]
fn the_schema_enums_and_defaults_are_the_codes() {
    let p = &schema()["properties"];
    let strings = |v: &Value| sorted(v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_owned()));
    assert_eq!(
        strings(&p["accounts"]["items"]["properties"]["tint"]["enum"]),
        sorted(ALLOWED_TINTS.map(str::to_owned))
    );
    assert_eq!(
        strings(&p["rows"]["items"]["enum"]),
        sorted(CONFIG_ROWS.map(|r| r.as_str().to_lowercase()))
    );
    assert_eq!(
        strings(&p["modelOrder"]["items"]["enum"]),
        sorted(MODEL_FAMILY_KEYS.map(|(f, _)| f.to_owned()))
    );
    assert_eq!(p["updateRepo"]["default"], crate::self_update::DEFAULT_REPO);
    assert_eq!(p["version"]["default"], CONFIG_VERSION);
}

fn ctx_for_schema() -> PathCtx {
    PathCtx {
        home: std::path::PathBuf::from(r"C:\Users\u"),
        cwd: std::path::PathBuf::from(r"C:\work"),
    }
}
