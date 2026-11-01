//! Data shared across modules and phase B workers. No phase B worker edits this file.

use std::collections::BTreeMap;

/// One account's last-known limits (`Get-RateLimitSummary`, Env.ps1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LimitRecord {
    pub five_hour: Option<i64>,
    pub seven_day: Option<i64>,
    /// Always rendered: a stale number must say so ("12 min ago", "just now").
    pub age_text: String,
    /// The per-model weekly bucket and its label ('FABLE'), from the same record.
    pub model: Option<i64>,
    pub model_label: Option<String>,
    /// THREE-STATE: `None` = the widget never reported (hide nothing), `Some(empty)` = an answer.
    pub available_models: Option<Vec<String>>,
}

/// Account key -> its limits.
pub type Limits = BTreeMap<String, LimitRecord>;

/// Row name (`Model`, `Effort`, `Advisor`, `Permission`) -> what 'default' resolves to there
/// (`Get-DefaultLabels`, Env.ps1).
pub type DefaultLabels = BTreeMap<String, String>;

/// Installed and newest Claude Code versions for the header (`Get-InstalledClaudeVersion`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionInfo {
    pub installed: Option<String>,
    pub newest: Option<String>,
}

/// Environment changes for the child `claude` only; the launcher's own environment never moves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChildEnv {
    /// `(name, Some(value))` sets, `(name, None)` removes. Applied in order.
    pub vars: Vec<(String, Option<String>)>,
}

impl ChildEnv {
    pub fn set(&mut self, name: &str, value: &str) {
        self.vars.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.vars.push((name.to_owned(), Some(value.to_owned())));
    }

    pub fn remove(&mut self, name: &str) {
        self.vars.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.vars.push((name.to_owned(), None));
    }

    /// Applies the table to a command, in order: the one place a child gets the session env.
    pub fn apply(&self, cmd: &mut std::process::Command) {
        for (k, v) in &self.vars {
            match v {
                Some(val) => cmd.env(k, val),
                None => cmd.env_remove(k),
            };
        }
    }

    /// The value the child will see for `name`: this table first, then the launcher's own.
    pub fn effective(&self, name: &str) -> Option<String> {
        match self.vars.iter().rev().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            Some((_, v)) => v.clone(),
            None => std::env::var(name).ok(),
        }
    }
}
