//! The launch, in the order `claude-auto.ps1` runs it: argv pass-through, config, log, the UI
//! (launch screen -> project screen -> picker), profile, secrets, sharing, hooks, MCP, exec.

use std::collections::BTreeMap;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use serde_json::{Value, json};

use crate::config::{self, Config, PathCtx};
use crate::frame::{self, Style};
use crate::input::console::ConsoleInput;
use crate::input::{Event, KeyPress, is_hotkey, vk};
use crate::launch::screen::{LaunchScreen, LaunchView};
use crate::launch::{LAUNCH_MODES, LaunchState, Roster, launch_args, launch_mode};
use crate::log::{self, Logger};
use crate::log_data;
use crate::out::{self, Fg};
use crate::prefs;
use crate::types::{ChildEnv, VersionInfo};
use crate::ui::{ConsoleTerm, Exhausted, PreviewTerm, Term, parse_preview_keys, parse_preview_size};
use crate::{env, invocation, maintenance, projects, remote, sessions, sys, theme};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// How the UI ended.
#[derive(Debug)]
enum UiEnd {
    Launch(Box<UiOutcome>),
    Cancelled,
    PreviewPickerCancelled,
    PreviewHeldBack,
    /// `CONIN$` would not arm: the keyboard-free fallback, as when the UI cannot run.
    NoConsole,
}

#[derive(Debug)]
struct UiOutcome {
    state: LaunchState,
    resume_id: Option<String>,
    fork: bool,
    view: LaunchView,
    restored: Vec<String>,
    project_source: String,
    preselect: String,
    chosen_how: Option<String>,
}

/// Everything one run shares.
struct Run {
    args: Vec<String>,
    preview: bool,
    started: Instant,
    ctx: PathCtx,
    cfg: Config,
    roster: Roster,
    logger: Logger,
}

/// The whole launcher. Returns the process exit code.
pub fn run(args: Vec<String>) -> i32 {
    if args.len() == 1 && args.first().is_some_and(|a| a == "--launcher-version") {
        out::line(&format!("claude-auto {VERSION}"), Fg::Plain);
        return 0;
    }
    let preview = var("CLAUDE_AUTO_PREVIEW").as_deref() == Some("1");
    let started = Instant::now();
    let ctx = PathCtx::from_env();
    let cfg = config::read_config(&config::config_path(&ctx, var("CLAUDE_AUTO_CONFIG").as_deref()), &ctx);
    let canonical = cfg.canonical().map(|a| (a.key.clone(), a.root.clone()));
    let roster = Roster::new(&cfg.accounts, cfg.remote, canonical.as_ref().map(|(k, _)| k.as_str()));
    for w in &cfg.warnings {
        out::line(&format!("  config: {w}"), Fg::DarkYellow);
    }
    let log_root = PathBuf::from(canonical.map_or_else(String::new, |(_, r)| r)).join("launcher-logs");
    let logger = Logger {
        root: log_root,
        run_id: log::new_run_id(),
        enabled: !preview,
    };
    let run = Run {
        args,
        preview,
        started,
        ctx,
        cfg,
        roster,
        logger,
    };
    launch(&run)
}

fn ms(run: &Run) -> u64 {
    u64::try_from(run.started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn log_start(run: &Run, input_redirected: bool, output_redirected: bool) {
    log::prune(&run.logger.root, 14);
    let env_names = [
        "TERMINAL_EMULATOR",
        "TERM_PROGRAM",
        "WT_SESSION",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_NO_ROAM",
        "CLAUDE_CODE_SSE_PORT",
    ];
    let env_map: serde_json::Map<String, Value> = env_names.iter().map(|n| ((*n).to_owned(), json!(var(n)))).collect();
    run.logger.write(
        "start",
        log_data! {
            "cwd" => run.ctx.cwd.to_string_lossy(),
            "moduleMs" => 0,
            "args" => run.args,
            "inputRedirected" => input_redirected,
            "outputRedirected" => output_redirected,
            "psVersion" => format!("rust {VERSION}"),
            "configDir" => var("CLAUDE_CONFIG_DIR"),
            "env" => env_map,
            "parents" => log::parent_chain(6),
        },
    );
}

#[expect(clippy::too_many_lines, reason = "the launch order reads top to bottom as in claude-auto.ps1")]
fn launch(run: &Run) -> i32 {
    let input_redirected = !std::io::stdin().is_terminal();
    let output_redirected = !std::io::stdout().is_terminal();
    if !run.preview {
        log_start(run, input_redirected, output_redirected);
    }
    let mut use_ui = run.preview || (!input_redirected && run.args.is_empty());
    if use_ui && !run.preview && sys::Handle::open_console("CONOUT$").and_then(|h| sys::window_size(&h)).is_none() {
        use_ui = false;
    }
    let mut choice = run.roster.default_account().to_owned();
    let mut remote_on = run.cfg.remote;
    let mut show_qr = false;
    let mut stop_server = false;
    let mut ui: Option<UiOutcome> = None;
    if use_ui {
        match run_ui(run, input_redirected, output_redirected) {
            Ok(UiEnd::Launch(o)) => ui = Some(*o),
            Ok(UiEnd::Cancelled) => {
                run.logger.write("cancelled", log_data! { "ms" => ms(run) });
                return 0;
            }
            Ok(UiEnd::PreviewPickerCancelled) => {
                preview_banner("--- preview: picker cancelled ---");
                return 0;
            }
            Ok(UiEnd::PreviewHeldBack) => {
                preview_banner("--- preview: held-session warning: back ---");
                return 0;
            }
            Ok(UiEnd::NoConsole) => {
                use_ui = false;
                fallback_prompts(run, &mut choice, &mut remote_on, &mut show_qr, &mut stop_server);
            }
            Err(e) => {
                run.logger.write(
                    "error",
                    log_data! { "where" => "ui", "type" => "Exhausted", "message" => e.to_string() },
                );
                out::err_line(&format!("claude-auto: {e}"));
                return 1;
            }
        }
    } else if !input_redirected && run.args.is_empty() {
        fallback_prompts(run, &mut choice, &mut remote_on, &mut show_qr, &mut stop_server);
    }

    let mut child = ChildEnv::default();
    if let Some(o) = &ui {
        let saved = if run.preview {
            Value::Null
        } else {
            prefs::save_prefs(
                &prefs::prefs_path(&run.ctx, var("CLAUDE_AUTO_PREFS").as_deref()),
                &o.state,
                now_ms(),
                &run.roster,
            )
        };
        if o.state.advisor.eq_ignore_ascii_case("off") {
            child.set("CLAUDE_CODE_DISABLE_ADVISOR_TOOL", "1");
        }
        choice.clone_from(&o.state.account);
        if run.cfg.remote {
            match o.state.remote.as_str() {
                "off" => remote_on = false,
                "on+QR" => show_qr = true,
                "stop server" => {
                    remote_on = false;
                    stop_server = true;
                }
                _ => {}
            }
        }
        if !run.preview {
            run.logger.write(
                "ui",
                log_data! {
                    "ms" => ms(run), "account" => choice, "action" => o.state.action,
                    "remote" => run.cfg.remote.then(|| o.state.remote.clone()),
                    "effort" => o.state.effort, "advisor" => o.state.advisor,
                    "resumeId" => o.resume_id, "fork" => o.fork, "restored" => o.restored, "saved" => saved,
                    "launchedFrom" => run.ctx.cwd.to_string_lossy(), "project" => o.state.project,
                    "projectSource" => o.project_source,
                    "preselect" => json!({ "source": o.project_source, "path": o.preselect }),
                    "chosen" => json!({ "path": o.state.project, "how": o.chosen_how }),
                },
            );
        }
    }

    if let Err(e) = env::set_profile(&run.cfg, &choice, run.preview, &mut child) {
        out::line(&format!("  {e}"), Fg::Red);
        return 1;
    }
    if let Some(o) = ui.as_ref().filter(|_| !run.preview)
        && !o.state.project.is_empty()
        && !env::set_project_directory(&o.state.project)
    {
        run.logger.write(
            "error",
            log_data! { "where" => "Set-ClaudeProjectDirectory", "message" => "not a directory; launch directory kept" },
        );
    }
    if !run.preview {
        let cwd = std::env::current_dir().unwrap_or_else(|_| run.ctx.cwd.clone());
        env::import_project_secrets(&cwd, &run.cfg.secrets_root, &mut child);
    }
    env::repair_shared_profiles(&run.cfg, run.preview);
    if !run.preview {
        env::run_launch_hooks(&run.cfg.launch_hooks);
    }
    let mcp = env::mcp_config_paths(&run.cfg, run.preview);
    let claude = match invocation::resolve_claude() {
        Ok(p) => p,
        Err(msg) => {
            out::line(&format!("  {msg}"), Fg::Red);
            return 1;
        }
    };

    let launch_argv: Vec<String> = match &ui {
        Some(o) => {
            let available = o.view.limits.get(&o.state.account).and_then(|l| l.available_models.as_deref());
            let default_model = o
                .view
                .labels_by_account
                .get(&o.state.account)
                .and_then(|l| l.get("Model"))
                .cloned()
                .unwrap_or_default();
            launch_args(&run.roster, &o.state, o.resume_id.as_deref(), o.fork, available, &default_model)
        }
        None => run.args.clone(),
    };
    let off_modes: Vec<&str> = LAUNCH_MODES
        .iter()
        .filter(|m| !m.remote_off.is_empty() && !m.flag.is_empty() && launch_argv.iter().any(|a| a == m.flag))
        .map(|m| m.remote_off)
        .collect();
    let selects_session = !off_modes.is_empty();
    if selects_session && remote_on {
        out::line(
            &format!("  remote off for this session: {}", off_modes.last().copied().unwrap_or_default()),
            Fg::DarkYellow,
        );
        remote_on = false;
    }
    let argv = invocation::claude_invocation(&launch_argv, &mcp);

    if run.preview {
        preview_summary(&choice, &child, remote_on, show_qr, stop_server, &launch_argv, &claude, &argv);
        return 0;
    }
    if stop_server {
        match remote::stop_companion_server() {
            Ok(s) => out::line(&format!("  companion server: {s}"), Fg::Yellow),
            Err(e) => out::line(&format!("  companion server stop failed: {e}"), Fg::DarkYellow),
        }
    }
    if run.cfg.remote && !remote_on && !selects_session {
        out::line(
            "  remote off for this session - plain claude, not reachable from the phone",
            Fg::DarkGray,
        );
    }
    let no_roam = var("CLAUDE_NO_ROAM").as_deref() == Some("1");
    if run.cfg.remote && !selects_session && !no_roam && remote_on && (use_ui || launch_argv.is_empty()) {
        let req = remote::CrcLaunch {
            argv: &argv,
            account: &choice,
            show_qr,
            child: &child,
        };
        if let Some(code) = remote::launch_via_crc(&req) {
            run.logger
                .write("exit", log_data! { "via" => "crc", "code" => code, "ms" => ms(run) });
            return code;
        }
        out::line(
            "  crc not on PATH - plain session, not reachable from the phone (npm link in the server/ directory)",
            Fg::DarkYellow,
        );
    }
    run.logger.write(
        "decision",
        log_data! {
            "via" => "claude", "exe" => claude.to_string_lossy(), "account" => choice, "useUi" => use_ui,
            "remote" => remote_on, "mcpConfigs" => mcp, "argv" => argv, "ms" => ms(run),
        },
    );
    let code = match invocation::run_child(&claude, &argv, &child) {
        Ok(c) => c,
        Err(e) => {
            out::line(&format!("  claude could not be started: {e}"), Fg::Red);
            1
        }
    };
    run.logger
        .write("exit", log_data! { "via" => "claude", "code" => code, "ms" => ms(run) });
    code
}

fn preview_banner(title: &str) {
    out::line("", Fg::Plain);
    out::line(title, Fg::Cyan);
    out::line(
        "in an interactive session this returns to the launch screen; preview cannot loop because the scripted key list is finite",
        Fg::Plain,
    );
}

#[expect(clippy::too_many_arguments, reason = "one line per preview field")]
fn preview_summary(
    choice: &str,
    child: &ChildEnv,
    remote: bool,
    qr: bool,
    stop: bool,
    launch_argv: &[String],
    claude: &Path,
    argv: &[String],
) {
    let ps_bool = |b: bool| if b { "True" } else { "False" };
    out::line("", Fg::Plain);
    out::line("--- preview, nothing launched ---", Fg::Cyan);
    out::line(&format!("account            : {choice}"), Fg::Plain);
    out::line(
        &format!("CLAUDE_CONFIG_DIR  : {}", child.effective("CLAUDE_CONFIG_DIR").unwrap_or_default()),
        Fg::Plain,
    );
    out::line(
        &format!(
            "remote             : {}   qr={}   stopServer={}",
            ps_bool(remote),
            ps_bool(qr),
            ps_bool(stop)
        ),
        Fg::Plain,
    );
    let advisor = if child.effective("CLAUDE_CODE_DISABLE_ADVISOR_TOOL").as_deref() == Some("1") {
        "disabled (CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1)"
    } else {
        "enabled"
    };
    out::line(&format!("advisor tool       : {advisor}"), Fg::Plain);
    out::line(&format!("launch args        : {}", launch_argv.join(" ")), Fg::Plain);
    out::line(&format!("command            : {} {}", claude.display(), argv.join(" ")), Fg::Plain);
    out::line(&format!("argv count         : {}", argv.len()), Fg::Plain);
}

/// The no-UI prompts: the roster's account letters, then the remote question.
fn fallback_prompts(run: &Run, choice: &mut String, remote: &mut bool, qr: &mut bool, stop: &mut bool) {
    let accounts = &run.cfg.accounts;
    let prefixes = account_prefixes(accounts.iter().map(|a| a.key.as_str()).collect::<Vec<_>>().as_slice());
    let visible: Vec<&config::Account> = accounts.iter().filter(|a| !a.hidden).collect();
    if visible.len() >= 2 {
        let parts: Vec<String> = visible
            .iter()
            .map(|a| {
                let p = prefixes.get(&a.key).cloned().unwrap_or_default();
                let rest: String = a.key.chars().skip(p.chars().count()).collect();
                format!("[{p}]{rest}")
            })
            .collect();
        out::inline(
            &format!("Claude account: {}, Enter = {} : ", parts.join(" / "), run.roster.default_account()),
            Fg::Cyan,
        );
        let answer = read_line();
        *choice = resolve_account_answer(&answer, &prefixes, choice);
    }
    if run.cfg.remote {
        out::inline("Remote: [Enter] on / [n] off / [q] on + QR / [s] stop server + off : ", Fg::Cyan);
        let a = read_line();
        if a.starts_with('n') {
            *remote = false;
        } else if a.starts_with('q') {
            *qr = true;
        } else if a.starts_with('s') {
            *remote = false;
            *stop = true;
        }
    }
}

fn read_line() -> String {
    let mut s = String::new();
    if std::io::stdin().read_line(&mut s).is_err() {
        s.clear();
    }
    s.trim_end_matches(['\r', '\n']).to_owned()
}

/// `Get-AccountKeyPrefixes`: key -> its shortest unique lower-case prefix.
pub fn account_prefixes(keys: &[&str]) -> BTreeMap<String, String> {
    let lower: Vec<String> = keys.iter().map(|k| k.to_lowercase()).collect();
    keys.iter()
        .zip(&lower)
        .map(|(key, k)| {
            let chars: Vec<char> = k.chars().collect();
            let prefix = (1..=chars.len())
                .map(|n| chars.iter().take(n).collect::<String>())
                .find(|p| !lower.iter().any(|o| o != k && o.starts_with(p.as_str())))
                .unwrap_or_else(|| k.clone());
            ((*key).to_owned(), prefix)
        })
        .collect()
}

/// `Resolve-AccountAnswer`: the key in full, else the LONGEST advertised prefix; else the default.
pub fn resolve_account_answer(answer: &str, prefixes: &BTreeMap<String, String>, default: &str) -> String {
    let a = answer.trim().to_lowercase();
    if a.is_empty() {
        return default.to_owned();
    }
    if let Some(k) = prefixes.keys().find(|k| k.to_lowercase() == a) {
        return k.clone();
    }
    prefixes
        .iter()
        .filter(|(_, p)| a.starts_with(p.as_str()))
        .max_by_key(|(_, p)| p.len())
        .map_or_else(|| default.to_owned(), |(k, _)| k.clone())
}

/// The UI: launch screen, project screen, held-session warning, picker - until a launch.
#[expect(clippy::too_many_lines, reason = "the screen sequence reads top to bottom as in claude-auto.ps1")]
fn run_ui(run: &Run, input_redirected: bool, output_redirected: bool) -> Result<UiEnd, Exhausted> {
    let style = Style {
        color: theme::color_supported(var("NO_COLOR").as_deref(), var("TERM").as_deref()),
        ascii: theme::ascii_required(var("CLAUDE_AUTO_ASCII").as_deref(), sys::output_code_page()),
    };
    let mut version: VersionInfo = maintenance::read_version();
    if !run.preview && var("CLAUDE_AUTO_NO_UPDATE").as_deref() != Some("1") {
        maintenance::update_before_launch(&mut version);
    }
    let mut roster = run.roster.clone();
    if let Some(labels) = env::model_family_labels(run.preview) {
        roster.set_model_labels(&labels);
    }
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

    let mut term: Box<dyn Term> = if run.preview {
        let keys = parse_preview_keys(&var("CLAUDE_AUTO_PREVIEW_KEYS").unwrap_or_default()).map_err(|e| {
            out::err_line(&e);
            Exhausted
        })?;
        Box::new(PreviewTerm::new(
            parse_preview_size(var("CLAUDE_AUTO_PREVIEW_SIZE").as_deref()),
            keys,
            false,
        ))
    } else {
        let mouse = var("CLAUDE_AUTO_NO_MOUSE").as_deref() != Some("1");
        let trace = (var("CLAUDE_AUTO_INPUT_TRACE").as_deref() == Some("1")).then(|| trace_path(&run.ctx));
        let Some(input) = ConsoleInput::open(mouse, output_redirected, trace) else {
            return Ok(UiEnd::NoConsole);
        };
        let alt = !output_redirected && !input_redirected;
        Box::new(ConsoleTerm::open(alt, Some(input), run.logger.clone()))
    };

    let prefs_file = prefs::read_prefs(&prefs::prefs_path(&run.ctx, var("CLAUDE_AUTO_PREFS").as_deref()));
    let mut state = roster.new_state();
    let merged = prefs::merge_prefs(&mut state, &prefs_file, &roster, now_ms());
    let cache = run.preview.then(|| {
        let tmp = std::env::temp_dir();
        projects::remove_stale_preview_cache(&tmp);
        tmp.join(format!("claude-auto-projects-preview-{}.json", std::process::id()))
    });
    let roots: Vec<String> = run.cfg.accounts.iter().map(|a| a.root.clone()).collect();
    let project_list = projects::project_registry(cache.as_deref(), &roots);
    let project_source = projects::set_launch_start_project(&mut state, &run.ctx.cwd, &project_list);
    let preselect = state.project.clone();
    let held_dirs: Vec<PathBuf> = run.cfg.accounts.iter().map(|a| Path::new(&a.root).join("sessions")).collect();
    let mut chosen_how: Option<String>;
    let actions = run.cfg.maintenance_actions.clone();

    let outcome = loop {
        let mut hook = |k: &KeyPress, t: &mut dyn Term| -> bool {
            if is_hotkey(k, 'u') {
                if maintenance::run_screen(t, &actions, style).is_err() {
                    return true;
                }
                t.drain();
                return true;
            }
            false
        };
        let mut screen = LaunchScreen::new(&roster, state.clone(), &view, &prefs_file, now_ms(), &mut hook);
        let Some(next) = crate::ui::screen_loop::run(&mut screen, term.as_mut())? else {
            break UiEnd::Cancelled;
        };
        state = next;
        let Some(chosen) = projects::run_project_screen(term.as_mut(), &project_list, &run.ctx.cwd, style)? else {
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
        let mode = launch_mode(&state.action);
        let mut slugs: Vec<String> = state.project_slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
        if slugs.is_empty() && !state.project_slug.is_empty() {
            slugs.push(state.project_slug.clone());
        }
        let sessions_root = sessions::sessions_root(&run.cfg, &state.account);
        if mode.is_some_and(|m| m.attaches_newest) {
            let holder = sessions::continue_session_holder(&held_dirs, &slugs, &sessions_root);
            if confirm_held(term.as_mut(), holder.as_deref(), style)? {
                break launch_end(state, None, false, view, merged.restored, project_source, preselect, chosen_how);
            }
            if run.preview {
                break UiEnd::PreviewHeldBack;
            }
            continue;
        }
        if !mode.is_some_and(|m| m.picks_session) {
            break launch_end(state, None, false, view, merged.restored, project_source, preselect, chosen_how);
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
                sessions::session_holder(&held_dirs, &picked.session_id)
            };
            if confirm_held(term.as_mut(), holder.as_deref(), style)? {
                break launch_end(
                    state,
                    Some(picked.session_id),
                    picked.fork,
                    view,
                    merged.restored,
                    project_source,
                    preselect,
                    chosen_how,
                );
            }
            if run.preview {
                break UiEnd::PreviewHeldBack;
            }
            continue;
        }
        if run.preview {
            break UiEnd::PreviewPickerCancelled;
        }
    };
    if let Some(c) = cache
        && c.exists()
        && std::fs::remove_file(&c).is_err()
    {
        // A stale per-PID cache is swept by the next preview.
    }
    Ok(outcome)
}

#[expect(clippy::too_many_arguments, reason = "the outcome record is assembled once, here")]
fn launch_end(
    state: LaunchState,
    resume_id: Option<String>,
    fork: bool,
    view: LaunchView,
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
fn confirm_held(term: &mut dyn Term, holder: Option<&str>, style: Style) -> Result<bool, Exhausted> {
    let Some(h) = holder else { return Ok(true) };
    let warning = sessions::held_warning(h);
    let mut dirty = true;
    loop {
        let size = term.size();
        if dirty {
            term.paint(&frame::held_session_frame(&warning, size.0, size.1, style.ascii));
            dirty = false;
        }
        match term.wait(size)? {
            Event::Resize => dirty = true,
            Event::Key(k) if k.is(vk::ENTER) => return Ok(true),
            Event::Key(k) if k.is(vk::ESCAPE) || k.is_ctrl(vk::C) => return Ok(false),
            Event::Key(_) | Event::Mouse(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_shortest_unique() {
        let p = account_prefixes(&["work", "main", "mamoru", "ma"]);
        assert_eq!(p.get("work").map(String::as_str), Some("w"));
        assert_eq!(p.get("main").map(String::as_str), Some("mai"));
        assert_eq!(p.get("mamoru").map(String::as_str), Some("mam"));
        assert_eq!(
            p.get("ma").map(String::as_str),
            Some("ma"),
            "a key that prefixes another keeps itself"
        );
    }

    #[test]
    fn answers_resolve_by_full_key_or_longest_prefix() {
        let p = account_prefixes(&["work", "personal"]);
        assert_eq!(resolve_account_answer(" Personal ", &p, "work"), "personal");
        assert_eq!(resolve_account_answer("p", &p, "work"), "personal");
        assert_eq!(resolve_account_answer("", &p, "work"), "work");
        assert_eq!(resolve_account_answer("x", &p, "work"), "work");
    }
}
