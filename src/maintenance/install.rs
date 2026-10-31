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
            write_atomic(&self.path, &text);
        }
        Some(hash)
    }
}

/// Write-then-rename, so a concurrent reader never sees half a file. Failure is silent: the
/// cache is an optimisation.
pub(crate) fn write_atomic(path: &Path, text: &str) {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
        drop(std::fs::remove_file(&tmp));
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
mod tests {
    use super::*;

    /// A fresh directory under TEMP, removed on drop.
    pub(crate) struct Tmp(pub(crate) PathBuf);

    impl Tmp {
        pub(crate) fn new(name: &str) -> Self {
            let d = std::env::temp_dir().join(format!("claude-auto-rs-w4-{name}-{}", crate::log::new_run_id()));
            std::fs::create_dir_all(&d).unwrap();
            Self(d)
        }
        pub(crate) fn file(&self, rel: &str, body: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, body).unwrap();
            p
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    fn install(t: &Tmp, bin: &Path, versions: &str) -> Install {
        Install {
            bin: bin.to_path_buf(),
            versions: t.0.join(versions),
            cache: HashCache {
                path: t.0.join("hash-cache.json"),
                preview: false,
            },
        }
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    fn set_mtime(p: &Path, t: SystemTime) {
        std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
    }

    #[test]
    fn version_parse_matches_dotnet_shapes() {
        assert!(parse_version("2.1.230").is_some());
        assert!(parse_version("1.2").is_some());
        assert!(parse_version("1").is_none());
        assert!(parse_version("2.1.280 (Claude Code)").is_none());
        assert!(parse_version("nightly").is_none());
        assert!(parse_version("2.1.264.exe.partial").is_none());
        assert!(parse_version("2.1.10") > parse_version("2.1.9"), "numeric, not lexical");
    }

    #[test]
    fn install_info_finds_newest_and_matches_by_hash() {
        let t = Tmp::new("info");
        t.file("versions/2.1.226", "old build");
        t.file("versions/2.1.230", "new build");
        let bin = t.file("claude.exe", "old build");
        let i = install(&t, &bin, "versions");
        let info = i.info();
        assert_eq!(
            info.newest_version.as_deref(),
            Some("2.1.230"),
            "the newest version directory is found"
        );
        assert!(!info.matches, "an installed binary that is the old build does not match");
        assert_eq!(info.version_count, 2, "every downloaded build is counted");
        std::fs::write(&bin, "new build").unwrap();
        assert!(i.info().matches, "an installed binary equal to the newest build matches");
    }

    #[test]
    fn newest_is_by_version_not_write_time_and_the_swap_installs_it() {
        let t = Tmp::new("order");
        t.file("v/2.1.230", "build 230");
        t.file("v/2.1.226", "build 226");
        let late = SystemTime::now() + std::time::Duration::from_secs(60);
        set_mtime(&t.0.join("v/2.1.226"), late);
        let none = install(&t, &t.0.join("no-such-binary-2.exe"), "v");
        assert_eq!(none.info().newest_version.as_deref(), Some("2.1.230"));
        let bin = t.file("order-bin.exe", "installed old");
        install(&t, &bin, "v").rename_swap();
        assert_eq!(read(&bin), "build 230", "the rename swap installs the highest version");
    }

    #[test]
    fn a_write_time_tie_is_broken_by_version() {
        let t = Tmp::new("tie");
        t.file("v/2.1.100", "build 100");
        t.file("v/2.1.900", "build 900");
        let now = SystemTime::now();
        set_mtime(&t.0.join("v/2.1.100"), now);
        set_mtime(&t.0.join("v/2.1.900"), now);
        assert_eq!(newest_build_name(&t.0.join("v")).as_deref(), Some("2.1.900"));
    }

    #[test]
    fn prune_orders_by_version_and_keeps_the_window() {
        let t = Tmp::new("prune");
        t.file("v/2.1.226", "build 226");
        t.file("v/2.1.230", "build 230");
        t.file("v/2.1.200", "ancient");
        set_mtime(&t.0.join("v/2.1.200"), SystemTime::now() + std::time::Duration::from_secs(60));
        let bin = t.file("installed-newest.exe", "build 230");
        let r = install(&t, &bin, "v").prune(2);
        assert_eq!(
            r.deleted,
            vec!["2.1.200".to_owned()],
            "pruning deletes only what is beyond the keep count"
        );
        assert!(!t.0.join("v/2.1.200").exists(), "the oldest-by-version build is gone");
        assert!(t.0.join("v/2.1.230").exists() && t.0.join("v/2.1.226").exists());
    }

    #[test]
    fn prune_never_deletes_the_installed_build() {
        let t = Tmp::new("prune-inst");
        t.file("v/2.1.226", "build 226");
        t.file("v/2.1.230", "build 230");
        t.file("v/2.1.200", "ancient");
        let bin = t.file("installed-old.exe", "ancient");
        let r = install(&t, &bin, "v").prune(1);
        assert!(
            t.0.join("v/2.1.200").exists(),
            "an installed build outside the keep window survives"
        );
        assert!(!r.deleted.contains(&"2.1.200".to_owned()), "and is not reported as deleted");
    }

    #[test]
    fn prune_leaves_unparseable_names_alone() {
        let t = Tmp::new("prune-names");
        t.file("v/2.1.226", "build 226");
        t.file("v/2.1.230", "build 230");
        t.file("v/nightly", "unparseable");
        let bin = t.file("kept.exe", "build 230");
        install(&t, &bin, "v").prune(2);
        assert!(t.0.join("v/2.1.230").exists() && t.0.join("v/2.1.226").exists());
        assert!(t.0.join("v/nightly").exists(), "the unparseable name is not a build to prune");
    }

    #[test]
    fn prune_never_deletes_files_that_are_not_builds() {
        let t = Tmp::new("prune-safe");
        for v in ["2.1.260", "2.1.261", "2.1.262"] {
            t.file(&format!("v/{v}"), &format!("build {v}"));
        }
        t.file("v/notes.txt", "my notes");
        t.file("v/2.1.264.exe.partial", "half a download");
        let bin = t.file("safe-bin.exe", "build 2.1.262");
        let p = install(&t, &bin, "v").prune(2);
        assert!(t.0.join("v/notes.txt").exists(), "prune never deletes a file that is not a build");
        assert!(
            t.0.join("v/2.1.264.exe.partial").exists(),
            "prune never deletes an in-flight download"
        );
        assert!(
            !t.0.join("v/2.1.260").exists(),
            "prune still removes the build it was asked to remove"
        );
        assert_eq!(p.deleted.len(), 1, "exactly one build was pruned");
    }

    #[test]
    fn prune_fails_closed_when_the_installed_build_is_unidentified() {
        let t = Tmp::new("prune-gap");
        t.file("v/2.1.270", "build 2.1.270");
        t.file("v/2.1.271", "build 2.1.271");
        let g = install(&t, &t.0.join("not-there.exe"), "v").prune(1);
        assert!(g.deleted.is_empty(), "with no installed binary to identify, prune deletes nothing");
        assert!(t.0.join("v/2.1.270").exists());
        let stray = t.file("stray.exe", "a build this directory has never seen");
        assert!(
            install(&t, &stray, "v").prune(1).deleted.is_empty(),
            "a binary matching no build stops the prune"
        );
    }

    #[test]
    fn rename_swap_installs_keeps_old_and_is_idempotent() {
        let t = Tmp::new("swap");
        t.file("v/2.1.226", "old build");
        t.file("v/2.1.230", "new build");
        let bin = t.file("claude.exe", "old build");
        let i = install(&t, &bin, "v");
        let r = i.rename_swap();
        assert!(r.ok, "the rename swap reports success");
        assert_eq!(read(&bin), "new build");
        assert_eq!(read(&with_suffix(&bin, ".old")), "old build", "the previous binary is kept as .old");
        assert!(
            i.rename_swap().ok,
            "a second swap on an already-current binary is a no-op that still reports success"
        );
        assert_eq!(read(&bin), "new build");
    }

    #[test]
    fn a_missing_versions_directory_is_not_a_match_and_not_native() {
        let t = Tmp::new("nodir");
        let bin = t.file("claude.exe", "x");
        let info = install(&t, &bin, "does-not-exist").info();
        assert_eq!(info.newest_version, None);
        assert!(!info.matches && !info.native);
    }

    #[test]
    fn a_lying_updater_still_ends_with_the_newest_build() {
        let t = Tmp::new("upd-lie");
        t.file("v/2.1.240", "build 240");
        let bin = t.file("update-bin.exe", "build 200");
        let r = install(&t, &bin, "v").update(|| Ok("Successfully updated from 2.1.200 to version 2.1.240".into()));
        assert_eq!(read(&bin), "build 240");
        assert!(r.matches && r.swapped, "the re-hash decides and the swap is reported");
        assert!(r.message.contains("2.1.240"));
    }

    #[test]
    fn an_updater_that_landed_needs_no_swap() {
        let t = Tmp::new("upd-ok");
        t.file("v/2.1.240", "build 240");
        let bin = t.file("update-bin.exe", "build 200");
        let b2 = bin.clone();
        let r = install(&t, &bin, "v").update(move || {
            std::fs::write(&b2, "build 240").unwrap();
            Ok("Successfully updated".into())
        });
        assert!(r.matches && !r.swapped);
        assert!(!with_suffix(&bin, ".old").exists(), "no .old file when no swap was needed");
    }

    #[test]
    fn update_messages_name_their_own_cause() {
        let t = Tmp::new("upd-msg");
        std::fs::create_dir_all(t.0.join("empty")).unwrap();
        let bin = t.file("update-bin.exe", "build 200");
        let r = install(&t, &bin, "empty").update(|| Ok("nothing to do".into()));
        assert!(!r.matches && r.message.contains("no build has been downloaded"));
        assert!(install(&t, &bin, "empty").info().native, "an existing empty versions dir is native");
        let r = install(&t, &bin, "never-created").update(|| Ok("Successfully updated".into()));
        assert!(!r.matches);
        assert_eq!(r.message, NOT_NATIVE);
        let r = install(&t, &bin, "empty").update(|| Err("claude: command not found".into()));
        assert!(
            !r.ran && r.message.contains("command not found"),
            "an updater that cannot start is a status line"
        );
    }

    #[test]
    fn hash_cache_serves_exact_answers_only() {
        let t = Tmp::new("hash");
        let cache = HashCache {
            path: t.0.join("cache.json"),
            preview: false,
        };
        let big = t.0.join("big.bin");
        std::fs::write(&big, vec![0u8; 2 * 1024 * 1024]).unwrap();
        let h1 = cache.hash(&big).unwrap();
        assert_eq!(cache.hash(&big).as_deref(), Some(h1.as_str()));
        assert!(cache.path.exists(), "a large file populates the cache file");
        let mut changed = vec![0u8; 2 * 1024 * 1024];
        changed[0] = 7;
        std::fs::write(&big, &changed).unwrap();
        let h2 = cache.hash(&big).unwrap();
        assert_ne!(h1, h2, "changed bytes at the same length are not served from the cache");
        assert_eq!(Some(h2), super::super::win::sha256_file(&big));
        let small = t.file("small.txt", "one");
        let s1 = cache.hash(&small);
        std::fs::write(&small, "two").unwrap();
        assert_ne!(s1, cache.hash(&small), "a small file is hashed fresh every time");
        assert!(!read(&cache.path).contains("small.txt"), "a small file never enters the cache");
        assert_eq!(cache.hash(&t.0.join("never-existed.bin")), None);
    }

    #[test]
    fn hash_cache_prunes_entries_of_gone_files() {
        let t = Tmp::new("hash-prune");
        let cache = HashCache {
            path: t.0.join("cache.json"),
            preview: false,
        };
        let (big, gone) = (t.0.join("big.bin"), t.0.join("gone.bin"));
        std::fs::write(&big, vec![0u8; 2 * 1024 * 1024]).unwrap();
        std::fs::write(&gone, vec![0u8; 2 * 1024 * 1024]).unwrap();
        cache.hash(&big);
        cache.hash(&gone);
        std::fs::remove_file(&gone).unwrap();
        let mut miss = vec![0u8; 2 * 1024 * 1024];
        miss[1] = 9;
        std::fs::write(&big, &miss).unwrap();
        cache.hash(&big);
        let text = read(&cache.path);
        assert!(!text.contains("gone.bin") && text.contains("big.bin"));
    }

    #[test]
    fn a_locked_file_answers_none_and_is_never_cached() {
        use std::os::windows::fs::OpenOptionsExt as _;
        let t = Tmp::new("hash-lock");
        let cache = HashCache {
            path: t.0.join("cache.json"),
            preview: false,
        };
        let small = t.file("locked-small.txt", "held");
        let big = t.0.join("locked-big.bin");
        std::fs::write(&big, vec![0u8; 2 * 1024 * 1024]).unwrap();
        let hold = |p: &Path| std::fs::OpenOptions::new().read(true).share_mode(0).open(p).unwrap();
        let (h1, h2) = (hold(&small), hold(&big));
        assert_eq!(cache.hash(&small), None);
        assert_eq!(cache.hash(&big), None);
        drop((h1, h2));
        assert!(!cache.path.exists() || !read(&cache.path).contains("locked-big"));
    }

    #[test]
    fn a_preview_answers_from_the_cache_and_writes_nothing() {
        let t = Tmp::new("hash-preview");
        let p = t.0.join("preview.bin");
        std::fs::write(&p, vec![0u8; 2 * 1024 * 1024]).unwrap();
        let path = t.0.join("cache.json");
        let preview = HashCache {
            path: path.clone(),
            preview: true,
        };
        assert_eq!(preview.hash(&p), None, "a cold large file is unknown in a preview");
        assert!(!path.exists(), "and no cache file is written");
        let warm = HashCache { path, preview: false }.hash(&p);
        assert_eq!(preview.hash(&p), warm, "a warm entry is still served during a preview");
    }

    #[test]
    fn a_failed_swap_leaves_the_working_binary() {
        use std::os::windows::fs::OpenOptionsExt as _;
        let t = Tmp::new("swap-fail");
        let src = t.file("v/2.1.280", "build 280");
        let bin = t.file("swap-bin.exe", "build 200");
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&src).unwrap();
        let r = install(&t, &bin, "v").rename_swap();
        drop(held);
        assert!(!r.ok && r.message.contains("swap"));
        assert_eq!(read(&bin), "build 200", "the ORIGINAL binary, not a partial copy");
    }

    #[test]
    fn the_swap_refuses_a_shim() {
        let t = Tmp::new("swap-shim");
        t.file("v/2.1.280", "build 280");
        let shim = t.file("claude.cmd", "@echo off & node claude.js %*");
        let r = install(&t, &shim, "v").rename_swap();
        assert!(!r.ok && r.message.contains("not a native install"));
        assert_eq!(read(&shim), "@echo off & node claude.js %*");
        assert!(!with_suffix(&shim, ".old").exists());
    }
}
