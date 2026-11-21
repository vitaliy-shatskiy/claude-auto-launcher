//! The small steps around the screens: the start's choices (project screen, cwd, slugs, mouse),
//! the default labels, the status line, the registry wait, the rows store.

use std::path::Path;

use crate::app::Run;
use crate::background::Background;
use crate::config::ProjectScreen;
use crate::launch::screen::RowsStore;
use crate::launch::{LaunchState, PROJECT_ACTION};
use crate::log::Logger;
use crate::log_data;
use crate::out;
use crate::types::LabelDefaults;
use crate::ui::Term;
use crate::{env, maintenance, projects};

/// The `default (X)` labels: the canonical root's model and advisor, every account's defaults.
pub(in crate::app) fn label_defaults(canonical_root: &str, accounts: &[(String, String)]) -> LabelDefaults {
    let settings = |root: &str| Path::new(root).join("settings.json");
    LabelDefaults {
        model: env::default_model_label(&settings(canonical_root)),
        advisor: env::default_advisor_label(&settings(canonical_root)),
        by_account: accounts
            .iter()
            .map(|(key, root)| (key.clone(), env::default_labels(&settings(root))))
            .collect(),
    }
}

/// A one-line frame for a wait after Enter that outlasts `background::SLOW`; none in preview,
/// whose printed frames a harness compares.
pub(in crate::app) fn status<'t>(term: &'t mut dyn Term, preview: bool, text: &'t str) -> impl FnOnce() + 't {
    move || {
        if !preview {
            term.paint(&[String::new(), format!("  {text}")]);
        }
    }
}

/// The check's span (measured on its thread) and the lines it held back while the screen was up,
/// printed now that the console is back; a check that panicked is logged instead.
pub(super) fn report_update(update: &maintenance::UpdateWatch, logger: &Logger) {
    match update.outcome() {
        Some(Ok(found)) => {
            logger.timings.add(&["update check"], None, found.span.0, found.span.1, None);
            for n in &found.lines {
                out::line(&n.text, n.fg);
            }
        }
        Some(Err(e)) => {
            logger.write("error", log_data! { "where" => "update check", "message" => e });
        }
        None => {}
    }
}

/// The prefetched registry. The first call records the build (`load:project`, its `registry:*`
/// steps, measured on the build's thread) and how long the screen waited for it (`wait:project`);
/// a build that panicked is logged once and is an empty list: an optional step never fails a launch.
pub(super) fn registry<'a>(
    prefetch: &'a mut Background<projects::Registry>,
    logger: &Logger,
    on_slow: impl FnOnce(),
) -> &'a [projects::Project] {
    let tm = &logger.timings;
    let (answer, waited) = prefetch.wait(on_slow);
    let Some(w) = waited else {
        return answer.as_ref().map_or(&[], |r| &r.projects);
    };
    match answer {
        Ok(reg) => {
            if let (Some(first), Some(last)) = (reg.phases.first(), reg.phases.last()) {
                let load = tm.add(&["load", "project"], None, first.start, last.end, None);
                for p in &reg.phases {
                    tm.add(&["registry", p.name], Some(load), p.start, p.end, None);
                }
            }
        }
        Err(e) => {
            logger.write("error", log_data! { "where" => "registry", "message" => e });
        }
    }
    tm.add(&["wait", "project"], None, w.start, w.end, None);
    answer.as_ref().map_or(&[], |r| &r.projects)
}

/// Config `rows` and `hiddenValues` on disk, re-read each time.
pub(super) struct ConfigRows<'a> {
    pub(super) run: &'a Run,
}

impl RowsStore for ConfigRows<'_> {
    fn load(&mut self) -> crate::config::RowsConfig {
        crate::config::read_config(&self.run.cfg.path, &self.run.ctx).rows_config()
    }
}

/// `mouse` false or `CLAUDE_AUTO_NO_MOUSE=1`: keys only; the variable wins over the config.
pub(in crate::app) fn mouse_armed(config_mouse: bool, no_mouse_env: Option<&str>) -> bool {
    config_mouse && no_mouse_env != Some("1")
}

/// `projectScreen` always: the project screen follows every launch screen; onDemand: only the
/// `project` action opens it.
pub(in crate::app) fn opens_project_screen(mode: ProjectScreen, action: &str) -> bool {
    match mode {
        ProjectScreen::Always => true,
        ProjectScreen::OnDemand => action == PROJECT_ACTION,
    }
}

/// A launch without the project screen runs in the launch directory, with every slug it has.
pub(in crate::app) fn start_in_cwd(state: &mut LaunchState, cwd: &Path, projects: &[projects::Project]) {
    state.project = cwd.to_string_lossy().into_owned();
    state.project_slugs = projects::slugs_for_path(projects, &state.project);
    state.project_slug = state.project_slugs.first().cloned().unwrap_or_default();
}

/// The slugs the session picker (every account's store) and the held check for `continue` scope
/// on; empty = the whole account.
pub(in crate::app) fn session_slugs(state: &LaunchState) -> Vec<String> {
    let mut slugs: Vec<String> = state.project_slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
    if slugs.is_empty() && !state.project_slug.is_empty() {
        slugs.push(state.project_slug.clone());
    }
    // A directory the registry does not know: its own transcript folder, never the whole account
    // filtered by leaf name, which read every page to find none of its rows (149 pages, 5.5 s).
    if slugs.is_empty() && !state.project.is_empty() {
        slugs.push(crate::plugins::projects::claude_slug(&state.project));
    }
    slugs
}
