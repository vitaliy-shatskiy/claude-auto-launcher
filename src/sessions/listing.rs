//! The session list (`Sessions.ps1` second half): one summary per transcript, the newest-first
//! file snapshot a picker pages over, and the summary cache shared by every account root that
//! reaches the same physical projects directory.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::transcript::{
    MAX_BYTES, clean_text, collapse_fences, field, head_lines, is_headless, parse_line, prompt_detail, record_text, slug_names, tail_lines,
    user_prompt,
};
use crate::config::ps_truthy;

/// Who said a recent message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Message {
    pub speaker: Speaker,
    pub text: String,
}

/// One transcript, summarised. Every text field went through `clean_text`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub session_id: String,
    pub path: PathBuf,
    pub slug: String,
    pub project: String,
    /// From the slug (a directory name), never from transcript content.
    pub worktree: String,
    /// Last write, ms since the Unix epoch (UTC).
    pub modified_ms: i64,
    pub size_bytes: u64,
    /// An int plus a flag, never "N+": the picker renders the '+' from the flag.
    pub prompt_count: i64,
    pub prompt_count_capped: bool,
    pub title: String,
    pub last_user: String,
    pub last_assistant: String,
    pub recent: Vec<Message>,
    pub headless: bool,
}

const HEAD_LINES: usize = 400;
const TAIL_LINES: usize = 120;
const MAX_RECENT: usize = 12;

fn ms_since_epoch(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn ticks(t: SystemTime) -> u128 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() / 100)
}

fn leaf(p: &str) -> String {
    let t = p.trim_end_matches(['\\', '/']);
    t.rsplit(['\\', '/']).next().unwrap_or(t).to_owned()
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// `Get-ProjectPathFromTranscript`: the cwd the NEWEST transcript of a slug directory records.
pub(crate) fn project_path_from_transcript(dir: &Path) -> Option<String> {
    let newest = jsonl_files(dir).into_iter().max_by_key(|(_, t)| *t)?.0;
    crate::projects::path_from_transcript(&newest)
}

/// `Get-ClaudeSessionSummary`. `pre_filter` false parses every line (the suite's control); the
/// prompt count reads at most `max_bytes` (`MAX_BYTES`; a test passes a small cap).
pub(crate) fn summarize(path: &Path, project_path: Option<&str>, pre_filter: bool, max_bytes: usize) -> Option<Session> {
    let meta = std::fs::metadata(path).ok()?;
    let session_id = path.file_stem()?.to_string_lossy().into_owned();
    let slug = path.parent()?.file_name()?.to_string_lossy().into_owned();
    let (slug_project, worktree) = slug_names(&slug);
    let project = clean_text(&project_path.map_or(slug_project, leaf));
    let mut s = Session {
        session_id: session_id.clone(),
        path: path.to_path_buf(),
        slug,
        project,
        worktree,
        modified_ms: meta.modified().map_or(0, ms_since_epoch),
        size_bytes: meta.len(),
        ..Session::default()
    };
    if is_headless(path) {
        s.title = format!("(headless) {}", short_id(&session_id));
        s.headless = true;
        return Some(s);
    }
    let user_line = |l: &str| !pre_filter || !l.contains("\"type\"") || super::transcript::is_user_line(l);
    let title = head_lines(path, HEAD_LINES)
        .iter()
        .filter(|l| user_line(l))
        .filter_map(|l| parse_line(l))
        .find_map(|r| user_prompt(&r));
    let (last_user, last_assistant, recent) = walk_tail(path, pre_filter);
    let (count, capped) = prompt_detail(path, max_bytes);
    let title = title
        .or_else(|| last_user.clone())
        .unwrap_or_else(|| format!("(no prompt) {}", short_id(&session_id)));
    s.prompt_count = count;
    s.prompt_count_capped = capped;
    s.title = clean_text(&title);
    s.last_user = clean_text(last_user.as_deref().unwrap_or(""));
    s.last_assistant = clean_text(last_assistant.as_deref().unwrap_or(""));
    s.recent = recent
        .into_iter()
        .map(|m| Message {
            speaker: m.speaker,
            text: clean_text(&collapse_fences(&m.text)),
        })
        .collect();
    Some(s)
}

fn is_assistant(r: &Value) -> bool {
    field(r, "type").is_some_and(|t| crate::config::ps_str(t).eq_ignore_ascii_case("assistant")) && !ps_truthy(field(r, "isSidechain"))
}

fn is_user(r: &Value) -> bool {
    field(r, "type").is_some_and(|t| crate::config::ps_str(t).eq_ignore_ascii_case("user"))
}

/// The tail walk, newest first: last user prompt, last assistant text, up to 12 recent messages.
fn walk_tail(path: &Path, pre_filter: bool) -> (Option<String>, Option<String>, Vec<Message>) {
    let (mut last_user, mut last_assistant) = (None, None);
    let mut recent = Vec::new();
    for line in tail_lines(path, TAIL_LINES, MAX_BYTES).iter().rev() {
        if pre_filter && line.contains("\"type\"") && !super::transcript::is_chat_line(line) {
            continue;
        }
        let Some(r) = parse_line(line) else { continue };
        let content = field(&r, "message").and_then(|m| field(m, "content"));
        if last_assistant.is_none() && is_assistant(&r) {
            last_assistant = record_text(content);
        }
        if last_user.is_none() {
            last_user = user_prompt(&r);
        }
        if recent.len() < MAX_RECENT {
            if is_assistant(&r) {
                if let Some(t) = record_text(content).filter(|t| !t.is_empty()) {
                    recent.push(Message {
                        speaker: Speaker::Assistant,
                        text: t,
                    });
                }
            } else if is_user(&r)
                && let Some(t) = user_prompt(&r)
            {
                recent.push(Message {
                    speaker: Speaker::User,
                    text: t,
                });
            }
        }
        if last_user.is_some() && last_assistant.is_some() && recent.len() >= MAX_RECENT {
            break;
        }
    }
    recent.reverse();
    (last_user, last_assistant, recent)
}

fn jsonl_files(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("jsonl")))
        .map(|e| (e.path(), e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH)))
        .collect()
}

fn slug_dirs(root: &Path, slugs: &[String]) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    rd.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() || t.is_symlink()) && e.path().is_dir())
        .filter(|e| slugs.is_empty() || slugs.iter().any(|s| e.file_name().to_string_lossy().eq_ignore_ascii_case(s)))
        .map(|e| e.path())
        .collect()
}

/// `Get-ClaudeSessionFile`: every transcript under the root (or under the slugs), newest first.
/// Taken once when a picker opens; every page indexes THIS list.
pub fn session_files(root: &Path, slugs: &[String]) -> Vec<PathBuf> {
    let slugs: Vec<String> = slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
    let mut all: Vec<(PathBuf, SystemTime)> = slug_dirs(root, &slugs).iter().flat_map(|d| jsonl_files(d)).collect();
    all.sort_by_key(|x| std::cmp::Reverse(x.1));
    all.into_iter().map(|(p, _)| p).collect()
}

/// `Get-PhysicalDirectoryPath`: every link component resolved, no trailing separator; anything
/// unresolvable comes back as given.
pub(crate) fn physical_dir(path: &Path) -> PathBuf {
    let s = std::fs::canonicalize(path).map_or_else(
        |_| path.to_string_lossy().into_owned(),
        |p| {
            let t = p.to_string_lossy().into_owned();
            if let Some(rest) = t.strip_prefix(r"\\?\UNC\") {
                format!(r"\\{rest}")
            } else {
                t.strip_prefix(r"\\?\").map_or(t.clone(), str::to_owned)
            }
        },
    );
    let trimmed = if s.chars().count() > 3 {
        s.trim_end_matches(['\\', '/'])
    } else {
        s.as_str()
    };
    PathBuf::from(trimmed)
}

/// `Get-SessionsCachePath`, renamed so the Rust and the PowerShell launcher never prune each
/// other's entries while both run (each keys rows by its own summary version).
pub fn cache_path(projects_root: &Path) -> PathBuf {
    let physical = physical_dir(projects_root);
    physical.parent().unwrap_or(&physical).join("claude-auto-rs-sessions.json")
}

/// The summary logic's own content hash: a code change invalidates the cache, a touch does not.
pub(crate) fn summary_version() -> String {
    let src = [include_str!("transcript.rs"), include_str!("listing.rs")].concat();
    crate::sys::sha256(src.as_bytes()).map_or_else(
        || "v2".to_owned(),
        |h| h.iter().take(8).map(|b| format!("{b:02X}")).collect::<Vec<_>>().concat(),
    )
}

/// What one listing call covers. A snapshot carries its own scope, so slug and files exclude
/// each other by construction.
#[derive(Debug, Clone, Copy)]
pub enum Scope<'a> {
    All,
    Slug(&'a str),
    Files(&'a [PathBuf]),
}

/// The cache file, with this process's last good copy for a busy read.
#[derive(Debug, Default)]
pub struct SessionCache {
    memo: Option<(PathBuf, String, BTreeMap<String, Session>)>,
    /// Preview writes nothing.
    read_only: bool,
}

fn stamp(path: &Path) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    Some(format!("{}|{}", m.modified().map_or(0, ticks), m.len()))
}

const SHARING_VIOLATION: i32 = 32;

impl SessionCache {
    /// A cache that never writes when `read_only` (preview).
    pub fn new(read_only: bool) -> Self {
        Self { memo: None, read_only }
    }

    /// `Read-SessionsCache`: the file, a brief retry while another instance holds it, then this
    /// process's own copy of the same bytes; corrupt = empty.
    pub fn read(&mut self, path: &Path) -> BTreeMap<String, Session> {
        let Some(st) = stamp(path) else { return BTreeMap::new() };
        for _ in 0..5 {
            match std::fs::read_to_string(path) {
                Ok(text) => {
                    let Ok(map) = serde_json::from_str::<BTreeMap<String, Session>>(&text) else {
                        return BTreeMap::new();
                    };
                    self.memo = Some((path.to_path_buf(), st, map.clone()));
                    return map;
                }
                Err(e) if e.raw_os_error() == Some(SHARING_VIOLATION) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => return BTreeMap::new(),
            }
        }
        match &self.memo {
            Some((p, s, m)) if p == path && *s == st => m.clone(),
            _ => BTreeMap::new(),
        }
    }

    /// `Write-SessionsCache`: a sibling temp moved over the file (atomic, last writer wins);
    /// stranded temps older than a minute swept first; any failure is swallowed.
    pub fn write(&mut self, path: &Path, entries: &BTreeMap<String, Session>) {
        if self.read_only {
            return;
        }
        sweep_stale_temps(path);
        let Ok(text) = serde_json::to_string_pretty(entries) else { return };
        let tmp = PathBuf::from(format!("{}.{:032x}.tmp", path.display(), temp_suffix()));
        if std::fs::write(&tmp, text).is_err() {
            return;
        }
        for i in 0..5 {
            if std::fs::rename(&tmp, path).is_ok() {
                if let Some(st) = stamp(path) {
                    self.memo = Some((path.to_path_buf(), st, entries.clone()));
                }
                return;
            }
            if i < 4 {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        if std::fs::remove_file(&tmp).is_err() {
            // The next write sweeps it once it is a minute old.
        }
    }

    /// `Get-ClaudeSessions`: up to `limit` summaries after `skip`, over the scope, cached.
    pub fn list(&mut self, root: &Path, scope: Scope<'_>, skip: usize, limit: usize, cache_file: &Path) -> Vec<Session> {
        if !root.exists() {
            return Vec::new();
        }
        let cache = self.read(cache_file);
        let all: Vec<PathBuf> = match scope {
            Scope::Files(f) => f.iter().filter(|p| p.is_file()).cloned().collect(),
            Scope::Slug(s) => session_files(root, &[s.to_owned()]),
            Scope::All => session_files(root, &[]),
        };
        let window: Vec<&PathBuf> = all.iter().skip(skip).take(limit).collect();
        let window_len = window.len();
        let version = summary_version();
        let named = root.to_string_lossy().into_owned();
        let named = if named.chars().count() > 3 {
            named.trim_end_matches(['\\', '/']).to_owned()
        } else {
            named
        };
        let physical = physical_dir(root).to_string_lossy().into_owned();
        let mut dirs: HashMap<PathBuf, Option<String>> = HashMap::new();
        let mut fresh = BTreeMap::new();
        let mut out = Vec::new();
        for f in window {
            let Ok(meta) = std::fs::metadata(f) else { continue };
            let full = f.to_string_lossy().into_owned();
            let key_path = match full.get(..named.len()) {
                Some(head) if named != physical && head.eq_ignore_ascii_case(&named) => {
                    format!("{physical}{}", full.get(named.len()..).unwrap_or(""))
                }
                _ => full.clone(),
            };
            let key = format!("{version}|{key_path}|{}|{}", meta.modified().map_or(0, ticks), meta.len());
            let dir = f.parent().map(Path::to_path_buf).unwrap_or_default();
            let project_path = dirs
                .entry(dir.clone())
                .or_insert_with(|| project_path_from_transcript(&dir))
                .clone();
            let summary = if let Some(hit) = cache.get(&key) {
                let mut s = hit.clone();
                s.path.clone_from(f);
                s.project = clean_text(&project_path.as_deref().map_or_else(|| slug_names(&s.slug).0, leaf));
                s
            } else {
                let Some(s) = summarize(f, project_path.as_deref(), true, MAX_BYTES) else {
                    continue;
                };
                s
            };
            fresh.insert(key, summary.clone());
            out.push(summary);
        }
        let survey = matches!(scope, Scope::All) && skip == 0 && window_len == all.len();
        if survey {
            self.write(cache_file, &fresh);
        } else {
            let mut merged = cache;
            merged.extend(fresh);
            self.write(cache_file, &merged);
        }
        out
    }
}

fn temp_suffix() -> u128 {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    t ^ (u128::from(std::process::id()) << 64)
}

fn sweep_stale_temps(path: &Path) {
    let (Some(dir), Some(leaf)) = (path.parent(), path.file_name()) else {
        return;
    };
    let prefix = format!("{}.", leaf.to_string_lossy());
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let cutoff = SystemTime::now().checked_sub(Duration::from_secs(60)).unwrap_or(UNIX_EPOCH);
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < cutoff);
        let tmp = Path::new(&name).extension().is_some_and(|x| x.eq_ignore_ascii_case("tmp"));
        if name.starts_with(&prefix) && tmp && old && std::fs::remove_file(e.path()).is_err() {
            // Another instance may be removing it at the same moment.
        }
    }
}

/// A JSON object of the cache, for tests that count keys.
#[cfg(test)]
pub(crate) fn cache_keys(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Map<String, Value>>(&t).ok())
        .map_or(0, |m| m.len())
}

#[cfg(test)]
mod tests;
