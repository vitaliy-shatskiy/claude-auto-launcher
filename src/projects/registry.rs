//! `Get-ProjectRegistry`: every slug directory under the projects root, resolved from its newest
//! transcript's cwd, merged one row per real directory, ordered by last interactive activity. The
//! cache holds only derived data (path, git-root answer, headless verdict), so a lost or corrupt
//! write costs one recompute; written atomically, last writer wins.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{Map, Value, json};

use super::paths::{DriveCache, is_scratch_key, on_network, path_present, project_key, scratch_key, scratch_keys};
use super::{Phase, Project, Registry};

/// What one build reads, all injected so a suite owns its tree.
#[derive(Debug, Clone)]
pub(crate) struct RegistryInput<'a> {
    pub projects_root: &'a Path,
    pub cache_path: &'a Path,
    pub temp_roots: &'a [String],
    pub profile_roots: &'a [String],
    /// `hideHeadlessProjects`: a folder only headless runs used is hidden even as a git root.
    pub hide_headless: bool,
}

/// One transcript file of a slug folder.
#[derive(Debug, Clone)]
struct Transcript {
    path: PathBuf,
    name: String,
    modified: SystemTime,
}

/// .NET ticks (100 ns since 0001-01-01 UTC): the PowerShell launcher keys the shared cache on them.
fn ticks(t: SystemTime) -> i64 {
    const EPOCH_TICKS: i64 = 621_355_968_000_000_000;
    let hundreds = |d: Duration| i64::try_from(d.as_nanos() / 100).unwrap_or(i64::MAX);
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => EPOCH_TICKS.saturating_add(hundreds(d)),
        Err(e) => EPOCH_TICKS.saturating_sub(hundreds(e.duration())),
    }
}

pub(crate) fn unix_ms(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

/// The transcripts of one folder, newest first.
fn transcripts(dir: &Path) -> Vec<Transcript> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Transcript> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let is_jsonl = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("jsonl"));
            // The enumeration's own record: no open per file (most of a warm build's time). A
            // link still resolves to its target as before.
            let meta = e.metadata().ok().filter(|m| !m.is_symlink()).or_else(|| fs::metadata(&path).ok());
            let meta = meta.filter(|m| m.is_file() && is_jsonl)?;
            Some(Transcript {
                path,
                name,
                modified: meta.modified().ok()?,
            })
        })
        .collect();
    out.sort_by_key(|t| std::cmp::Reverse(t.modified));
    out
}

/// `Get-ProjectPathFromTranscript`: the cwd of the first of the newest transcript's first 40
/// records that carries one. A slug is not reversible; the transcript is the only authority.
pub(crate) fn path_from_transcript(file: &Path) -> Option<String> {
    let mut r = BufReader::new(File::open(file).ok()?);
    let mut buf = Vec::new();
    for _ in 0..40 {
        buf.clear();
        if r.read_until(b'\n', &mut buf).ok()? == 0 {
            return None;
        }
        let Ok(rec) = serde_json::from_slice::<Value>(trim_line(&buf)) else {
            continue;
        };
        if let Some(cwd) = rec.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty()) {
            return Some(cwd.to_owned());
        }
    }
    None
}

fn trim_line(b: &[u8]) -> &[u8] {
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    let end = b.iter().rposition(|c| !c.is_ascii_whitespace()).map_or(0, |i| i + 1);
    b.get(..end).unwrap_or_default()
}

static ENTRYPOINT: LazyLock<Regex> = LazyLock::new(|| crate::theme::re(r#""entrypoint":\s?"([^"]*)""#));

/// `Test-HeadlessEntrypoint`: an entrypoint starting with `sdk-` is a headless run.
fn is_headless_entrypoint(e: &str) -> bool {
    e.starts_with("sdk-")
}

/// `Get-TranscriptHeadlessVerdict`: `Some(true)` when every entrypoint seen is headless,
/// `Some(false)` when any is not, `None` when none is seen or the file cannot be read. Seen = the
/// head up to its first entrypoint plus the last `window` bytes; a file up to two windows is read
/// whole.
pub(crate) fn headless_verdict(path: &Path, window: u64) -> Option<bool> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let head_max = if len <= 2 * window { len } else { window };
    let mut head = Vec::new();
    let mut chunk = [0u8; 8192];
    while (head.len() as u64) < head_max {
        let want = usize::try_from((head_max - head.len() as u64).min(8192)).unwrap_or(8192);
        let n = f.read(chunk.get_mut(..want)?).ok()?;
        if n == 0 {
            break;
        }
        head.extend_from_slice(chunk.get(..n)?);
        if head_max < len && ENTRYPOINT.is_match(&String::from_utf8_lossy(&head)) {
            break;
        }
    }
    let mut seen = String::from_utf8_lossy(&head).into_owned();
    if let Some(c) = ENTRYPOINT.captures(&seen)
        && !is_headless_entrypoint(c.get(1).map_or("", |m| m.as_str()))
    {
        return Some(false);
    }
    if head_max < len {
        f.seek(SeekFrom::Start(len - window)).ok()?;
        let mut tail = Vec::new();
        f.take(window).read_to_end(&mut tail).ok()?;
        seen.push('\n');
        seen.push_str(&String::from_utf8_lossy(&tail));
    }
    let mut any = false;
    for c in ENTRYPOINT.captures_iter(&seen) {
        any = true;
        if !is_headless_entrypoint(c.get(1).map_or("", |m| m.as_str())) {
            return Some(false);
        }
    }
    any.then_some(true)
}

/// `Test-HeadlessTranscript`: no entrypoint, or an unreadable file, is NOT headless.
#[cfg(test)]
pub(crate) fn is_headless_transcript(path: &Path) -> bool {
    headless_verdict(path, 65_536) == Some(true)
}

/// `ConvertFrom-ClaudeProjectSlug`'s worktree half: the name after `--claude-worktrees-`.
fn slug_worktree(slug: &str) -> Option<String> {
    slug.split_once("--claude-worktrees-").map(|(_, w)| w.to_owned())
}

/// `Find-ProjectInteractiveTranscript`: the newest transcript that is not a headless run. Only a
/// FOUND entrypoint is cached; a transcript read before its first records were written is asked
/// again next build.
fn interactive_transcript<'a>(
    slug: &str,
    ts: &'a [Transcript],
    cache: &Map<String, Value>,
    fresh: &mut Map<String, Value>,
) -> Option<&'a Transcript> {
    for t in ts {
        let key = format!("entrypoint|{slug}|{}|{}", t.name, ticks(t.modified));
        let headless = match cache.get(&key).and_then(|v| v.get("Headless")).and_then(Value::as_bool) {
            Some(h) => h,
            None => match headless_verdict(&t.path, 65_536) {
                None => return Some(t),
                Some(h) => h,
            },
        };
        fresh.insert(key, json!({ "Headless": headless }));
        if !headless {
            return Some(t);
        }
    }
    None
}

/// `Test-ProjectGitRoot`: `.git` is a DIRECTORY (a linked worktree's is a file).
fn is_git_root(path: &str) -> bool {
    !path.is_empty() && Path::new(path).join(".git").is_dir()
}

/// `Split-Path -Leaf`, with a drive root named as `C:`, not as the whole path again.
fn leaf_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let leaf = trimmed.rsplit(['\\', '/']).next().unwrap_or_default();
    if leaf.is_empty() { path.to_owned() } else { leaf.to_owned() }
}

fn read_cache(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// Atomic temp + rename; a failed write is a recompute next time, never an error.
fn write_cache(path: &Path, fresh: &Map<String, Value>) {
    let Ok(text) = serde_json::to_string_pretty(&Value::Object(fresh.clone())) else {
        return;
    };
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let tmp = PathBuf::from(format!("{}.{}-{:06x}.tmp", path.display(), std::process::id(), nonce & 0x00FF_FFFF));
    if fs::write(&tmp, text).is_err() || fs::rename(&tmp, path).is_err() {
        // Best effort: a temp file that cannot be removed is swept by nobody and harms nothing.
        fs::remove_file(&tmp).unwrap_or_default();
    }
}

/// One slug folder's row before the merge.
struct SlugRow {
    project: Project,
    interactive: bool,
}

/// Times `f` as the phase `name`.
fn timed<T>(phases: &mut Vec<Phase>, name: &'static str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    phases.push(Phase {
        name,
        start,
        end: Instant::now(),
    });
    out
}

/// Every slug folder with its transcripts, newest first; a folder without one is left out.
fn scan(root: &Path) -> Option<Vec<(String, Vec<Transcript>)>> {
    let dirs = fs::read_dir(root).ok()?;
    Some(
        dirs.flatten()
            .filter(|d| d.path().is_dir())
            .map(|d| (d.file_name().to_string_lossy().into_owned(), transcripts(&d.path())))
            .filter(|(_, ts)| !ts.is_empty())
            .collect(),
    )
}

/// Builds the registry. Never fails: a missing root is an empty list.
pub(crate) fn build_registry(inp: &RegistryInput<'_>) -> Registry {
    let mut phases = Vec::new();
    let Some(folders) = timed(&mut phases, "scan", || scan(inp.projects_root)) else {
        return Registry {
            projects: Vec::new(),
            phases,
        };
    };
    let cache = timed(&mut phases, "cache read", || read_cache(inp.cache_path));
    let mut fresh = Map::new();
    let rows = timed(&mut phases, "derive", || derive(inp, &folders, &cache, &mut fresh));
    timed(&mut phases, "cache write", || write_cache(inp.cache_path, &fresh));
    Registry {
        projects: merge(rows),
        phases,
    }
}

/// One row per slug folder whose path is known, with the cache entries this build keeps.
fn derive(
    inp: &RegistryInput<'_>,
    folders: &[(String, Vec<Transcript>)],
    cache: &Map<String, Value>,
    fresh: &mut Map<String, Value>,
) -> Vec<SlugRow> {
    let mut drives = DriveCache::new();
    let (temp_keys, profile_keys) = scratch_keys(inp.temp_roots, inp.profile_roots);
    let mut rows = Vec::new();
    for (slug, ts) in folders {
        let Some(newest) = ts.first() else { continue };
        let interactive = interactive_transcript(slug, ts, cache, fresh);
        let activity = interactive.unwrap_or(newest).modified;
        let key = format!("{slug}|{}", ticks(newest.modified));
        let cached_path = cache
            .get(&key)
            .and_then(|v| v.get("Path"))
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
            .map(str::to_owned);
        let Some(path) = cached_path.or_else(|| path_from_transcript(&newest.path)) else {
            continue;
        };
        if path.contains('\0') {
            continue;
        }
        let mut entry = Map::new();
        entry.insert("Path".into(), json!(path));
        let present = path_present(&path, &mut drives);
        let network = on_network(&path, &drives);
        let mut headless_hidden = false;
        if present && interactive.is_none() && !network && inp.hide_headless {
            headless_hidden = true;
        } else if present && interactive.is_none() && !network {
            let git = cache
                .get(&key)
                .and_then(|v| v.get("GitRoot"))
                .and_then(Value::as_bool)
                .unwrap_or_else(|| is_git_root(&path));
            entry.insert("GitRoot".into(), json!(git));
            headless_hidden = !git;
        }
        fresh.insert(key, Value::Object(entry));
        if !present {
            continue;
        }
        let scratch = !network && is_scratch_key(&scratch_key(&path), &temp_keys, &profile_keys);
        rows.push(SlugRow {
            project: Project {
                name: leaf_name(&path),
                path,
                slug: slug.clone(),
                slugs: vec![slug.clone()],
                worktree: slug_worktree(slug),
                last_activity_ms: Some(unix_ms(activity)),
                hidden: headless_hidden || scratch,
            },
            interactive: interactive.is_some(),
        });
    }
    rows
}

/// One real directory is one row: every slug kept, hidden only when every slug is, dated by its
/// newest interactive slug.
fn merge(rows: Vec<SlugRow>) -> Vec<Project> {
    let mut groups: Vec<(String, Vec<SlugRow>)> = Vec::new();
    for r in rows {
        let k = project_key(&r.project.path);
        match groups.iter_mut().find(|(g, _)| *g == k) {
            Some((_, v)) => v.push(r),
            None => groups.push((k, vec![r])),
        }
    }
    let mut merged: Vec<Project> = groups
        .into_iter()
        .filter_map(|(_, mut g)| {
            g.sort_by_key(|r| std::cmp::Reverse(r.project.last_activity_ms));
            let dated = g.iter().find(|r| r.interactive).map(|r| r.project.last_activity_ms);
            let hidden = g.iter().all(|r| r.project.hidden);
            let slugs = g.iter().map(|r| r.project.slug.clone()).collect();
            let top = g.into_iter().next()?.project;
            Some(Project {
                slugs,
                last_activity_ms: dated.unwrap_or(top.last_activity_ms),
                hidden,
                ..top
            })
        })
        .collect();
    merged.sort_by_key(|p| std::cmp::Reverse(p.last_activity_ms));
    merged
}

/// `Remove-StalePreviewProjectsCache`: only the exact name a preview writes, older than a day.
pub(crate) fn remove_stale_preview_cache_at(dir: &Path, now: SystemTime) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let cutoff = now.checked_sub(Duration::from_hours(24)).unwrap_or(UNIX_EPOCH);
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let ours = name
            .strip_prefix("claude-auto-projects-preview-")
            .and_then(|r| r.strip_suffix(".json"))
            .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()));
        let old = e
            .metadata()
            .ok()
            .filter(fs::Metadata::is_file)
            .and_then(|m| m.modified().ok())
            .is_some_and(|m| m < cutoff);
        if ours && old {
            // Best effort: a file another process holds is pruned by a later run.
            fs::remove_file(e.path()).unwrap_or_default();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
