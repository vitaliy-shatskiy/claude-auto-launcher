//! The launch, in the order `claude-auto.ps1` runs it: argv pass-through, config, log, the UI
//! (launch screen -> project screen -> picker), profile, secrets, sharing, hooks, MCP, exec.

mod headless;
mod plugins;
mod prompts;
mod report;
mod screens;
mod waits;

use std::io::IsTerminal as _;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

use serde_json::{Value, json};

use crate::config::{self, Config, PathCtx};
use crate::launch::screen::LaunchView;
use crate::launch::{LAUNCH_MODES, LaunchState, Roster, launch_args};
use crate::launcher_api;
use crate::log::{self, Logger};
use crate::log_data;
use crate::out::{self, Fg};
use crate::prefs;
#[cfg(feature = "remote")]
use crate::remote;
use crate::timing::{self, SpanId, Timings};
use crate::types::ChildEnv;
use crate::{env, invocation, sys};

use plugins::{post_launch, pre_launch};
use prompts::fallback_prompts;
pub use prompts::{account_prefixes, resolve_account_answer};
use report::{log_start, preview_banner, preview_summary, print_timings};
use screens::run_ui;

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
    /// The picked session lives in another account's store: copied before claude starts.
    transfer: Option<crate::sessions::cross_account::Transfer>,
    fork: bool,
    view: LaunchView,
    /// The rows as the launch screen left them (edited there this run).
    roster: Roster,
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
    /// Debug mode's root span: process start to the spawn of claude.
    process: SpanId,
    /// `--launcher-run`, checked: this stands in for the screen's outcome.
    headless: Option<headless::HeadlessRun>,
    /// The described plugins (config `plugins`).
    plugins: crate::plugins::Plugins,
}

/// The whole launcher. Returns the process exit code.
pub fn run(args: Vec<String>) -> i32 {
    if args.len() == 1 && args.first().is_some_and(|a| a == "--launcher-version") {
        out::line(&format!("claude-auto {VERSION}"), Fg::Plain);
        return 0;
    }
    // The screenless entries, only as the first argument: a pass-through line that happens to carry
    // the words still reaches claude. A bad line is refused here, before the config is read.
    let wants_options = args.first().is_some_and(|a| a == launcher_api::OPTIONS_FLAG);
    if wants_options && args.len() > 1 {
        return refuse(&format!("{} takes no arguments", launcher_api::OPTIONS_FLAG));
    }
    let run_request = match args.split_first() {
        Some((flag, rest)) if flag == launcher_api::RUN_FLAG => match launcher_api::parse_run_args(rest) {
            Ok(req) => Some(req),
            Err(e) => return refuse(&e),
        },
        _ => None,
    };
    let preview = var("CLAUDE_AUTO_PREVIEW").as_deref() == Some("1");
    let started = Instant::now();
    let debug = timing::debug_mode(&args, var("CLAUDE_AUTO_DEBUG").as_deref());
    let args = if timing::is_debug_flag(&args) { Vec::new() } else { args };
    let timings = Timings::new(debug, started);
    let process = timings.begin_at(&["process"], started);
    let ctx = PathCtx::from_env();
    let cfg = {
        let _s = timings.scope(&["config"]);
        config::read_config(&config::config_path(&ctx, var("CLAUDE_AUTO_CONFIG").as_deref()), &ctx)
    };
    let canonical = cfg.canonical().map(|a| (a.key.clone(), a.root.clone()));
    let mut roster = Roster::new(&cfg.accounts, cfg.remote, canonical.as_ref().map(|(k, _)| k.as_str()));
    if let Some(order) = &cfg.model_order {
        roster.apply_model_order(order);
    }
    if let Some(rows) = &cfg.rows {
        roster.apply_rows(rows);
    }
    roster.set_hidden_values(&cfg.hidden_values);
    if cfg.project_screen == config::ProjectScreen::OnDemand {
        roster.add_action_row();
    }
    if !wants_options {
        for w in &cfg.warnings {
            out::line(&format!("  config: {w}"), Fg::DarkYellow);
        }
    }
    // A refused `--launcher-run` starts no plugin and writes no plugin cache: its built-in fields
    // are checked first, its `pluginRows` by `prepare_run` once the plugins are described.
    if let Some(req) = &run_request
        && let Err(e) = headless::check_builtin(req, &ctx, &cfg, &roster)
    {
        return refuse(&e);
    }
    let (plugins, plugin_warnings) = plugins::load(&cfg, &timings, &mut roster, !wants_options && !preview);
    if wants_options {
        // stderr: stdout is the one line of JSON the client parses.
        for w in &cfg.warnings {
            out::err_line(&format!("  config: {w}"));
        }
        for w in &plugin_warnings {
            out::err_line(&format!("  {w}"));
        }
        return headless::print_options(&ctx, &cfg, &roster);
    }
    for w in &plugin_warnings {
        out::line(&format!("  {w}"), Fg::DarkYellow);
    }
    let headless = match run_request.map(|req| headless::prepare_run(&req, &ctx, &cfg, &roster)) {
        Some(Ok(checked)) => Some(checked),
        Some(Err(e)) => return refuse(&e),
        None => None,
    };
    let log_root = PathBuf::from(canonical.map_or_else(String::new, |(_, r)| r)).join("launcher-logs");
    if let Some(days) = timing::summary::parse_args(&args) {
        return print_timings(&log_root, days);
    }
    let logger = Logger {
        root: log_root,
        run_id: log::new_run_id(),
        enabled: !preview,
        timings,
    };
    let run = Run {
        args,
        preview,
        started,
        ctx,
        cfg,
        roster,
        logger,
        process,
        headless,
        plugins,
    };
    let code = launch(&run);
    // Every path but the spawn ends here; the spawn path ended and wrote these already.
    run.logger.timings.end(run.process);
    run.logger.write_timings();
    code
}

/// A request a screenless entry cannot take: exit 2 = "not started", said on stderr.
fn refuse(why: &str) -> i32 {
    out::err_line(&format!("claude-auto: {why}"));
    2
}

fn ms(run: &Run) -> u64 {
    u64::try_from(run.started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The error log's `type`: `Exhausted` stays the name earlier logs used for a finished preview.
fn ui_stop_type(e: crate::ui::UiStop) -> &'static str {
    match e {
        crate::ui::UiStop::KeysExhausted => "Exhausted",
        crate::ui::UiStop::ConsoleLost => "ConsoleLost",
    }
}

#[expect(clippy::too_many_lines, reason = "the launch order reads top to bottom as in claude-auto.ps1")]
fn launch(run: &Run) -> i32 {
    let input_redirected = !std::io::stdin().is_terminal();
    let output_redirected = !std::io::stdout().is_terminal();
    if !run.preview {
        let _s = run.logger.timings.scope(&["start log"]);
        log_start(run, input_redirected, output_redirected);
    }
    // `--launcher-run` arrives with the screen's answer already made: no screen, no prompts.
    let screenless = run.headless.is_some();
    let mut use_ui = !screenless && (run.preview || (!input_redirected && run.args.is_empty()));
    if use_ui && !run.preview && sys::Handle::open_console("CONOUT$").and_then(|h| sys::window_size(&h)).is_none() {
        use_ui = false;
    }
    let mut choice = run.roster.default_account().to_owned();
    let mut remote_on = run.cfg.remote;
    let mut show_qr = false;
    let mut stop_server = false;
    let mut ui: Option<UiOutcome> = run.headless.as_ref().map(headless::HeadlessRun::outcome);
    // The launcher's own release check: a thread, never in the way of the first frame. It lives
    // until this function returns, after claude exits: a download that outlasts the screens still
    // lands, and the drop keeps a swap from starting once the launcher is quitting.
    let self_update = crate::self_update::Local::from_env(
        run.cfg.self_update,
        &run.cfg.update_repo,
        &run.ctx.home,
        if run.preview {
            crate::self_update::Start::Preview
        } else {
            crate::self_update::Start::Launch
        },
    );
    let launcher = std::cell::RefCell::new(crate::self_update::Watch::start(
        if use_ui { self_update.clone() } else { self_update.off() },
        now_ms(),
    ));
    if use_ui {
        let ui_span = run.logger.timings.begin(&["ui"]);
        let ended = run_ui(run, input_redirected, output_redirected, (&self_update, &launcher));
        run.logger.timings.end(ui_span);
        match ended {
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
                    log_data! { "where" => "ui", "type" => ui_stop_type(e), "message" => e.to_string() },
                );
                out::err_line(&format!("claude-auto: {e}"));
                return 1;
            }
        }
    } else if !screenless && !input_redirected && run.args.is_empty() {
        fallback_prompts(run, &mut choice, &mut remote_on, &mut show_qr, &mut stop_server);
    }

    let mut child = ChildEnv::default();
    if let Some(o) = &ui {
        // A screenless launch's choices last for that launch: saved, they would quietly become the
        // next screen launch's remembered rows.
        let saved = if run.preview || screenless {
            Value::Null
        } else {
            let (saved, written) = prefs::save_prefs(
                &prefs::prefs_path(&run.ctx, var("CLAUDE_AUTO_PREFS").as_deref()),
                &o.state,
                now_ms(),
                &o.roster,
            );
            if let Err(e) = written {
                run.logger
                    .write("error", log_data! { "where" => "prefs save", "message" => e.to_string() });
            }
            saved
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

    let tm = &run.logger.timings;
    let prep = tm.begin(&["prepare"]);
    if let Err(e) = env::set_profile(&run.cfg, &choice, run.preview, &mut child) {
        out::line(&format!("  {e}"), Fg::Red);
        return 1;
    }
    // After the profile, before claude scans its store for the id.
    if let Some(t) = ui.as_ref().and_then(|o| o.transfer.as_ref()) {
        let target = crate::sessions::sessions_root(&run.cfg, &choice);
        match crate::sessions::cross_account::transfer_before_launch(t, &target, &choice, run.preview) {
            Ok(line) => out::line(&line, if run.preview { Fg::Plain } else { Fg::DarkGray }),
            Err(e) => {
                out::line(&e, Fg::Red);
                return 1;
            }
        }
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
        let _s = tm.scope(&["secrets"]);
        let cwd = std::env::current_dir().unwrap_or_else(|_| run.ctx.cwd.clone());
        env::import_project_secrets(&cwd, &run.cfg, &mut child);
    }
    {
        let _s = tm.scope(&["sharing"]);
        env::repair_shared_profiles(&run.cfg, run.preview);
    }
    if !run.preview {
        let _s = tm.scope(&["hooks"]);
        env::run_launch_hooks_with_env(&run.cfg.launch_hooks, &child, tm);
    }
    let plugin_args = match ui.as_ref().filter(|_| !run.preview) {
        Some(o) => {
            let _s = tm.scope(&["plugins", "preLaunch"]);
            match pre_launch(run, o, &mut child) {
                Ok(args) => args,
                Err(code) => return code,
            }
        }
        None => Vec::new(),
    };
    let mcp = {
        let _s = tm.scope(&["mcp"]);
        env::mcp_config_paths(&run.cfg, run.preview)
    };
    let resolved = {
        let _s = tm.scope(&["resolve claude"]);
        invocation::resolve_claude()
    };
    tm.end(prep);
    let claude = match resolved {
        Ok(p) => p,
        Err(msg) => {
            out::line(&format!("  {msg}"), Fg::Red);
            return 1;
        }
    };

    let mut launch_argv: Vec<String> = match &ui {
        Some(o) => {
            let available = o.view.limits.get(&o.state.account).and_then(|l| l.available_models.as_deref());
            let resolved = |row: &str| {
                o.view
                    .labels_by_account
                    .get(&o.state.account)
                    .and_then(|l| l.get(row))
                    .cloned()
                    .unwrap_or_default()
            };
            let (default_model, default_advisor) = (resolved("Model"), resolved("Advisor"));
            launch_args(
                &o.roster,
                &o.state,
                o.resume_id.as_deref(),
                o.fork,
                available,
                &default_model,
                &default_advisor,
            )
        }
        None => run.args.clone(),
    };
    launch_argv.extend(plugin_args);
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
        if let Some(h) = &run.headless {
            headless::preview_project(h);
        }
        preview_summary(&choice, &child, remote_on, show_qr, stop_server, &launch_argv, &claude, &argv);
        return 0;
    }
    #[cfg(feature = "remote")]
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
    #[cfg(feature = "remote")]
    if run.cfg.remote
        && !selects_session
        && var("CLAUDE_NO_ROAM").as_deref() != Some("1")
        && remote_on
        && (use_ui || screenless || launch_argv.is_empty())
    {
        let req = remote::CrcLaunch {
            argv: &argv,
            account: &choice,
            show_qr,
            child: &child,
        };
        if let Some(exe) = remote::crc_path() {
            run.logger.write(
                "decision",
                log_data! {
                    "via" => "crc", "exe" => exe.to_string_lossy(), "account" => choice, "useUi" => use_ui,
                    "remote" => remote_on, "mcpConfigs" => mcp, "argv" => argv, "ms" => ms(run),
                },
            );
        }
        tm.end(run.process);
        run.logger.write_timings();
        if let Some(code) = remote::launch_via_crc(&req) {
            run.logger
                .write("exit", log_data! { "via" => "crc", "code" => code, "ms" => ms(run) });
            post_launch(run, ui.as_ref(), code);
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
    let code = match invocation::run_child(&claude, &argv, &child, Some((&run.logger, run.process))) {
        Ok(c) => c,
        Err(e) => {
            out::line(&format!("  claude could not be started: {e}"), Fg::Red);
            1
        }
    };
    run.logger
        .write("exit", log_data! { "via" => "claude", "code" => code, "ms" => ms(run) });
    post_launch(run, ui.as_ref(), code);
    code
}

#[cfg(test)]
mod tests;
