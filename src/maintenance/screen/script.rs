//! What an action reports: status text cleaned of control characters, the maintenance script's
//! run and its verdict, GiB in words. Exports `status_text`, `run_script`, `gib`, `Verdict`.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use unicode_general_category::{GeneralCategory, get_general_category};

pub(super) const GIB: u64 = 1024 * 1024 * 1024;

static CSI: LazyLock<Regex> = LazyLock::new(|| crate::theme::re(r"\x1b\[[0-9;?]*[A-Za-z]"));

/// `ConvertTo-StatusText`: CSI sequences and every other control character out (line breaks and
/// tabs kept for the wrapper downstream), trimmed. An escape byte would make every width wrong.
pub(crate) fn status_text(output: &str) -> String {
    let text = CSI.replace_all(output, "");
    let kept: String = text
        .chars()
        .filter(|c| {
            matches!(c, '\r' | '\n' | '\t')
                || !matches!(
                    get_general_category(*c),
                    GeneralCategory::Control
                        | GeneralCategory::Format
                        | GeneralCategory::Surrogate
                        | GeneralCategory::PrivateUse
                        | GeneralCategory::Unassigned
                )
        })
        .collect();
    kept.trim().to_owned()
}

/// What a configured script printed and how it exited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptRun {
    pub(crate) output: String,
    pub(crate) exit_code: i32,
}

/// A maintenance script's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptResult {
    pub(crate) ran: bool,
    pub(crate) ok: bool,
    pub(crate) message: String,
}

/// The existence check runs before any runner; exit 2 is "could not
/// run", never a pass; the last three non-blank lines follow the verdict.
pub(crate) fn run_script(script: &Path, label: &str, runner: &mut dyn FnMut(&Path) -> Result<ScriptRun, String>) -> ScriptResult {
    if !script.exists() {
        return ScriptResult {
            ran: false,
            ok: false,
            message: format!("{label} script not found at {} - nothing was run.", script.display()),
        };
    }
    let r = match runner(script) {
        Ok(r) => r,
        Err(e) => {
            return ScriptResult {
                ran: false,
                ok: false,
                message: format!("{label} could not start: {e}"),
            };
        }
    };
    let verdict = match r.exit_code {
        0 => format!("{label} finished green"),
        2 => format!("{label} COULD NOT RUN (exit 2) - not a pass"),
        c => format!("{label} FAILED (exit {c})"),
    };
    let text = status_text(&r.output);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines.get(lines.len().saturating_sub(3)..).unwrap_or_default().join(" | ");
    ScriptResult {
        ran: true,
        ok: r.exit_code == 0,
        message: if tail.is_empty() { verdict } else { format!("{verdict} - {tail}") },
    }
}

/// Bytes as GiB with one decimal, rounded half away from zero (`'{0:N1}' -f ($b / 1GB)`).
pub(crate) fn gib(bytes: u64) -> String {
    let tenths = (u128::from(bytes) * 10 + u128::from(GIB) / 2) / u128::from(GIB);
    let whole = tenths / 10;
    let grouped = whole
        .to_string()
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(",");
    format!("{grouped}.{}", tenths % 10)
}

/// How an action ended: done or failed, and what it reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub(crate) ok: bool,
    pub(crate) text: String,
}

impl Verdict {
    pub(crate) fn done(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
        }
    }

    pub(crate) fn failed(text: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: text.into(),
        }
    }
}
