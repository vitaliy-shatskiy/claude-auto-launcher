//! The UI sequence of one launch: launch screen, project screen, held-session warning, picker.

use std::path::{Path, PathBuf};

use crate::background::Background;
use crate::config::{PathCtx, ProjectScreen};
use crate::frame::{self, FooterLight, Frame, Hint, RowMap, Style};
use crate::input::console::ConsoleInput;
use crate::input::{KeyPress, is_hotkey};
use crate::launch::LaunchState;
use crate::launch::PROJECT_ACTION;
use crate::launch::launch_mode;
use crate::launch::screen::{LaunchScreen, LaunchView, RowsStore};
use crate::layout::split_text_lines;
use crate::log::Logger;
use crate::log_data;
use crate::out;
use crate::prefs;
use crate::timing::Timings;
use crate::types::VersionInfo;
use crate::ui::screen_loop::{Cursor, Flow, Screen};
use crate::ui::{ConsoleTerm, PreviewTerm, Term, UiStop, parse_preview_keys, parse_preview_size};
use crate::{env, maintenance, projects, sessions, sys, theme};

use super::{Run, UiEnd, UiOutcome, now_ms, var};

/// The UI: launch screen, project screen, held-session warning, picker - until a launch.
#[expect(clippy::too_many_lines, reason = "the screen sequence reads top to bottom as in claude-auto.ps1")]
pub(super) fn run_ui(run: &Run, input_redirected: bool, output_redirected: bool) -> Result<UiEnd, UiStop> {
    let style = Style {
        color: theme::color_supported(var("NO_COLOR").as_deref(), var("TERM").as_deref()),
        ascii: theme::ascii_required(var("CLAUDE_AUTO_ASCII").as_deref(), sys::output_code_page()),
    };
    let tm = &run.logger.timings;
    tm.set_style(style.color, style.ascii);
    let load = tm.begin(&["load", "launch"]);
    let version: VersionInfo = maintenance::read_version();
    // The check runs behind the launch screen; Enter waits for what is left of it. Declared before
    // the console so an exit drops the console first, then joins the check.
    let mut update = if !run.preview && var("CLAUDE_AUTO_NO_UPDATE").as_deref() != Some("1") {
        let installed = version.clone();
        maintenance::UpdateWatch::start(version.clone(), move || maintenance::check_update(installed))
    } else {
        maintenance::UpdateWatch::off(version.clone())
    };
    let mut roster = run.roster.clone();
    let labels = {
        let _s = tm.scope(&["model catalog"]);
        env::model_family_labels(run.preview)
    };
    if let Some(labels) = labels {
        roster.set_model_labels(&labels);
    }
    let accounts = tm.begin(&["accounts"]);
    let canonical_root = run.cfg.canonical().map(|a| a.root.clone()).unwrap_or_default();
    let settings = |root: &str| Path::new(root).join("settings.json");
    let rate_dir = var("CLAUDE_AUTO_RATE_LIMITS_DIR").map_or_else(|| run.ctx.home.join(".claude").join("rate-limits"), PathBuf::from);
    let view = LaunchView {
        limits: env::rate_limit_summary(&run.cfg.accounts, &rate_dir, now_ms()),
        version,
        default_model_label: env::default_model_label(&settings(&canonical_root)),
        default_advisor_label: env::default_advisor_label(&settings(&canonical_root)),
        labels_by_account: run
            .cfg
            .accounts
            .iter()
            .map(|a| (a.key.clone(), env::default_labels(&settings(&a.root))))
            .collect(),
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
        Box::new(ConsoleTerm::open(alt, Some(input), run.logger.clone()))
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
    let mut prefetch = Background::start("registry", move || projects::project_registry(build_cache.as_deref(), &roots, hide));
    // An unwritable or refused config keeps the change for this run only; the screen says why.
    let save_hidden = |path: &str, change: projects::HideChange| -> Result<(), String> {
        if run.preview {
            return Ok(());
        }
        projects::save_hidden_project(&run.cfg.path, &run.ctx, path, change).map_err(|e| e.to_string())
    };
    let hiding = projects::Hiding {
        paths: std::cell::RefCell::new(run.cfg.hidden_projects.clone()),
        save: &save_hidden,
    };
    let mut project_source = String::new();
    let mut preselect = String::new();
    let held_dirs: Vec<PathBuf> = run.cfg.accounts.iter().map(|a| Path::new(&a.root).join("sessions")).collect();
    let mut chosen_how: Option<String>;
    let mut rows_store = ConfigRows { run };

    let outcome = loop {
        let mut hook = |k: &KeyPress, t: &mut dyn Term| -> bool {
            if is_hotkey(k, 'u') {
                if maintenance::run_screen(t, &run.cfg, style).is_err() {
                    return true;
                }
                t.drain();
                return true;
            }
            false
        };
        let mut screen = LaunchScreen::new(&mut roster, state.clone(), &view, &prefs_file, now_ms(), &mut hook)
            .with_rows(&mut rows_store)
            .with_update(&mut update);
        let Some(next) = crate::ui::screen_loop::run(&mut screen, term.as_mut())? else {
            break UiEnd::Cancelled;
        };
        state = next;
        let project_list = registry(
            &mut prefetch,
            &run.logger,
            status(term.as_mut(), run.preview, "loading projects..."),
        );
        // Deferred from before the launch screen: the rule reads only the cwd and the project the
        // final tab remembers, so applying it once here equals applying it on every tab switch.
        if state.start.is_none() {
            project_source = projects::set_launch_start_project(&mut state, &run.ctx.cwd, project_list);
            preselect.clone_from(&state.project);
        }
        let via_project = state.action == PROJECT_ACTION;
        if opens_project_screen(run.cfg.project_screen, &state.action) {
            let Some(chosen) = projects::run_project_screen(term.as_mut(), project_list, &run.ctx.cwd, style, &hiding, run.preview)? else {
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
        // Back on the launch screen after a pick, the action row still says `project`.
        let back_to = |state: &mut LaunchState| {
            if via_project {
                PROJECT_ACTION.clone_into(&mut state.action);
            }
        };
        let mode = launch_mode(&state.action);
        let mut slugs: Vec<String> = state.project_slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
        if slugs.is_empty() && !state.project_slug.is_empty() {
            slugs.push(state.project_slug.clone());
        }
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
        let req = sessions::PickerRequest {
            sessions_root,
            project_slugs: &slugs,
            project_name: &name,
            style,
            preview: run.preview,
        };
        if let Some(picked) = sessions::run_picker(term.as_mut(), &req)? {
            let holder = if picked.fork {
                None
            } else {
                let _s = tm.scope(&["held check"]);
                sessions::session_holder(&held_dirs, &picked.session_id)
            };
            if confirm_held(term.as_mut(), holder.as_deref(), style)? {
                break launch_end(
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
    let launching = matches!(outcome, UiEnd::Launch(_));
    wait_for_update(
        launching,
        &mut update,
        tm,
        status(term.as_mut(), run.preview, "checking for a Claude Code update..."),
    );
    drop(prefetch);
    drop(term);
    report_update(&update, &run.logger);
    if let Some(c) = cache
        && c.exists()
        && std::fs::remove_file(&c).is_err()
    {
        // A stale per-PID cache is swept by the next preview.
    }
    Ok(outcome)
}

/// Claude never starts mid-update: a launch waits for the check (its fetch is bounded by 5 s) and
/// records `wait:update`; a cancel leaves the check to the drop's join.
pub(super) fn wait_for_update(launching: bool, update: &mut maintenance::UpdateWatch, tm: &Timings, on_slow: impl FnOnce()) {
    if !launching {
        return;
    }
    if let Some(w) = update.finish(on_slow) {
        tm.add(&["wait", "update"], None, w.start, w.end, None);
    }
}

/// A one-line frame for a wait after Enter that outlasts `background::SLOW`; none in preview,
/// whose printed frames a harness compares.
fn status<'t>(term: &'t mut dyn Term, preview: bool, text: &'t str) -> impl FnOnce() + 't {
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

/// `Confirm-HeldSession`: Enter or `[enter] go on` = go on, Esc/Ctrl+C or `[esc] back` = back;
/// nothing held = go on.
pub(super) fn confirm_held(term: &mut dyn Term, holder: Option<&str>, style: Style) -> Result<bool, UiStop> {
    let Some(h) = holder else { return Ok(true) };
    let mut screen = HeldConfirm {
        warning: sessions::held_warning(h),
        style,
        cursor: Cursor::at(0),
    };
    Ok(crate::ui::screen_loop::run(&mut screen, term)?.unwrap_or(false))
}

/// The held-session warning: the text, then the two buttons as its footer.
struct HeldConfirm {
    warning: String,
    style: Style,
    cursor: Cursor,
}

impl Screen for HeldConfirm {
    type Output = bool;

    fn name(&self) -> &'static str {
        "held"
    }

    fn cursor(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    /// The buttons placed first, the warning wrapped into what is left.
    fn draw(&mut self, (width, height): (usize, usize)) -> Frame {
        let g = theme::glyphs(self.style.ascii);
        let fw = frame::frame_width(width);
        let hints = [Hint::key("enter", "go on", "Enter"), Hint::key("esc", "back", "Escape")];
        let footer = frame::new_hint_footer(&hints, &g, fw, !self.style.color);
        // 2 = the blank lines above and below the warning.
        let room = height.saturating_sub(footer.lines.len() + 2).max(1);
        let mut lines = vec![String::new()];
        lines.extend(
            split_text_lines(&self.warning, fw.saturating_sub(2).max(1), room, self.style.ascii)
                .into_iter()
                .map(|l| format!("  {l}")),
        );
        lines.push(String::new());
        let light = FooterLight {
            hover: frame::footer_hover(&footer, self.cursor.hover),
            selected: Vec::new(),
        };
        let mut map = RowMap::default();
        let lines = frame::complete_frame(&lines, &footer, fw, self.style, &|l: &str, _| l.to_owned(), &light, &mut map);
        Frame { lines, map }
    }

    fn enter(&mut self) -> Option<Flow<bool>> {
        Some(Flow::Done(Some(true)))
    }
}
