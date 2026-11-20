//! A page before it is summarised: each listed file's cache key, the hit or the miss, and what a
//! miss needs (the scan to resume). The picker shows quick rows for the misses and summarises them
//! on its worker; `list` summarises them in place.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use super::{
    Depth, Scope, Session, SessionCache, clean_text, leaf, physical_dir, project_path_from_transcript, session_files, slug_names,
    summarize_from, summary_version, ticks,
};
use crate::sessions::scan::ScanState;
use crate::sessions::transcript::MAX_BYTES;

impl SessionCache {
    /// A page's rows in list order: hits ready, misses with what summarising them needs.
    /// `None` = nothing to list. Reads the cache file once; writes nothing.
    pub fn plan(&mut self, root: &Path, scope: Scope<'_>, skip: usize, limit: usize, cache_file: &Path) -> Option<Plan> {
        // A snapshot names its files: another account's sessions list although this account
        // has no projects folder yet (AC29).
        if !matches!(scope, Scope::Files(_)) && !root.exists() {
            return None;
        }
        let cache = self.read(cache_file);
        let all: Vec<PathBuf> = match scope {
            Scope::Files(f) => f.iter().filter(|p| p.is_file()).cloned().collect(),
            Scope::Slug(s) => session_files(root, &[s.to_owned()]),
            Scope::All => session_files(root, &[]),
        };
        let window: Vec<&PathBuf> = all.iter().skip(skip).take(limit).collect();
        let survey = matches!(scope, Scope::All) && skip == 0 && window.len() == all.len();
        let version = summary_version();
        let named = root.to_string_lossy().into_owned();
        let named = if named.chars().count() > 3 {
            named.trim_end_matches(['\\', '/']).to_owned()
        } else {
            named
        };
        let physical = physical_dir(root).to_string_lossy().into_owned();
        let mut dirs: HashMap<PathBuf, Option<String>> = HashMap::new();
        let mut rows = Vec::new();
        for f in window {
            let Ok(meta) = std::fs::metadata(f) else { continue };
            let full = f.to_string_lossy().into_owned();
            let key_path = match full.get(..named.len()) {
                Some(head) if named != physical && head.eq_ignore_ascii_case(&named) => {
                    format!("{physical}{}", full.get(named.len()..).unwrap_or(""))
                }
                _ => full.clone(),
            };
            let same_file = format!("{version}|{key_path}|");
            let key = format!("{same_file}{}|{}", meta.modified().map_or(0, ticks), meta.len());
            let dir = f.parent().map(Path::to_path_buf).unwrap_or_default();
            let project_path = dirs
                .entry(dir.clone())
                .or_insert_with(|| project_path_from_transcript(&dir))
                .clone();
            // A changed file: its last summary's scan resumes where it stopped.
            let prior = cache
                .iter()
                .filter(|(k, _)| k.starts_with(&same_file))
                .map(|(_, s)| &s.scan)
                .max_by_key(|scan| scan.offset)
                .cloned();
            let hit = cache.get(&key).map(|hit| {
                let mut s = hit.clone();
                s.path.clone_from(f);
                s.project = clean_text(&project_path.as_deref().map_or_else(|| slug_names(&s.slug).0, leaf));
                s
            });
            let e = Entry {
                key,
                same_file,
                path: f.clone(),
                project_path,
                prior,
            };
            rows.push(match hit {
                Some(s) => Row::Hit(e, Box::new(s)),
                None => Row::Miss(e),
            });
        }
        Some(Plan { rows, cache, survey })
    }
}

/// A listed file's cache identity and what summarising it needs.
#[derive(Debug, Clone)]
pub struct Entry {
    pub(super) key: String,
    /// The key without the file's stamp: every summary of this file starts with it.
    pub(super) same_file: String,
    path: PathBuf,
    project_path: Option<String>,
    prior: Option<ScanState>,
}

impl Entry {
    /// The full summary: the title scan reads from `prior` to the end of the file.
    pub fn summarize(&self) -> Option<Session> {
        summarize_from(
            &self.path,
            self.project_path.as_deref(),
            true,
            MAX_BYTES,
            Depth::Full(self.prior.as_ref()),
        )
    }

    /// What a head and tail read give, for a row shown before its full summary lands. Never
    /// stored: the cache holds full summaries only.
    pub fn quick(&self) -> Option<Session> {
        summarize_from(&self.path, self.project_path.as_deref(), true, MAX_BYTES, Depth::Quick)
    }
}

/// One row of a page before its misses are summarised.
#[derive(Debug, Clone)]
pub enum Row {
    Hit(Entry, Box<Session>),
    Miss(Entry),
}

/// A page as `SessionCache::plan` found it.
#[derive(Debug)]
pub struct Plan {
    pub rows: Vec<Row>,
    pub(super) cache: BTreeMap<String, Session>,
    /// The whole root in one page: a write replaces the cache instead of merging.
    pub(super) survey: bool,
}
