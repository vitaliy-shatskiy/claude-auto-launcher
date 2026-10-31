//! Maintenance screen, Claude Code versions and the update check (`Maintenance.ps1`,
//! `Invoke-MaintenanceScreen`, `Get-MaintenanceFrame`). PHASE B, worker W4 (PORTING.md).
//! Signatures are final; STUB bodies keep the launch path working.

use crate::config::MaintenanceAction;
use crate::frame::Style;
use crate::types::VersionInfo;
use crate::ui::{Exhausted, Term};

/// STUB (W4): `Get-InstalledClaudeVersion` + `Get-ClaudeInstallInfo` for the header.
pub fn read_version() -> VersionInfo {
    VersionInfo::default()
}

/// STUB (W4): the pre-UI update check - `Get-RemoteClaudeVersion` (30 min cache) and
/// `Invoke-ClaudeUpdate` when newer, then re-read; prints the progress lines. Never in preview,
/// never with `CLAUDE_AUTO_NO_UPDATE=1` (the caller checks both).
pub fn update_before_launch(_version: &mut VersionInfo) {}

/// STUB (W4): `Invoke-MaintenanceScreen`, opened by `u` from the launch screen and returning to it.
pub fn run_screen(_term: &mut dyn Term, _actions: &[MaintenanceAction], _style: Style) -> Result<(), Exhausted> {
    Ok(())
}
