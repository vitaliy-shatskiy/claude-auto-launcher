//! The UI sequence of one launch: launch screen, project screen, held-session warning, picker.

use std::path::{Path, PathBuf};

use crate::config::{PathCtx, ProjectScreen};
use crate::frame::{self, Style};
use crate::input::console::ConsoleInput;
use crate::input::{Event, KeyPress, is_hotkey, vk};
use crate::launch::LaunchState;
use crate::launch::PROJECT_ACTION;
use crate::launch::RowName;
use crate::launch::launch_mode;
use crate::launch::screen::{LaunchScreen, LaunchView, RowsStore};
use crate::out;
use crate::prefs;
use crate::timing::view;
use crate::types::VersionInfo;
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
    let mut version: VersionInfo = maintenance::read_version();
    if !run.preview && var("CLAUDE_AUTO_NO_UPDATE").as_deref() != Some("1") {
        let _s = tm.scope(&["update check"]);
        maintenance::update_before_launch(&mut version);
    }
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
    let load = tm.begin(&["load", "project"]);
    let cache = run.preview.then(|| {
        let tmp = std::env::temp_dir();
        projects::remove_stale_preview_cache(&tmp);
        tmp.join(format!("claude-auto-projects-preview-{}.json", std::process::id()))
    });
    let account_roots: Vec<String> = run.cfg.accounts.iter().map(|a| a.root.clone()).collect();
    let roots = projects::launcher_profile_roots(&run.ctx.home, var("CLAUDE_CONFIG_DIR").as_deref(), &account_roots);
    let project_list = projects::project_registry(cache.as_deref(), &roots, run.cfg.hide_headless_projects);
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
    let project_source = projects::set_launch_start_project(&mut state, &run.ctx.cwd, &project_list);
    let preselect = state.project.clone();
    tm.end(load);
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
        let mut screen = LaunchScreen::new(&mut roster, state.clone(), &view, &prefs_file, now_ms(), &mut hook).with_rows(&mut rows_store);
        let Some(next) = crate::ui::screen_loop::run(&mut screen, term.as_mut())? else {
            break UiEnd::Cancelled;
        };
        state = next;
        let via_project = state.action == PROJECT_ACTION;
        if opens_project_screen(run.cfg.project_screen, &state.action) {
            let Some(chosen) = projects::run_project_screen(term.as_mut(), &project_list, &run.ctx.cwd, style, &hiding)? else {
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
            start_in_cwd(&mut state, &run.ctx.cwd, &project_list);
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
    if let Some(c) = cache
        && c.exists()
        && std::fs::remove_file(&c).is_err()
    {
        // A stale per-PID cache is swept by the next preview.
    }
    Ok(outcome)
}

/// Config `rows` on disk, re-read each time.
struct ConfigRows<'a> {
    run: &'a Run,
}

impl RowsStore for ConfigRows<'_> {
    fn load(&mut self) -> Option<Vec<RowName>> {
        crate::config::read_config(&self.run.cfg.path, &self.run.ctx).rows
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

/// `Confirm-HeldSession`: Enter = go on, Esc/Ctrl+C = back; nothing held = go on.
pub(super) fn confirm_held(term: &mut dyn Term, holder: Option<&str>, style: Style) -> Result<bool, UiStop> {
    let Some(h) = holder else { return Ok(true) };
    let warning = sessions::held_warning(h);
    let mut dirty = true;
    let tm = term.logger().timings.clone();
    loop {
        let size = term.size();
        if dirty {
            let (w, h) = view::body_size(&tm, size);
            let lines = frame::held_session_frame(&warning, w, h, style.ascii);
            term.paint(&view::with_footer(&tm, lines, "held", size));
            dirty = false;
        }
        match term.wait(size)? {
            Event::Resize => dirty = true,
            Event::Key(k) if tm.is_on() && k.is(vk::F12) => {
                view::show_timings(term, &tm)?;
                dirty = true;
            }
            Event::Key(k) if k.is(vk::ENTER) => return Ok(true),
            Event::Key(k) if k.is(vk::ESCAPE) || k.is_ctrl(vk::C) => return Ok(false),
            Event::Key(_) | Event::Mouse(_) => {}
        }
    }
}
