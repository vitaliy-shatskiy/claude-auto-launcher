//! The no-UI prompts (`claude-auto.ps1` fallback): the account letters, then the remote question.

use std::collections::BTreeMap;

use crate::config;
use crate::out::{self, Fg};

use super::Run;

/// The no-UI prompts: the roster's account letters, then the remote question.
pub(super) fn fallback_prompts(run: &Run, choice: &mut String, remote: &mut bool, qr: &mut bool, stop: &mut bool) {
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
