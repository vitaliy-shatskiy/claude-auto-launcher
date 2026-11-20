//! Plugins, protocol v1: processes listed in config `plugins`, started as `<path> <args> <verb>`
//! with JSON on stdin and one JSON value on stdout. Nothing unlisted ever runs. A crash, a timeout
//! or bad output is one warning and that plugin's part is skipped; only a `preLaunch` `cancel`
//! stops a launch. Exports the verb registry `VERBS`, `load` and `Plugins`; one file per verb.

pub mod action;
mod cache;
pub mod describe;
pub mod post_launch;
pub mod pre_launch;
pub mod projects;
pub mod run;
pub mod sessions;

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::config::PluginSpec;
use crate::launch::{LaunchState, Roster, RowName};
use describe::{Description, Row};
use run::Runner;

/// What a verb is for: `describe` names the hooks and sources it wants by these kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Describe,
    Hook,
    Source,
    Action,
}

/// One verb of the protocol and its default timeout (`timeoutMs` overrides it per plugin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verb {
    pub name: &'static str,
    pub timeout_ms: u64,
    pub kind: Kind,
}

/// Every verb of protocol 1: a new verb is a new file plus one line here.
pub const VERBS: [&Verb; 6] = [
    &describe::VERB,
    &pre_launch::VERB,
    &post_launch::VERB,
    &action::VERB,
    &projects::VERB,
    &sessions::VERB,
];

/// The cache file beside the config: `claude-auto-plugins.json`.
pub fn cache_path(config_path: &str) -> Option<std::path::PathBuf> {
    Path::new(config_path).parent().map(|d| d.join("claude-auto-plugins.json"))
}

/// One described plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub spec: PluginSpec,
    pub desc: Description,
}

/// The plugins this run uses, in config order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plugins {
    pub list: Vec<Loaded>,
}

/// The warning line for a failed call.
pub fn warning(spec: &PluginSpec, verb: &Verb, what: &dyn std::fmt::Display) -> String {
    format!("plugin {} {}: {what}; skipped", spec.name, verb.name)
}

/// `describe` for every listed plugin at once, from the cache while the exe is unchanged. With
/// `may_run` false (preview, `--launcher-options`) no process starts: an uncached plugin is absent.
pub fn load(specs: &[PluginSpec], cache_file: Option<&Path>, runner: &dyn Runner, may_run: bool) -> (Plugins, Vec<String>) {
    let mut cache = cache::Cache::open(cache_file);
    let answers: Vec<Option<Result<(Value, bool), String>>> = std::thread::scope(|s| {
        let cache = &cache;
        let running: Vec<_> = specs
            .iter()
            .map(|spec| {
                s.spawn(move || match cache.get(spec) {
                    Some(v) => Some(Ok((v.clone(), false))),
                    None if may_run => Some(
                        runner
                            .call(spec, &describe::VERB, &json!({}))
                            .map(|v| (v, true))
                            .map_err(|e| warning(spec, &describe::VERB, &e)),
                    ),
                    None => None,
                })
            })
            .collect();
        running.into_iter().map(|t| t.join().unwrap_or(None)).collect()
    });
    let mut plugins = Plugins::default();
    let mut warnings = Vec::new();
    for (spec, answer) in specs.iter().zip(answers) {
        let Some(answer) = answer else { continue };
        let parsed = answer.and_then(|(v, fresh)| {
            let (desc, w) = describe::parse(&v).map_err(|e| warning(spec, &describe::VERB, &e))?;
            if fresh {
                cache.store(spec, &v);
            }
            Ok((desc, w))
        });
        match parsed {
            Ok((mut desc, w)) => {
                if desc.protocol > describe::PROTOCOL {
                    warnings.push(format!(
                        "plugin {} speaks protocol {}; this launcher speaks {p} and uses only the protocol {p} fields",
                        spec.name,
                        desc.protocol,
                        p = describe::PROTOCOL
                    ));
                }
                warnings.extend(w.into_iter().map(|w| format!("plugin {}: {w}; skipped", spec.name)));
                drop_taken_keys(&plugins, spec, &mut desc, &mut warnings);
                plugins.list.push(Loaded { spec: spec.clone(), desc });
            }
            Err(w) => warnings.push(w),
        }
    }
    cache.save();
    (plugins, warnings)
}

/// An action key an earlier plugin already holds on that screen: the first one wins.
fn drop_taken_keys(plugins: &Plugins, spec: &PluginSpec, desc: &mut Description, warnings: &mut Vec<String>) {
    desc.actions.retain(|a| {
        let taken = plugins
            .list
            .iter()
            .find(|l| l.desc.actions.iter().any(|b| b.key == a.key && b.screen == a.screen));
        if let Some(l) = taken {
            warnings.push(format!(
                "plugin {}: action '{}' is taken by plugin {}; skipped",
                spec.name, a.key, l.spec.name
            ));
        }
        taken.is_none()
    });
}

impl Plugins {
    /// Every plugin row in order with its plugin's index: row `i` here is `RowName::Plugin(i)`.
    fn rows(&self) -> impl Iterator<Item = (usize, &Row)> {
        self.list
            .iter()
            .enumerate()
            .flat_map(|(p, l)| l.desc.rows.iter().map(move |r| (p, r)))
    }

    /// The plugin rows onto the launch screen, after the built-in ones.
    pub fn add_rows(&self, roster: &mut Roster) {
        for (p, r) in self.rows() {
            let plugin = self.list.get(p).map_or("", |l| l.spec.name.as_str());
            if roster
                .add_plugin_row(&format!("{plugin}.{}", r.name), &r.label, &r.values, &r.default)
                .is_none()
            {
                break;
            }
        }
    }

    /// `plugin`'s rows as the state holds them; a value that is not one of the row's own is its default.
    pub fn own_rows(&self, plugin: usize, state: &LaunchState) -> Map<String, Value> {
        self.rows()
            .enumerate()
            .filter(|(_, (p, _))| *p == plugin)
            .map(|(i, (_, r))| {
                let v = u16::try_from(i).map_or("", |i| state.get(RowName::Plugin(i)));
                let v = r.values.iter().find(|x| *x == v).unwrap_or(&r.default);
                (r.name.clone(), json!(v))
            })
            .collect()
    }

    /// Plugin `plugin`'s view of a launch: the screen's choices and its own rows.
    pub fn choice(&self, plugin: usize, state: &LaunchState, resume_id: Option<&str>) -> Value {
        json!({
            "account": state.account, "project": state.project, "action": state.action,
            "model": state.model, "effort": state.effort, "advisor": state.advisor,
            "permission": state.permission, "mode": state.mode, "resumeId": resume_id,
            "rows": self.own_rows(plugin, state),
        })
    }

    /// Every plugin that wants `verb`, called at once; answers (or warnings) in config order.
    pub fn call_each(
        &self,
        verb: &Verb,
        input: &(dyn Fn(usize) -> Value + Sync),
        runner: &dyn Runner,
    ) -> Vec<(usize, Result<Value, String>)> {
        std::thread::scope(|s| {
            let running: Vec<_> = self
                .list
                .iter()
                .enumerate()
                .filter(|(_, l)| l.desc.wants(verb))
                .map(|(i, l)| {
                    let call = move || runner.call(&l.spec, verb, &input(i)).map_err(|e| warning(&l.spec, verb, &e));
                    (i, s.spawn(call))
                })
                .collect();
            running
                .into_iter()
                .map(|(i, t)| {
                    (
                        i,
                        t.join()
                            .unwrap_or_else(|_| Err(format!("plugin call {} panicked; skipped", verb.name))),
                    )
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests;
