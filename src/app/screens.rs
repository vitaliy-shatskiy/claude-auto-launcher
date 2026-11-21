//! The UI sequence of one launch: launch screen, project screen, held-session warning, picker.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::background::Background;
use crate::config::{PathCtx, ProjectScreen};
use crate::frame::Style;
use crate::input::console::ConsoleInput;
use crate::input::{KeyPress, is_hotkey};
use crate::launch::LaunchState;
use crate::launch::PROJECT_ACTION;
use crate::launch::launch_mode;
use crate::launch::screen::{LaunchScreen, LaunchView, RowsStore};
use crate::log::Logger;
use crate::log_data;
use crate::out;
use crate::prefs;
use crate::types::{LabelDefaults, VersionInfo};
use crate::ui::{ConsoleTerm, PreviewTerm, Term, UiStop, parse_preview_keys, parse_preview_size};
use crate::{env, maintenance, projects, self_update, sessions, sys, theme};

use super::plugins::{launch_key, project_entries, session_entries};
use super::waits::{confirm_held, open_maintenance, wait_on_screen};
use super::{Run, UiEnd, UiOutcome, now_ms, var};

/// The launcher's own release check: a thread, never in the way of the first frame, started only
/// for a start with screens. The caller holds it until claude exits: a download that outlasts the
/// screens still lands, and its drop keeps a swap from starting once the launcher is quitting.
pub(super) fn self_update_watch(run: &Run, use_ui: bool) -> (self_update::Local, RefCell<self_update::Watch>) {
    let start = if run.preview {
        self_update::Start::Preview
    } else {
        self_update::Start::Launch
    };
    let local = self_update::Local::from_env(run.cfg.self_update, &run.cfg.update_repo, &run.ctx.home, start);
    let checked = if use_ui { local.clone() } else { local.off() };
    (local, RefCell::new(self_update::Watch::start(checked, now_ms())))
}

/// The UI: launch screen, project screen, held-session warning, picker - until a launch.
#[expect(clippy::too_many_lines, reason = "the screen sequence reads top to bottom as in claude-auto.ps1")]
pub(super) fn run_ui(
    run: &Run,
    input_redirected: bool,
    output_redirected: bool,
    (local_update, launcher): (&self_update::Local, &RefCell<self_update::Watch>),
) -> Result<UiEnd, UiStop> {
    let style = Style {
        color: theme::color_supported(var("NO_COLOR").as_deref(), var("TERM").as_deref()),
        ascii: theme::ascii_required(var("CLAUDE_AUTO_ASCII").as_deref(), sys::output_code_page()),
    };
    let tm = &run.logger.timings;
    tm.set_style(style.color, style.ascii);
    let load = tm.begin(&["load", "launch"]);
    let version: VersionInfo = maintenance::read_version();
    let canonical_root = run.cfg.canonical().map(|a| a.root.clone()).unwrap_or_default();
    let account_dirs: Vec<(String, String)> = run.cfg.accounts.iter().map(|a| (a.key.clone(), a.root.clone())).collect();
    let mut roster = run.roster.clone();
    // Read before the check starts: an install re-reads the catalog, and its cache write is plain.
    let labels = {
        let _s = tm.scope(&["model catalog"]);
        env::model_family_labels(run.preview)
    };
    if let Some(labels) = labels {
        roster.set_model_labels(&labels);
    }
    // The check runs behind the launch screen; Enter waits for what is left of it. Declared before
    // the console so an exit drops the console first, then settles the check.
    let update = RefCell::new(if !run.preview && var("CLAUDE_AUTO_NO_UPDATE").as_deref() != Some("1") {
        let (installed, root, dirs) = (version.clone(), canonical_root.clone(), account_dirs.clone());
        maintenance::UpdateWatch::start(version.clone(), move |gate| {
            let mut found = maintenance::check_update(installed, gate);
            // An install re-read the catalog: `default (X)` names the new build's models.
            if found.labels.is_some() {
                found.defaults = Some(label_defaults(&root, &dirs));
            }
            found
        })
    } else {
        maintenance::UpdateWatch::off(version.clone())
    });
    let accounts = tm.begin(&["accounts"]);
    let rate_dir = var("CLAUDE_AUTO_RATE_LIMITS_DIR").map_or_else(|| run.ctx.home.join(".claude").join("rate-limits"), PathBuf::from);
    let defaults = label_defaults(&canonical_root, &account_dirs);
    let mut view = LaunchView {
        limits: env::rate_limit_summary(&run.cfg.accounts, &rate_dir, now_ms()),
        version,
        default_model_label: defaults.model,
        default_advisor_label: defaults.advisor,
        labels_by_account: defaults.by_account,
        running: Vec::new(),
        mcp: maintenance::last_mcp_check(&run.ctx.home, theme::glyphs(style.ascii).dot),
        style,
    };
    tm.end(accounts);
    let console = tm.begin(&["console"]);
    let mut term: Box<dyn Term> = if run.preview {
        let keys = parse_preview_keys(&var("CLAUDE_AUTO_PREVIEW_KEYS").unwrap_or_default()).map_err(|e| {
            out::err_line(&e);
            UiStop::KeysExhausted
        })?;
        Box::new(
            PreviewTerm::new(parse_preview_size(var("CLAUDE_AUTO_PREVIEW_SIZE").as_deref()), keys, false).with_logger(run.logger.clone()),
        )
    } else {
        let mouse = mouse_armed(run.cfg.mouse, var("CLAUDE_AUTO_NO_MOUSE").as_deref());
        let trace = (var("CLAUDE_AUTO_INPUT_TRACE").as_deref() == Some("1")).then(|| trace_path(&run.ctx));
        let Some(input) = ConsoleInput::open(mouse, output_redirected, trace) else {
            return Ok(UiEnd::NoConsole);
        };
        let alt = !output_redirected && !input_redirected;
        let truecolor = theme::truecolor_supported(var("COLORTERM").as_deref(), var("WT_SESSION").as_deref());
        Box::new(ConsoleTerm::open(alt, Some(input), run.logger.clone()).with_truecolor(truecolor))
    };

    tm.end(console);
    let prefs_span = tm.begin(&["prefs"]);
    let prefs_file = prefs::read_prefs(&prefs::prefs_path(&run.ctx, var("CLAUDE_AUTO_PREFS").as_deref()));
    let mut state = roster.new_state();
    let merged = prefs::merge_prefs(&mut state, &prefs_file, &roster, now_ms());
    tm.end(prefs_span);
    tm.end(load);
    let cache = run.preview.then(|| {
        let tmp = std::env::temp_dir();
        let _s = tm.scope(&["preview cache sweep"]);
        projects::remove_stale_preview_cache(&tmp);
        tmp.join(format!("claude-auto-projects-preview-{}.json", std::process::id()))
    });
    let account_roots: Vec<String> = run.cfg.accounts.iter().map(|a| a.root.clone()).collect();
    let roots = projects::launcher_profile_roots(&run.ctx.home, var("CLAUDE_CONFIG_DIR").as_deref(), &account_roots);
    // The scan runs while the launch screen is up; the start selection waits for it after Enter.
    let (build_cache, hide) = (cache.clone(), run.cfg.hide_headless_projects);
    let prefetch = Background::start("registry", move || projects::project_registry(build_cache.as_deref(), &roots, hide));
    // A cancel or an error exit never waits for the build: its cache write is temp + rename. A
    // preview joins it: its per-PID cache file is removed below.
    let prefetch = RefCell::new(if run.preview { prefetch } else { prefetch.unjoined() });
    // An unwritable or refused config keeps the change for this run only; the screen says why.
    let save_hidden = |path: &str, change: projects::HideChange| -> Result<(), String> {
        if run.preview {
            return Ok(());
        }
        projects::save_hidden_project(&run.cfg.path, &run.ctx, path, change).map_err(|e| e.to_string())
    };
    let hiding = projects::Hiding {
        paths: std::cell::RefCell::new(run.cfg.hidden_projects.clone()),
        pinned: std::cell::RefCell::new(run.cfg.pinned_projects.clone()),
        save: &save_hidden,
    };
    let mut project_source = String::new();
    let mut preselect = String::new();
    let held_dirs: Vec<PathBuf> = run.cfg.accounts.iter().map(|a| Path::new(&a.root).join("sessions")).collect();
    // The running panel: live records only (a dead or reused pid never counts), read off the UI
    // thread and again every two seconds. A preview never idles: it reads them before its first
    // frame, so the screenshot walk shows the panel the owner sees.
    let running_dirs = held_dirs.clone();
    let running = RefCell::new(super::running::RunningWatch::start(move || {
        sessions::holder::running_sessions(&running_dirs, now_ms())
    }));
    if run.preview {
        view.running = sessions::holder::running_sessions(&held_dirs, now_ms());
    }
    let running_here = || running.borrow_mut().poll();
    // The wide launch page's projects panel, once the registry is in (plugin projects join on
    // the project page only: listing them starts their processes).
    let home_clock = now_ms;
    let home_account = state.account.clone();
    let home_source = || {
        let mut p = prefetch.try_borrow_mut().ok()?;
        let reg = p.poll()?.as_ref().ok()?;
        let stores = projects::detail::Stores {
            root: sessions::sessions_root(&run.cfg, &home_account),
            live_dirs: held_dirs.clone(),
        };
        Some(projects::home_list(
            &reg.projects,
            &run.ctx.cwd,
            style,
            &hiding,
            run.preview,
            stores,
            &home_clock,
        ))
    };
    // A preview never idles: the registry is read before its first frame, so the projects panel
    // shows in the printed frames.
    if run.preview {
        let mut p = prefetch.borrow_mut();
        let _ = registry(&mut p, &run.logger, || {});
    }
    let mut chosen_how: Option<String>;
    let mut rows_store = ConfigRows { run };
    // Plugin messages and warnings met on the screens, printed once the console is back.
    let plugin_lines: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let mut plugin_projects: Option<Vec<crate::plugins::projects::Entry>> = None;

    let outcome = loop {
        let mut hook = |k: &KeyPress, t: &mut dyn Term| -> bool {
            if is_hotkey(k, 'u') {
                open_maintenance(&update, tm, t, run.preview, |t| {
                    let shown = maintenance::Shown {
                        warnings: &run.warnings,
                        version: &view.version,
                    };
                    if maintenance::run_screen(t, &run.cfg, style, local_update, &shown).is_ok() {
                        t.drain();
                    }
                });
                return true;
            }
            launch_key(run, k, t, &plugin_lines)
        };
        // Per run of the screen: its borrows end with it (the source outlives the loop).
        let home_here = || home_source();
        let mut screen = LaunchScreen::new(&mut roster, state.clone(), &view, &prefs_file, now_ms(), &mut hook)
            .with_home(&home_here)
            .with_rows(&mut rows_store)
            .with_update(&update)
            .with_self_update(launcher)
            .with_running(&running_here)
            .with_warnings(run.warnings.len());
        let Some(next) = crate::ui::screen_loop::run(&mut screen, term.as_mut())? else {
            break UiEnd::Cancelled;
        };
        state = next;
        let mut prefetch_now = prefetch.borrow_mut();
        let registry_list = registry(
            &mut prefetch_now,
            &run.logger,
            status(term.as_mut(), run.preview, "loading projects..."),
        );
        let extra = plugin_projects.get_or_insert_with(|| project_entries(run, &plugin_lines));
        let merged_projects = crate::plugins::projects::merge(registry_list, extra);
        let project_list: &[projects::Project] = &merged_projects;
        // Deferred from before the launch screen: the rule reads only the cwd and the project the
        // final tab remembers, so applying it once here equals applying it on every tab switch.
        if state.start.is_none() {
            project_source = projects::set_launch_start_project(&mut state, &run.ctx.cwd, project_list);
            preselect.clone_from(&state.project);
        }
        let via_project = state.action == PROJECT_ACTION;
        let action_before = state.action.clone();
        let home_session = state.home_session.take();
        let home_preview = state.home_preview.take();
        let picked = state.home_pick.is_some();
        if let Some(chosen) = state.home_pick.take() {
            // The wide launch page's projects or sessions panel picked: no project page.
            state.project.clone_from(&chosen.path);
            state.project_slug.clone_from(&chosen.slug);
            state.project_slugs.clone_from(&chosen.slugs);
            state.action.clone_from(&chosen.action);
            chosen_how = Some(chosen.how);
            if let Some((id, fork)) = home_session {
                let holder = if fork { None } else { sessions::session_holder(&held_dirs, &id) };
                if confirm_held(term.as_mut(), holder.as_deref(), style)? {
                    break launch_end(
                        state,
                        Some(id),
                        fork,
                        view,
                        roster,
                        merged.restored,
                        project_source,
                        preselect,
                        chosen_how,
                    );
                }
                if run.preview {
                    break UiEnd::PreviewHeldBack;
                }
                state.action.clone_from(&action_before);
                continue;
            }
        } else if opens_project_screen(run.cfg.project_screen, &state.action) {
            let Some(chosen) = projects::run_project_screen(
                term.as_mut(),
                project_list,
                &run.ctx.cwd,
                style,
                &hiding,
                run.preview,
                projects::detail::Stores {
                    root: sessions::sessions_root(&run.cfg, &state.account),
                    live_dirs: held_dirs.clone(),
                },
            )?
            else {
                if run.preview {
                    break UiEnd::PreviewPickerCancelled;
                }
                continue;
            };
            state.project.clone_from(&chosen.path);
            state.project_slug.clone_from(&chosen.slug);
            state.project_slugs.clone_from(&chosen.slugs);
            state.action.clone_from(&chosen.action);
            chosen_how = Some(chosen.how);
        } else {
            start_in_cwd(&mut state, &run.ctx.cwd, project_list);
            chosen_how = Some("cwd".into());
        }
        // Back on the launch screen after a pick, the action row says what it said.
        let back_to = |state: &mut LaunchState| {
            if picked {
                state.action.clone_from(&action_before);
            } else if via_project {
                PROJECT_ACTION.clone_into(&mut state.action);
            }
        };
        let mode = launch_mode(&state.action);
        let slugs = session_slugs(&state);
        let sessions_root = sessions::sessions_root(&run.cfg, &state.account);
        if mode.is_some_and(|m| m.attaches_newest) {
            let holder = {
                let _s = tm.scope(&["held check"]);
                sessions::continue_session_holder(&held_dirs, &slugs, &sessions_root)
            };
            if confirm_held(term.as_mut(), holder.as_deref(), style)? {
                break launch_end(
                    state,
                    None,
                    false,
                    view,
                    roster,
                    merged.restored,
                    project_source,
                    preselect,
                    chosen_how,
                );
            }
            if run.preview {
                break UiEnd::PreviewHeldBack;
            }
            back_to(&mut state);
            continue;
        }
        if !mode.is_some_and(|m| m.picks_session) {
            break launch_end(
                state,
                None,
                false,
                view,
                roster,
                merged.restored,
                project_source,
                preselect,
                chosen_how,
            );
        }
        let name = Path::new(&state.project)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let others = sessions::other_stores(&run.cfg, &state.account);
        let req = sessions::PickerRequest {
            sessions_root,
            others: &others,
            project_slugs: &slugs,
            project_name: &name,
            style,
            live_dirs: &held_dirs,
            preview: run.preview,
            select: home_preview.as_deref(),
        };
        let project = state.project.clone();
        let plugin_sessions = |slugs: &[String]| session_entries(run, &project, slugs, &plugin_lines);
        if let Some(picked) = sessions::run_picker_with(term.as_mut(), &req, &plugin_sessions)? {
            let holder = if picked.fork {
                None
            } else {
                let _s = tm.scope(&["held check"]);
                sessions::session_holder(&held_dirs, &picked.session_id)
            };
            if confirm_held(term.as_mut(), holder.as_deref(), style)? {
                let mut end = launch_end(
                    state,
                    Some(picked.session_id),
                    picked.fork,
                    view,
                    roster,
                    merged.restored,
                    project_source,
                    preselect,
                    chosen_how,
                );
                if let UiEnd::Launch(o) = &mut end {
                    o.transfer = picked.transfer;
                }
                break end;
            }
            if run.preview {
                break UiEnd::PreviewHeldBack;
            }
            back_to(&mut state);
            continue;
        }
        if run.preview {
            break UiEnd::PreviewPickerCancelled;
        }
        back_to(&mut state);
    };
    let mut outcome = outcome;
    let launching = matches!(outcome, UiEnd::Launch(_));
    let mut watch = update.into_inner();
    if wait_on_screen(launching, &mut watch, tm, term.as_mut(), run.preview).is_err() {
        // Esc or Ctrl+C during the wait: claude never starts mid-update, and the exit waits for nothing.
        watch.abandon();
        outcome = UiEnd::Cancelled;
    }
    // The launch reads its defaults from the view: an install moved them.
    if let (UiEnd::Launch(o), Some(Ok(found))) = (&mut outcome, watch.outcome()) {
        o.view.apply(found);
    }
    drop(prefetch);
    drop(term);
    for l in plugin_lines.borrow().iter() {
        out::line(&format!("  {l}"), crate::out::Fg::DarkYellow);
    }
    report_update(&watch, &run.logger);
    if let Some(c) = cache
        && c.exists()
        && std::fs::remove_file(&c).is_err()
    {
        // A stale per-PID cache is swept by the next preview.
    }
    Ok(outcome)
}

/// The `default (X)` labels: the canonical root's model and advisor, every account's defaults.
pub(super) fn label_defaults(canonical_root: &str, accounts: &[(String, String)]) -> LabelDefaults {
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
pub(super) fn status<'t>(term: &'t mut dyn Term, preview: bool, text: &'t str) -> impl FnOnce() + 't {
    move || {
        if !preview {
            term.paint(&[String::new(), format!("  {text}")]);
        }
    }
}

/// The check's span (measured on its thread) and the lines it held back while the screen was up,
/// printed now that the console is back; a check that panicked is logged instead.
fn report_update(update: &maintenance::UpdateWatch, logger: &Logger) {
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
fn registry<'a>(prefetch: &'a mut Background<projects::Registry>, logger: &Logger, on_slow: impl FnOnce()) -> &'a [projects::Project] {
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
struct ConfigRows<'a> {
    run: &'a Run,
}

impl RowsStore for ConfigRows<'_> {
    fn load(&mut self) -> crate::config::RowsConfig {
        crate::config::read_config(&self.run.cfg.path, &self.run.ctx).rows_config()
    }
}

/// `mouse` false or `CLAUDE_AUTO_NO_MOUSE=1`: keys only; the variable wins over the config.
pub(super) fn mouse_armed(config_mouse: bool, no_mouse_env: Option<&str>) -> bool {
    config_mouse && no_mouse_env != Some("1")
}

/// `projectScreen` always: the project screen follows every launch screen; onDemand: only the
/// `project` action opens it.
pub(super) fn opens_project_screen(mode: ProjectScreen, action: &str) -> bool {
    match mode {
        ProjectScreen::Always => true,
        ProjectScreen::OnDemand => action == PROJECT_ACTION,
    }
}

/// A launch without the project screen runs in the launch directory, with every slug it has.
pub(super) fn start_in_cwd(state: &mut LaunchState, cwd: &Path, projects: &[projects::Project]) {
    state.project = cwd.to_string_lossy().into_owned();
    state.project_slugs = projects::slugs_for_path(projects, &state.project);
    state.project_slug = state.project_slugs.first().cloned().unwrap_or_default();
}

/// The slugs the session picker (every account's store) and the held check for `continue` scope
/// on; empty = the whole account.
pub(super) fn session_slugs(state: &LaunchState) -> Vec<String> {
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

#[expect(clippy::too_many_arguments, reason = "the outcome record is assembled once, here")]
fn launch_end(
    state: LaunchState,
    resume_id: Option<String>,
    fork: bool,
    view: LaunchView,
    roster: crate::launch::Roster,
    restored: Vec<String>,
    project_source: String,
    preselect: String,
    chosen_how: Option<String>,
) -> UiEnd {
    UiEnd::Launch(Box::new(UiOutcome {
        state,
        resume_id,
        transfer: None,
        fork,
        view,
        roster,
        restored,
        project_source,
        preselect,
        chosen_how,
    }))
}

fn trace_path(ctx: &PathCtx) -> PathBuf {
    let dir = ctx.home.join(".claude").join("claude-auto-logs");
    if std::fs::create_dir_all(&dir).is_err() {
        // The trace then simply fails to open its file.
    }
    let d = sys::local_now().map_or_else(String::new, |t| format!("{:04}-{:02}-{:02}", t.year, t.month, t.day));
    dir.join(format!("input-{d}-{}.log", std::process::id()))
}
