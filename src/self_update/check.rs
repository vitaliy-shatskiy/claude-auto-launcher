//! The daily release check: the newest GitHub release of the update repo, parsed and cached for 24
//! hours in `claude-auto-self-update.json`, with what the last swap left. Exports `Version`,
//! `Release`, `State`, `latest`, `newer`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Net, Repo};

/// The two assets `.github/workflows/release.yml` publishes; no other asset is ever fetched.
pub(crate) const EXE_ASSET: &str = "claude-auto.exe";
pub(crate) const SHA_ASSET: &str = "claude-auto.exe.sha256";
pub(super) const DAY_MS: i64 = 24 * 60 * 60_000;
const API_CAP: usize = 1024 * 1024;

/// A release version, `major.minor.patch` and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct Version(pub(crate) [u64; 3]);

impl Version {
    /// `1.2.3` or `v1.2.3` exactly: a pre-release or build suffix is no release to offer.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut out = [0u64; 3];
        let mut parts = s.split('.');
        for slot in &mut out {
            let p = parts.next()?;
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || (p.len() > 1 && p.starts_with('0')) {
                return None;
            }
            *slot = p.parse().ok()?;
        }
        parts.next().is_none().then_some(Self(out))
    }

    /// This build's version; `None` for a pre-release build, which is never offered anything.
    pub(crate) fn current() -> Option<Self> {
        Self::parse(env!("CARGO_PKG_VERSION"))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b, c] = self.0;
        write!(f, "{a}.{b}.{c}")
    }
}

/// A release worth offering: its version and the two asset URLs, both under the repo's own
/// `https://github.com/<repo>/releases/download/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Release {
    pub(crate) version: Version,
    pub(crate) exe_url: String,
    pub(crate) sha_url: String,
}

/// The state file: the last check and what the last swap left in the exe's place and as `.old`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct State {
    pub(crate) checked_at_ms: i64,
    pub(crate) repo: String,
    pub(crate) latest: Option<Release>,
    pub(crate) installed: Option<Version>,
    pub(crate) previous: Option<Version>,
    /// The build the user rolled back from: `auto` never installs it again, only a newer one.
    pub(crate) rolled_back_from: Option<Version>,
    /// The last failed background update: `auto` waits a day before downloading that release again.
    pub(crate) failed: Option<Failed>,
}

/// A background update that failed, when, and why (the launch screen line repeats it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Failed {
    pub(crate) version: Version,
    pub(crate) at_ms: i64,
    pub(crate) why: String,
}

impl State {
    /// Missing or unreadable reads as never checked.
    pub(crate) fn read(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// A failed write only costs a check on the next start.
    pub(crate) fn write(&self, path: &Path) {
        if let Ok(text) = serde_json::to_vec(self)
            && crate::atomic_file::write_atomic(path, &text).is_err()
        {
            // The next start checks again.
        }
    }
}

/// The release JSON of `GET /repos/<repo>/releases/latest`, or `None`: a draft, a pre-release, a
/// tag that is no plain version, or an asset missing or served from anywhere else.
pub(crate) fn parse_release(body: &[u8], repo: &Repo) -> Option<Release> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return None;
    }
    let version = Version::parse(v.get("tag_name")?.as_str()?)?;
    let prefix = repo.download_prefix();
    let url_of = |name: &str| {
        v.get("assets")?
            .as_array()?
            .iter()
            .find(|a| a.get("name").and_then(Value::as_str) == Some(name))?
            .get("browser_download_url")?
            .as_str()
            .filter(|u| u.to_ascii_lowercase().starts_with(&prefix))
            .map(str::to_owned)
    };
    Some(Release {
        version,
        exe_url: url_of(EXE_ASSET)?,
        sha_url: url_of(SHA_ASSET)?,
    })
}

/// The newest release, from the state file when it was checked within 24 hours for this repo,
/// else from the network. Any failure (offline, 403/429, a malformed answer) is `None`, remembered
/// like an answer so the next request waits a day.
pub(crate) fn latest(path: &Path, repo: &Repo, now_ms: i64, net: &dyn Net) -> Option<Release> {
    let mut state = State::read(path);
    let age = now_ms.saturating_sub(state.checked_at_ms);
    if state.checked_at_ms != 0 && state.repo == repo.as_str() && (0..DAY_MS).contains(&age) {
        return state.latest;
    }
    state.latest = net.get(&repo.api_url(), API_CAP).ok().and_then(|b| parse_release(&b, repo));
    state.checked_at_ms = now_ms;
    repo.as_str().clone_into(&mut state.repo);
    state.write(path);
    state.latest
}

/// `release` when it is strictly newer than `current`; never a downgrade, never to a pre-release
/// build.
pub(crate) fn newer(release: Option<Release>, current: Option<Version>) -> Option<Release> {
    release.filter(|r| current.is_some_and(|c| r.version > c))
}

#[cfg(test)]
mod tests;
