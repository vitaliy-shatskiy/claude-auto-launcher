//! `postLaunch {choice, exitCode}`: told once claude has ended; its answer is not used. Exports
//! `VERB` and `run`.

use serde_json::{Value, json};

use super::run::Runner;
use super::{Kind, Plugins, Verb};

pub const VERB: Verb = Verb {
    name: "postLaunch",
    timeout_ms: 10_000,
    kind: Kind::Hook,
};

/// Every plugin with the hook; failures come back as warnings.
pub fn run(plugins: &Plugins, choice: &(dyn Fn(usize) -> Value + Sync), exit_code: i32, runner: &dyn Runner) -> Vec<String> {
    plugins
        .call_each(&VERB, &|i| json!({ "choice": choice(i), "exitCode": exit_code }), runner)
        .into_iter()
        .filter_map(|(_, r)| r.err())
        .collect()
}
