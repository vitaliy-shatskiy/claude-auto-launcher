//! Projects and the project screen (`Projects.ps1`, `Invoke-ProjectScreen`, `Get-ProjectFrame`,
//! the free-path prompt). PHASE B, worker W2 (PORTING.md). Signatures are final; STUB bodies keep
//! the launch path working.

use std::path::Path;

use crate::frame::Style;
use crate::launch::LaunchState;
use crate::ui::{Exhausted, Term};

/// One project row (`Get-ProjectRegistry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub path: String,
    pub slug: String,
    pub slugs: Vec<String>,
    pub worktree: bool,
    /// Unix milliseconds of the newest transcript.
    pub last_activity_ms: Option<i64>,
}

/// What the project screen returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectChoice {
    pub path: String,
    /// A launch-mode name (`launch::LAUNCH_MODES`).
    pub action: String,
    pub slug: String,
    pub slugs: Vec<String>,
    /// enter | hotkey | mouse.
    pub how: String,
}

/// STUB (W2): `Get-ProjectRegistry` - scan, merge slugs, cache (`cache_path` in preview).
pub fn project_registry(_cache_path: Option<&Path>, _profile_roots: &[String]) -> Vec<Project> {
    Vec::new()
}

/// STUB (W2): `Set-LaunchStartProject` - cwd when known or a git dir, else remembered, else none.
/// Returns the source: cwd | remembered | none.
pub fn set_launch_start_project(_state: &mut LaunchState, _cwd: &Path, _projects: &[Project]) -> String {
    "none".into()
}

/// STUB (W2): `Update-LaunchStartSelection` - re-decides the start selection after a tab switch.
pub fn update_start_selection(_state: &mut LaunchState) {}

/// STUB (W2): `Remove-StalePreviewProjectsCache`.
pub fn remove_stale_preview_cache(_dir: &Path) {}

/// STUB (W2): `Invoke-ProjectScreen`. The stub picks the launch directory with action 'new'
/// without reading a key, so the launch screen's result goes straight to the launch.
pub fn run_project_screen(
    _term: &mut dyn Term,
    _projects: &[Project],
    cwd: &Path,
    _style: Style,
) -> Result<Option<ProjectChoice>, Exhausted> {
    Ok(Some(ProjectChoice {
        path: cwd.to_string_lossy().into_owned(),
        action: "new".into(),
        slug: String::new(),
        slugs: Vec::new(),
        how: "stub".into(),
    }))
}
