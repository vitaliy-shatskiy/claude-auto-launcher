//! The session list: one summary per transcript, the newest-first
//! file snapshot a picker pages over, and the summary cache shared by every account root that
//! reaches the same physical projects directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use self::files::{remove_legacy_cache, sweep_stale_temps, temp_suffix};
use super::scan::{ScanState, Scanned, scan_transcript};
use super::transcript::{
    MAX_BYTES, clean_text, collapse_fences, field, head_lines, is_headless, parse_line, prompt_detail, record_text, slug_names, tail_lines,
    user_prompt,
};
use crate::config::ps_truthy;

mod files;
pub mod plan;

use self::plan::{Entry, Row};

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
    /// Every user and assistant message of the whole transcript (`scan`).
    pub message_count: i64,
    /// The `/rename` title, else the newest AI title, else the first prompt.
    pub title: String,
    pub first_prompt: String,
    pub last_user: String,
    pub last_assistant: String,
    pub recent: Vec<Message>,
    pub headless: bool,
    /// The title scan's resume point: the next summary of this file reads only past it.
    pub scan: ScanState,
    /// The other account whose store holds this session; empty for the launching one. Depends on
    /// who launches, so it never enters the summary cache.
    #[serde(skip)]
    pub account: String,
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

/// The cwd the NEWEST transcript of a slug directory records.
pub(crate) fn project_path_from_transcript(dir: &Path) -> Option<String> {
    let newest = jsonl_files(dir).into_iter().max_by_key(|(_, t)| *t)?.0;
    crate::projects::path_from_transcript(&newest)
}

/// `pre_filter` false parses every line (the suite's control); the
/// prompt count reads at most `max_bytes` (`MAX_BYTES`; a test passes a small cap).
#[cfg(test)]
pub(crate) fn summarize(path: &Path, project_path: Option<&str>, pre_filter: bool, max_bytes: usize) -> Option<Session> {
    summarize_from(path, project_path, pre_filter, max_bytes, Depth::Full(None))
}

/// How far a summary reads.
#[derive(Debug, Clone, Copy)]
enum Depth<'a> {
    /// Head and tail only, prompts counted in the first `QUICK_BYTES`: the first prompt is the
    /// title and no message is counted. Never cached.
    Quick,
    /// The whole file: the title scan, resumed from an earlier summary's `scan` of the same file.
    Full(Option<&'a ScanState>),
}

/// What a quick row counts prompts over (its count then reads "N+" on a longer file).
const QUICK_BYTES: usize = 256 * 1024;

fn summarize_from(path: &Path, project_path: Option<&str>, pre_filter: bool, max_bytes: usize, depth: Depth<'_>) -> Option<Session> {
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
    let first = head_lines(path, HEAD_LINES)
        .iter()
        .filter(|l| user_line(l))
        .filter_map(|l| parse_line(l))
        .find_map(|r| user_prompt(&r));
    let (last_user, last_assistant, recent) = walk_tail(path, pre_filter);
    let (count, capped) = match depth {
        Depth::Quick => prompt_detail(path, max_bytes.min(QUICK_BYTES)),
        Depth::Full(_) => prompt_detail(path, max_bytes),
    };
    let scanned = match depth {
        Depth::Quick => Scanned::default(),
        Depth::Full(prior) => scan_transcript(path, prior),
    };
    let first = first
        .or_else(|| last_user.clone())
        .unwrap_or_else(|| format!("(no prompt) {}", short_id(&session_id)));
    let Scanned { found, resume, .. } = scanned;
    s.prompt_count = count;
    s.prompt_count_capped = capped;
    s.message_count = found.messages;
    s.first_prompt = clean_text(&first);
    s.title = found.custom_title.or(found.ai_title).unwrap_or_else(|| s.first_prompt.clone());
    s.scan = resume;
    // A prompt the tail found is the newest; the scan's `last-prompt` covers one that a long
    // turn's tool lines pushed out of the tail (it lags a turn still running, so it comes second).
    s.last_user = clean_text(last_user.or(found.last_prompt).as_deref().unwrap_or(""));
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

/// Every transcript under the root (or under the slugs), newest first.
/// Taken once when a picker opens; every page indexes THIS list.
pub fn session_files(root: &Path, slugs: &[String]) -> Vec<PathBuf> {
    let slugs: Vec<String> = slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
    let mut all: Vec<(PathBuf, SystemTime)> = slug_dirs(root, &slugs).iter().flat_map(|d| jsonl_files(d)).collect();
    all.sort_by_key(|x| std::cmp::Reverse(x.1));
    all.into_iter().map(|(p, _)| p).collect()
}

/// Every link component resolved, no trailing separator; anything
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

/// The summary cache file, `claude-auto-sessions.json`; 0.9.0 took the name back from
/// `claude-auto-rs-sessions.json` (`files::LEGACY_CACHE_NAME`).
pub fn cache_path(projects_root: &Path) -> PathBuf {
    let physical = physical_dir(projects_root);
    physical.parent().unwrap_or(&physical).join(CACHE_NAME)
}

const CACHE_NAME: &str = "claude-auto-sessions.json";

/// The cached summary's schema: bump it by hand when what a summary holds or how it is derived
/// changes. A release that leaves summaries alone keeps every cache entry (a version taken from
/// the source text rescanned every transcript from byte 0 on the first picker after an update).
pub(crate) const SUMMARY_SCHEMA: u32 = 3;

/// The cache key's version part: the schema alone, so a touch or a code change keeps the entry.
pub(crate) fn summary_version() -> String {
    format!("v{SUMMARY_SCHEMA}")
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

    /// The file, a brief retry while another instance holds it, then this
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

    /// A sibling temp moved over the file (atomic, last writer wins);
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
                remove_legacy_cache(path);
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

    /// Up to `limit` summaries after `skip`, over the scope, cached. Every
    /// miss is summarised here, on the caller's thread (preview, tests, the perf world).
    pub fn list(&mut self, root: &Path, scope: Scope<'_>, skip: usize, limit: usize, cache_file: &Path) -> Vec<Session> {
        let Some(plan) = self.plan(root, scope, skip, limit, cache_file) else {
            return Vec::new();
        };
        let rows: Vec<(Entry, Session)> = plan
            .rows
            .into_iter()
            .filter_map(|r| match r {
                Row::Hit(e, s) => Some((e, *s)),
                Row::Miss(e) => e.summarize().map(|s| (e, s)),
            })
            .collect();
        if plan.survey {
            let fresh = rows.iter().map(|(e, s)| (e.key.clone(), s.clone())).collect();
            self.write(cache_file, &fresh);
        } else {
            self.merge_write(plan.cache, cache_file, &rows);
        }
        rows.into_iter().map(|(_, s)| s).collect()
    }

    /// Full summaries made elsewhere (the picker's worker) merged into the cache file as it is now.
    pub fn store(&mut self, cache_file: &Path, rows: &[(Entry, Session)]) {
        let cache = self.read(cache_file);
        self.merge_write(cache, cache_file, rows);
    }

    fn merge_write(&mut self, mut cache: BTreeMap<String, Session>, cache_file: &Path, rows: &[(Entry, Session)]) {
        // One entry per file: a grown transcript's older summary goes.
        cache.retain(|k, _| !rows.iter().any(|(e, _)| k.starts_with(e.same_file.as_str())));
        cache.extend(rows.iter().map(|(e, s)| (e.key.clone(), s.clone())));
        self.write(cache_file, &cache);
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
