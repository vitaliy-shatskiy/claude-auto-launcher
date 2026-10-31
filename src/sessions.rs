//! Sessions and the session picker (`Sessions.ps1`, `Invoke-SessionPicker`, `Get-PickerFrame`,
//! `Add-PickerColor`, `Expand-SessionPage`, the frame memo). PHASE B, worker W1 (PORTING.md).
//! Signatures are final; STUB bodies keep the launch path working.

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::frame::Style;
use crate::ui::{Exhausted, Term};

/// What the picker returns: the session to resume, and whether to fork it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedSession {
    pub session_id: String,
    pub fork: bool,
}

/// Everything the picker is opened with.
#[derive(Debug, Clone)]
pub struct PickerRequest<'a> {
    pub sessions_root: PathBuf,
    /// Every slug of the chosen project; empty = the whole account.
    pub project_slugs: &'a [String],
    pub project_name: &'a str,
    pub style: Style,
    pub preview: bool,
}

/// `Get-SessionsRoot`: sharing on = one list for every account; off = the account's own root.
/// STUB (W1): the canonical projects root.
pub fn sessions_root(cfg: &Config, _account: &str) -> PathBuf {
    cfg.canonical().map_or_else(PathBuf::new, |a| Path::new(&a.root).join("projects"))
}

/// STUB (W1): `Invoke-SessionPicker` over paged, snapshotted session lists. `Ok(None)` = Esc.
pub fn run_picker(_term: &mut dyn Term, _req: &PickerRequest<'_>) -> Result<Option<PickedSession>, Exhausted> {
    Ok(None)
}

/// STUB (W1): `Get-ContinueSessionHolder` - the claude.exe holding the project's newest session.
pub fn continue_session_holder(_sessions_dirs: &[PathBuf], _slugs: &[String], _projects_root: &Path) -> Option<String> {
    None
}

/// STUB (W1): `Get-ClaudeSessionHolder` - the claude.exe holding this session id.
pub fn session_holder(_sessions_dirs: &[PathBuf], _session_id: &str) -> Option<String> {
    None
}

/// STUB (W1): `Format-HeldSessionWarning`.
pub fn held_warning(holder: &str) -> String {
    format!("this session is open in another claude ({holder}); continuing forks its transcript")
}
