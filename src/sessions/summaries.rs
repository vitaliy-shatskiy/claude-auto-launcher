//! Summaries off the UI thread: a picker page lists cache hits and quick rows at once, and one
//! worker summarises the page's misses in full, stores them in the cache file and hands them back
//! as one batch per page (one redraw). Preview never starts it: it lists in place and writes nothing.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use super::listing::plan::{Entry, Row};
use super::listing::{Scope, Session, SessionCache};

/// What summarises one miss in full: `Entry::summarize` in the app, a held stub in tests. Runs on
/// the worker thread.
pub type Summarize = Arc<dyn Fn(&Entry) -> Option<Session> + Send + Sync>;

#[derive(Debug)]
struct Job {
    cache_file: PathBuf,
    misses: Vec<Entry>,
}

/// The worker's inbox: each picker page sends its misses here.
#[derive(Debug, Clone)]
pub struct Summarizer {
    jobs: Sender<Job>,
}

impl Summarizer {
    /// Starts the worker. Each page's full summaries go to the cache file first, then to `done`.
    /// It ends once every `Summarizer` clone is dropped and its queue is done.
    pub fn start(summarize: Summarize, done: Sender<Vec<Session>>) -> Self {
        let (jobs, inbox) = channel::<Job>();
        std::thread::spawn(move || {
            let mut cache = SessionCache::new(false);
            while let Ok(job) = inbox.recv() {
                let rows: Vec<(Entry, Session)> = job.misses.into_iter().filter_map(|e| summarize(&e).map(|s| (e, s))).collect();
                if rows.is_empty() {
                    continue;
                }
                cache.store(&job.cache_file, &rows);
                if done.send(rows.into_iter().map(|(_, s)| s).collect()).is_err() {
                    // The picker is gone (a session was picked): the cache still has them.
                }
            }
        });
        Self { jobs }
    }

    /// One picker page over the snapshot `files`: hits as cached, misses as quick rows (head and
    /// tail only), the misses queued for their full summaries. Reads no transcript past its head
    /// and tail; writes nothing.
    pub fn page(
        &self,
        cache: &mut SessionCache,
        root: &Path,
        files: &[PathBuf],
        skip: usize,
        limit: usize,
        cache_file: &Path,
    ) -> Vec<Session> {
        let Some(plan) = cache.plan(root, Scope::Files(files), skip, limit, cache_file) else {
            return Vec::new();
        };
        let (mut out, mut misses) = (Vec::new(), Vec::new());
        for row in plan.rows {
            match row {
                Row::Hit(_, s) => out.push(*s),
                Row::Miss(e) => {
                    if let Some(s) = e.quick() {
                        out.push(s);
                        misses.push(e);
                    }
                }
            }
        }
        let job = Job {
            cache_file: cache_file.to_path_buf(),
            misses,
        };
        if !job.misses.is_empty() && self.jobs.send(job).is_err() {
            // The worker is gone: the quick rows stay as they are.
        }
        out
    }
}

#[cfg(test)]
mod tests;
