//! The Claude Code install on disk (`Maintenance.ps1`): downloaded builds in version order, the
//! hash cache, install info, the rename swap, pruning and the verified update. Every verdict comes
//! from a SHA-256 of the files, never from the updater's output: `claude update` reports success
//! when Windows refused to overwrite the running image.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

/// Files under this size are hashed fresh every time and never cached.
const HASH_CACHE_MIN_BYTES: u64 = 1024 * 1024;
/// .NET ticks at the Unix epoch: the cache key is shared with the PowerShell launcher.
const EPOCH_TICKS: u128 = 621_355_968_000_000_000;

/// `[version]::TryParse`: 2 to 4 dot-separated non-negative integers; an absent part sorts first.
pub(crate) fn parse_version(s: &str) -> Option<[i64; 4]> {
    let parts: Vec<&str> = s.split('.').collect();
    if !(2..=4).contains(&parts.len()) {
        return None;
    }
    let mut v = [-1i64; 4];
    for (slot, p) in v.iter_mut().zip(&parts) {
        let p = p.trim();
        let digits = p.strip_prefix('+').unwrap_or(p);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = i64::from(digits.parse::<i32>().ok()?);
    }
    Some(v)
}

/// One file of the versions directory.
#[derive(Debug, Clone)]
pub(crate) struct Build {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) len: u64,
    pub(crate) modified: Option<SystemTime>,
}

/// The files of `dir`, or `None` when the directory does not exist (not a native install).
fn list_builds(dir: &Path) -> Option<Vec<Build>> {
    if !dir.is_dir() {
        return None;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Some(Vec::new());
    };
    Some(
        rd.flatten()
            .filter_map(|e| {
                let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
                Some(Build {
                    name: e.file_name().to_string_lossy().into_owned(),
                    path: e.path(),
                    len: meta.len(),
                    modified: meta.modified().ok(),
                })
            })
            .collect(),
    )
}

/// `Get-OrderedClaudeBuilds`: parseable names by version, newest first, then the rest by write
/// time. Version beats file time: a rollback re-downloads an older build with a newer timestamp.
pub(crate) fn order_builds(builds: Vec<Build>) -> Vec<Build> {
    let (mut parsed, mut rest): (Vec<_>, Vec<_>) = builds.into_iter().partition(|b| parse_version(&b.name).is_some());
    parsed.sort_by_key(|b| std::cmp::Reverse(parse_version(&b.name)));
    rest.sort_by_key(|b| std::cmp::Reverse(b.modified));
    parsed.extend(rest);
    parsed
}

/// The newest downloaded build's name, without hashing anything (the header needs only that).
pub(crate) fn newest_build_name(versions: &Path) -> Option<String> {
    order_builds(list_builds(versions)?).into_iter().next().map(|b| b.name)
}

/// The SHA-256 cache keyed by `path|length|write ticks`; last writer wins across launchers.
#[derive(Debug, Clone)]
pub(crate) struct HashCache {
    pub(crate) path: PathBuf,
    /// A preview answers from the cache only and never writes it.
    pub(crate) preview: bool,
}

fn ticks(t: SystemTime) -> Option<u128> {
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_nanos() / 100 + EPOCH_TICKS)
}

fn cache_key(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let full = std::path::absolute(path).ok()?;
    Some(format!("{}|{}|{}", full.display(), meta.len(), ticks(meta.modified().ok()?)?))
}

fn read_cache(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

impl HashCache {
    /// `Get-CachedFileHash`: upper-case hex, or `None` for a missing or unreadable file.
    pub(crate) fn hash(&self, path: &Path) -> Option<String> {
        let meta = std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file)?;
        if meta.len() < HASH_CACHE_MIN_BYTES {
            return super::win::sha256_file(path);
        }
        let key = cache_key(path)?;
        let mut cache = read_cache(&self.path);
        if let Some(Value::String(h)) = cache.get(&key) {
            return Some(h.clone());
        }
        if self.preview {
            return None;
        }
        let hash = super::win::sha256_file(path)?;
        cache.insert(key, Value::String(hash.clone()));
        // Entries whose file is gone or changed are dropped, so retired builds do not pile up.
        let live: Map<String, Value> = cache
            .into_iter()
            .filter(|(k, _)| {
                let parts: Vec<&str> = k.split('|').collect();
                parts.len() == 3 && parts.first().and_then(|p| cache_key(Path::new(p))).as_deref() == Some(k.as_str())
            })
            .collect();
        if let Ok(text) = serde_json::to_string_pretty(&Value::Object(live)) {
            write_cache(&self.path, &text);
        }
        Some(hash)
    }
}

/// A cache write: whole or not at all, and a failure is silent (the cache is an optimisation).
pub(crate) fn write_cache(path: &Path, text: &str) {
    if crate::atomic_file::write_atomic(path, text.as_bytes()).is_err() {
        // The next run recomputes it.
    }
}

/// `Get-ClaudeInstallInfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InstallInfo {
    pub(crate) bin_path: String,
    pub(crate) installed_hash: Option<String>,
    pub(crate) newest_version: Option<String>,
    pub(crate) newest_path: Option<PathBuf>,
    pub(crate) newest_hash: Option<String>,
    pub(crate) matches: bool,
    pub(crate) version_count: usize,
    pub(crate) versions_bytes: u64,
    /// The versions directory EXISTS (even empty): a native install.
    pub(crate) native: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub(crate) ok: bool,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Pruned {
    pub(crate) deleted: Vec<String>,
    pub(crate) freed_bytes: u64,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Update {
    pub(crate) ran: bool,
    pub(crate) output: String,
    pub(crate) matches: bool,
    pub(crate) swapped: bool,
    pub(crate) message: String,
}

const NOT_NATIVE: &str = "not a native install; updates are handled by your installer";

/// Where the install lives: the resolved `claude`, the versions directory and the hash cache.
#[derive(Debug, Clone)]
pub(crate) struct Install {
    pub(crate) bin: PathBuf,
    pub(crate) versions: PathBuf,
    pub(crate) cache: HashCache,
}

impl Install {
    /// The live install: `claude` where PATH resolves it (an npm shim included), else the native
    /// path; the versions directory and cache under the home directory.
    pub(crate) fn from_env(preview: bool) -> Self {
        let home = crate::config::PathCtx::from_env().home;
        let path = std::env::var("PATH").unwrap_or_default();
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let bin = crate::invocation::find_on_path("claude", &path, &pathext)
            .unwrap_or_else(|| home.join(".local").join("bin").join("claude.exe"));
        Self {
            bin,
            versions: home.join(".local").join("share").join("claude").join("versions"),
            cache: HashCache {
                path: home.join(".claude").join("claude-auto-hash-cache.json"),
                preview,
            },
        }
    }

    pub(crate) fn info(&self) -> InstallInfo {
        let installed_hash = self.cache.hash(&self.bin);
        let listed = list_builds(&self.versions);
        let native = listed.is_some();
        let builds = order_builds(listed.unwrap_or_default());
        let newest = builds.first();
        let newest_hash = newest.and_then(|b| self.cache.hash(&b.path));
        InstallInfo {
            bin_path: self.bin.display().to_string(),
            matches: matches!((&installed_hash, &newest_hash), (Some(a), Some(b)) if a == b),
            installed_hash,
            newest_version: newest.map(|b| b.name.clone()),
            newest_path: newest.map(|b| b.path.clone()),
            newest_hash,
            version_count: builds.len(),
            versions_bytes: builds.iter().map(|b| b.len).sum(),
            native,
        }
    }

    /// `Repair-ClaudeBinaryByRename`: copy the newest build beside the binary FIRST, then rename
    /// the running image aside (Windows allows that) and the copy into place; any failure after
    /// the first rename puts the original back.
    pub(crate) fn rename_swap(&self) -> Outcome {
        let fail = |m: String| Outcome { ok: false, message: m };
        let info = self.info();
        let is_exe = self.bin.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        if !is_exe {
            return fail(NOT_NATIVE.into());
        }
        let Some(newest) = info.newest_path.as_deref() else {
            return fail("no downloaded build to install".into());
        };
        if info.matches {
            return Outcome {
                ok: true,
                message: "already on the newest build".into(),
            };
        }
        let new = with_suffix(&self.bin, ".new");
        let old = with_suffix(&self.bin, ".old");
        if new.exists() {
            drop(std::fs::remove_file(&new));
        }
        if let Err(e) = std::fs::copy(newest, &new) {
            drop(std::fs::remove_file(&new));
            return fail(format!(
                "rename swap failed before anything moved, the installed build is untouched: {e}"
            ));
        }
        if old.exists() {
            drop(std::fs::remove_file(&old));
        }
        let mut renamed = false;
        let moved = std::fs::rename(&self.bin, &old).and_then(|()| {
            renamed = true;
            std::fs::rename(&new, &self.bin)
        });
        if let Err(reason) = moved {
            if renamed && !self.bin.exists() {
                drop(std::fs::rename(&old, &self.bin));
            }
            drop(std::fs::remove_file(&new));
            let back = if self.bin.exists() {
                "the working binary was put back".to_owned()
            } else {
                format!("the working binary is at {} and must be renamed back by hand", old.display())
            };
            return fail(format!("rename swap failed: {reason}; {back}"));
        }
        let after = self.info();
        Outcome {
            ok: after.matches,
            message: if after.matches {
                format!("swapped to {}, verified by hash", after.newest_version.unwrap_or_default())
            } else {
                "the swap ran but the hashes still differ".into()
            },
        }
    }

    /// `Remove-OldClaudeVersions`: keeps the `keep` newest builds and the installed one. Only
    /// files named as a version are builds; an unidentified installed build prunes nothing.
    pub(crate) fn prune(&self, keep: usize) -> Pruned {
        let builds: Vec<Build> = list_builds(&self.versions)
            .unwrap_or_default()
            .into_iter()
            .filter(|b| parse_version(&b.name).is_some())
            .collect();
        let ordered = order_builds(builds);
        let installed_hash = self.cache.hash(&self.bin);
        let installed = installed_hash
            .as_deref()
            .and_then(|h| ordered.iter().find(|b| self.cache.hash(&b.path).as_deref() == Some(h)));
        let Some(installed) = installed else {
            return Pruned {
                message: "could not identify the running build - nothing pruned".into(),
                ..Pruned::default()
            };
        };
        let doomed: Vec<&Build> = ordered.iter().skip(keep).filter(|b| b.path != installed.path).collect();
        for d in &doomed {
            drop(std::fs::remove_file(&d.path));
        }
        Pruned {
            deleted: doomed.iter().map(|b| b.name.clone()).collect(),
            freed_bytes: doomed.iter().map(|b| b.len).sum(),
            message: String::new(),
        }
    }

    /// `Invoke-ClaudeUpdate`: runs the updater, ignores what it said, re-hashes, and finishes by
    /// rename when the copy did not land. `updater` returns its output, or why it could not start.
    pub(crate) fn update(&self, updater: impl FnOnce() -> Result<String, String>) -> Update {
        let output = match updater() {
            Ok(o) => o.trim().to_owned(),
            Err(e) => {
                return Update {
                    ran: false,
                    message: format!("the updater could not be started: {e}"),
                    output: e,
                    matches: false,
                    swapped: false,
                };
            }
        };
        let info = self.info();
        if info.matches {
            return Update {
                ran: true,
                output,
                matches: true,
                swapped: false,
                message: format!(
                    "installed build now matches {} (verified by hash)",
                    info.newest_version.unwrap_or_default()
                ),
            };
        }
        let swap = self.rename_swap();
        let after = self.info();
        let message = if after.matches {
            format!(
                "the updater could not overwrite the running claude.exe, so it was installed by rename: now {}, verified by hash. New windows get it; sessions already open keep the old build, and claude.exe.old stays locked until they all exit.",
                after.newest_version.clone().unwrap_or_default()
            )
        } else if !info.native {
            NOT_NATIVE.into()
        } else if info.newest_path.is_none() {
            "nothing to install: no build has been downloaded. Check the connection and press u again.".into()
        } else {
            format!(
                "the updater did not land and the rename swap failed too: {}. Close every Claude Code session - including any a scheduled task starts - and press u again.",
                swap.message
            )
        };
        Update {
            ran: true,
            output,
            matches: after.matches,
            swapped: after.matches && !info.matches,
            message,
        }
    }
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests;
