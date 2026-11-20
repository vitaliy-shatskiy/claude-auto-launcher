//! The launch's plugin wiring: the described plugins onto the roster, a plugin key and the source
//! lists on the screens, `preLaunch` before claude starts and `postLaunch` once it has ended.
//! Every call goes through the real `Process` runner; preview starts none.

use std::cell::RefCell;

use serde_json::Value;

use crate::config::Config;
use crate::input::KeyPress;
use crate::launch::Roster;
use crate::log_data;
use crate::out::{self, Fg};
use crate::plugins;
use crate::sessions::listing::Session;
use crate::timing::Timings;
use crate::types::ChildEnv;
use crate::ui::Term;

use super::screens::status;
use super::{Run, UiOutcome, ms};

/// `describe` for every listed plugin and their rows onto the roster. Preview and
/// `--launcher-options` start no process (`may_run` false): they see only cached descriptions.
pub(super) fn load(cfg: &Config, timings: &Timings, roster: &mut Roster, may_run: bool) -> (plugins::Plugins, Vec<String>) {
    let (plugins, plugin_warnings) = {
        let _s = timings.scope(&["plugins"]);
        let cache = plugins::cache_path(&cfg.path);
        plugins::load(&cfg.plugins, cache.as_deref(), &plugins::run::Process, may_run)
    };
    plugins.add_rows(roster);
    (plugins, plugin_warnings)
}

/// A plugin's key on the launch screen runs its action (never in preview); `false` = not one.
pub(super) fn launch_key(run: &Run, k: &KeyPress, t: &mut dyn Term, lines: &RefCell<Vec<String>>) -> bool {
    let Some((p, key)) = plugins::action::find(&run.plugins, "launch", k) else {
        return false;
    };
    if !run.preview {
        status(t, false, "running plugin action...")();
        let context = serde_json::json!({ "screen": "launch", "cwd": run.ctx.cwd.to_string_lossy() });
        lines
            .borrow_mut()
            .extend(plugins::action::run(&run.plugins, p, key, &context, &plugins::run::Process));
    }
    true
}

/// The source plugins' projects (none in preview); their warnings go to `lines`.
pub(super) fn project_entries(run: &Run, lines: &RefCell<Vec<String>>) -> Vec<plugins::projects::Entry> {
    let mut w = Vec::new();
    let found = if run.preview {
        Vec::new()
    } else {
        plugins::projects::collect(&run.plugins, &plugins::run::Process, &mut w)
    };
    lines.borrow_mut().extend(w);
    found
}

/// The source plugins' sessions of `project`, or of every project when the picker has no slugs
/// (none in preview); their warnings go to `lines`.
pub(super) fn session_entries(run: &Run, project: &str, slugs: &[String], lines: &RefCell<Vec<String>>) -> Vec<Session> {
    if run.preview {
        return Vec::new();
    }
    let mut w = Vec::new();
    let scope = if slugs.is_empty() { "" } else { project };
    let found = plugins::sessions::collect(&run.plugins, scope, &plugins::run::Process, &mut w);
    lines.borrow_mut().extend(w);
    found
}

/// `preLaunch`: its variables into the session's environment, its arguments for claude; a
/// `cancel` is the exit code of a launch that does not happen.
pub(super) fn pre_launch(run: &Run, o: &UiOutcome, child: &mut ChildEnv) -> Result<Vec<String>, i32> {
    let mut warnings = Vec::new();
    let choices: Vec<Value> = (0..run.plugins.list.len())
        .map(|i| run.plugins.choice(i, &o.state, o.resume_id.as_deref()))
        .collect();
    let choice = |i: usize| choices.get(i).cloned().unwrap_or(Value::Null);
    let outcome = plugins::pre_launch::run(&run.plugins, &choice, &plugins::run::Process, &mut warnings);
    for w in &warnings {
        out::line(&format!("  {w}"), Fg::DarkYellow);
    }
    match outcome {
        plugins::pre_launch::Outcome::Go(extra) => {
            for (k, v) in &extra.env {
                child.set(k, v);
            }
            Ok(extra.args)
        }
        plugins::pre_launch::Outcome::Cancel { plugin, reason } => {
            let why = if reason.is_empty() { String::new() } else { format!(": {reason}") };
            out::line(&format!("  launch cancelled by plugin {plugin}{why}"), Fg::Red);
            run.logger
                .write("cancelled", log_data! { "ms" => ms(run), "plugin" => plugin, "reason" => reason });
            Err(1)
        }
    }
}

/// `postLaunch` once claude has ended; its warnings are the last lines.
pub(super) fn post_launch(run: &Run, ui: Option<&UiOutcome>, code: i32) {
    let Some(o) = ui else { return };
    let choices: Vec<Value> = (0..run.plugins.list.len())
        .map(|i| run.plugins.choice(i, &o.state, o.resume_id.as_deref()))
        .collect();
    let choice = |i: usize| choices.get(i).cloned().unwrap_or(Value::Null);
    for w in plugins::post_launch::run(&run.plugins, &choice, code, &plugins::run::Process) {
        out::line(&format!("  {w}"), Fg::DarkYellow);
    }
}
