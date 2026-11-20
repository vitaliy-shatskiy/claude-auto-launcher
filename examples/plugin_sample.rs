//! A sample claude-auto plugin (protocol 1): a `ticket` row on the launch screen whose value
//! reaches claude as `SAMPLE_TICKET`, a `j` key on the launch screen, and empty project and
//! session sources. Build it with `cargo build --example plugin_sample` and list the exe in config
//! `plugins` (README, Plugins). The launcher starts it as `<exe> <args...> <verb>`, writes one JSON
//! line to stdin and reads one JSON value from stdout.

use std::io::Read as _;

use serde_json::{Value, json};

/// One answer per verb; an unknown verb exits 2 (the launcher warns and goes on).
fn answer(verb: &str, input: &Value) -> Option<Value> {
    Some(match verb {
        "describe" => json!({
            "protocol": 1,
            "rows": [{ "name": "ticket", "label": "ticket", "values": ["none", "demo-1", "demo-2"], "default": "none" }],
            "actions": [{ "key": "j", "label": "say hello", "screen": "launch" }],
            "hooks": ["preLaunch", "postLaunch"],
            "sources": ["projects", "sessions"],
        }),
        "preLaunch" => match input["choice"]["rows"]["ticket"].as_str() {
            Some("none") | None => json!({}),
            Some(t) => json!({ "env": { "SAMPLE_TICKET": t } }),
        },
        "postLaunch" => json!({}),
        "action" => json!({ "message": format!("hello from the sample plugin ({})", input["key"]) }),
        "projects" | "sessions" => json!([]),
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
