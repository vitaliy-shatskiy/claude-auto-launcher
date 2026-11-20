//! The launcher's two screenless entries (`launcher_api`): the reads each needs, and nothing that
//! writes, logs or starts a process. A checked `--launcher-run` then goes through `launch()` as a
//! screen's outcome would, except that its choices are not saved.

use std::path::{Path, PathBuf};

use crate::config::{Config, PathCtx};
use crate::frame::Style;
use crate::launch::screen::LaunchView;
use crate::launch::{LaunchState, Roster};
use crate::launcher_api::{self, RunRequest};
use crate::types::VersionInfo;
use crate::{env, maintenance, out, prefs};

use super::screens::label_defaults;
use super::{UiOutcome, now_ms, var};

/// Nothing is drawn; the launch reads only the limits and the resolved defaults of the view.
const NOT_DRAWN: Style = Style { color: false, ascii: true };

/// What a checked `--launcher-run` hands to the launch.
#[derive(Debug, Clone)]
pub(super) struct HeadlessRun {
    state: LaunchState,
    resume_id: Option<String>,
    fork: bool,
    view: LaunchView,
    roster: Roster,
}

impl HeadlessRun {
    pub(super) fn project(&self) -> &str {
        &self.state.project
    }

    pub(super) fn outcome(&self) -> UiOutcome {
        UiOutcome {
            state: self.state.clone(),
            resume_id: self.resume_id.clone(),
            // The client names the account the transcript lives in, so nothing crosses accounts.
            transfer: None,
            fork: self.fork,
            view: self.view.clone(),
            roster: self.roster.clone(),
            restored: Vec::new(),
            project_source: "launcher-run".to_owned(),
            preselect: self.state.project.clone(),
            chosen_how: Some("launcher-run".to_owned()),
        }
    }
}

fn read_prefs(ctx: &PathCtx) -> prefs::Prefs {
    prefs::read_prefs(&prefs::prefs_path(ctx, var("CLAUDE_AUTO_PREFS").as_deref()))
}

/// The model labels from the catalog cache and nothing more: a cold cache would mean a rescan of
/// the claude binary and a cache write, which neither entry may pay for (`true` = that mode).
fn labelled(roster: &Roster) -> Roster {
    let mut roster = roster.clone();
    if let (Some(labels), _) = env::read_model_catalog(true) {
        roster.set_model_labels(&labels);
    }
    roster
}

/// Limits and resolved defaults per account, as the launch screen reads them: files only.
fn view(cfg: &Config, ctx: &PathCtx) -> LaunchView {
    let canonical_root = cfg.canonical().map(|a| a.root.clone()).unwrap_or_default();
    let dirs: Vec<(String, String)> = cfg.accounts.iter().map(|a| (a.key.clone(), a.root.clone())).collect();
    let rate_dir = var("CLAUDE_AUTO_RATE_LIMITS_DIR").map_or_else(|| ctx.home.join(".claude").join("rate-limits"), PathBuf::from);
    let defaults = label_defaults(&canonical_root, &dirs);
    LaunchView {
        limits: env::rate_limit_summary(&cfg.accounts, &rate_dir, now_ms()),
        version: VersionInfo::default(),
        default_model_label: defaults.model,
        default_advisor_label: defaults.advisor,
        labels_by_account: defaults.by_account,
        style: NOT_DRAWN,
    }
}

/// Checks the request; `Err` is the refusal, before anything is written, logged or started.
pub(super) fn prepare_run(req: &RunRequest, ctx: &PathCtx, cfg: &Config, roster: &Roster) -> Result<HeadlessRun, String> {
    let mut req = req.clone();
    req.project = ctx.expand(&req.project);
    let roster = labelled(roster);
    let view = view(cfg, ctx);
    let state = launcher_api::resolve_run(&req, &roster, &view.limits, &read_prefs(ctx))?;
    Ok(HeadlessRun {
        state,
        resume_id: req.resume_id,
        fork: req.fork,
        view,
        roster,
    })
}

/// `--launcher-options`: one line of JSON on stdout. The version is the newest downloaded build,
/// not `claude --version`, which would start a process.
pub(super) fn print_options(ctx: &PathCtx, cfg: &Config, roster: &Roster) -> i32 {
    let roster = labelled(roster);
    let view = view(cfg, ctx);
    let version = maintenance::newest_build();
    let body = launcher_api::options_json(
        &roster,
        &cfg.accounts,
        &view.limits,
        &view.labels_by_account,
        &read_prefs(ctx),
        version.as_deref(),
    );
    out::raw(&format!("{body}\n"));
    0
}

/// The project line a headless preview adds to the summary: the screen's own launches show it on
/// the screen instead.
pub(super) fn preview_project(run: &HeadlessRun) {
    out::line(
        &format!("project            : {}", Path::new(run.project()).display()),
        out::Fg::Plain,
    );
}
