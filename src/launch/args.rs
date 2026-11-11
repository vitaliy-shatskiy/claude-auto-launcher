//! The launch modes (`LaunchModes.ps1`) and the claude arguments a launch state produces
//! (`Get-LaunchArgs`). Pure.

use super::options::default_model_unavailable;
use super::{LaunchState, Roster, RowName, UNAVAILABLE_DEFAULT_ADVISOR};

/// One launch mode (`LaunchModes.ps1`): the project screen's action field values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchMode {
    pub name: &'static str,
    pub hotkey: Option<char>,
    pub flag: &'static str,
    pub attaches_newest: bool,
    pub picks_session: bool,
    /// Why the mode cannot launch through crc; empty when it can.
    pub remote_off: &'static str,
}

const CANNOT_ATTACH: &str = "crc births a new session, it cannot attach to an existing one";

pub const LAUNCH_MODES: [LaunchMode; 4] = [
    LaunchMode {
        name: "new",
        hotkey: None,
        flag: "",
        attaches_newest: false,
        picks_session: false,
        remote_off: "",
    },
    LaunchMode {
        name: "continue",
        hotkey: Some('c'),
        flag: "-c",
        attaches_newest: true,
        picks_session: false,
        remote_off: CANNOT_ATTACH,
    },
    LaunchMode {
        name: "resume",
        hotkey: Some('r'),
        flag: "--resume",
        attaches_newest: false,
        picks_session: true,
        remote_off: CANNOT_ATTACH,
    },
    LaunchMode {
        name: "worktree",
        hotkey: Some('t'),
        flag: "-w",
        attaches_newest: false,
        picks_session: false,
        remote_off: "a worktree launch is not routed through crc (unverified interaction)",
    },
];

/// Keys the project screen keeps for itself: a mode hotkey on one would replace it.
pub const PROJECT_SCREEN_OWN_KEYS: [char; 7] = ['/', 'w', 'a', 's', 'd', 'h', 'v'];

/// `Get-LaunchMode`: case-insensitive, like the switch it replaced.
pub fn launch_mode(name: &str) -> Option<&'static LaunchMode> {
    LAUNCH_MODES.iter().find(|m| m.name.eq_ignore_ascii_case(name))
}

/// `Get-LaunchModeTableErrors`: every hotkey clash, one line each.
pub fn launch_mode_table_errors(modes: &[LaunchMode], reserved: &[char]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut seen: Vec<(char, &str)> = Vec::new();
    for m in modes {
        let Some(k) = m.hotkey else { continue };
        if reserved.contains(&k) {
            errors.push(format!(
                "mode '{}': hotkey '{k}' is a project-screen key (/ filter, w/s move, a/d action, h hide, v hidden)",
                m.name
            ));
        }
        if let Some((_, owner)) = seen.iter().find(|(c, _)| *c == k) {
            errors.push(format!("mode '{}': hotkey '{k}' is already mode '{owner}'s", m.name));
        } else {
            seen.push((k, m.name));
        }
    }
    errors
}

/// `Get-LaunchArgs`: a default state yields NOTHING. `available`/`default_model`: the one
/// exception, the CS-231 pin when the account lacks the default model's family; `default_advisor`
/// (the raw `advisorModel`) gets the same pin on the advisor row (CS-1044).
pub fn launch_args(
    roster: &Roster,
    state: &LaunchState,
    resume_id: Option<&str>,
    fork: bool,
    available: Option<&[String]>,
    default_model: &str,
    default_advisor: &str,
) -> Vec<String> {
    let mut a = Vec::new();
    // A hidden model row pinned by `hiddenValues` passes its value as a shown one would.
    let model_row = roster.def(RowName::Model);
    let mut model_arg = model_row
        .and_then(|r| r.args.iter().find(|(k, _)| k.eq_ignore_ascii_case(&state.model)))
        .map(|(_, v)| v.clone());
    if model_arg.is_none()
        && state.model.eq_ignore_ascii_case("default")
        && !default_model.is_empty()
        && default_model_unavailable(default_model, available)
    {
        model_arg = Some(roster.unavailable_default_model().0).filter(|s| !s.is_empty());
    }
    if let Some(m) = model_arg {
        a.extend(["--model".to_owned(), m]);
    }
    if !state.effort.eq_ignore_ascii_case("default") {
        a.extend(["--effort".to_owned(), state.effort.clone()]);
    }
    let advisor = if state.advisor.eq_ignore_ascii_case("default") && default_model_unavailable(default_advisor, available) {
        UNAVAILABLE_DEFAULT_ADVISOR
    } else {
        state.advisor.as_str()
    };
    if ["fable", "opus"].iter().any(|v| v.eq_ignore_ascii_case(advisor)) {
        a.extend(["--advisor".to_owned(), advisor.to_owned()]);
    }
    if !state.permission.eq_ignore_ascii_case("default") {
        let mode = if state.permission.eq_ignore_ascii_case("bypass") {
            "bypassPermissions".to_owned()
        } else {
            state.permission.clone()
        };
        a.extend(["--permission-mode".to_owned(), mode]);
    }
    if state.mode.eq_ignore_ascii_case("safe") {
        a.push("--safe-mode".into());
    }
    if let Some(m) = launch_mode(&state.action).filter(|m| !m.flag.is_empty()) {
        if !m.picks_session {
            a.push(m.flag.into());
        } else if let Some(id) = resume_id.filter(|s| !s.is_empty()) {
            a.extend([m.flag.to_owned(), id.to_owned()]);
            if fork {
                a.push("--fork-session".into());
            }
        }
    }
    a
}
