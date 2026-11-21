//! The selected project's context beside the list: its git branch, how many sessions it has and
//! the newest of them, from the summary cache (a miss gets the quick head-and-tail row). Loaded off
//! the UI thread; preview loads in place. Exports `Detail`, `DetailSource`, `git_branch`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use super::paths::project_key;
use crate::sessions::listing::plan::Row;
use crate::sessions::listing::{Scope, SessionCache, cache_path, session_files};

/// The most sessions one detail lists: more than a 72-row terminal shows.
const LIMIT: usize = 60;

/// One session of the selected project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionLine {
    pub title: String,
    pub modified_ms: i64,
    /// A running Claude Code has it open.
    pub live: bool,
}

/// What the right column shows for one project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Detail {
    /// `project_key` of the project's path: a detail belongs to exactly one row.
    pub key: String,
    pub branch: Option<String>,
    pub count: usize,
    /// Newest first.
    pub sessions: Vec<SessionLine>,
}

/// The branch `.git/HEAD` names, or the short commit of a detached head; `None` outside a repo.
/// A worktree's `.git` file points at its own HEAD.
pub(crate) fn git_branch(dir: &Path) -> Option<String> {
    let dot = dir.join(".git");
    let head_file = if dot.is_file() {
        let text = std::fs::read_to_string(&dot).ok()?;
        PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim()).join("HEAD")
    } else {
        dot.join("HEAD")
    };
    let head = std::fs::read_to_string(head_file).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref:") {
        Some(r) => Some(r.trim().trim_start_matches("refs/heads/").to_owned()),
        None => Some(head.chars().take(7).collect()).filter(|s: &String| !s.is_empty()),
    }
}

/// One request: the project's path and every slug of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ask {
    path: String,
    slugs: Vec<String>,
}

/// Where details come from: the account's projects root and the `sessions\` folders of the
/// running sessions.
#[derive(Debug, Clone)]
pub struct Stores {
    pub root: PathBuf,
    pub live_dirs: Vec<PathBuf>,
}

/// The detail of `ask`: reads the cache file and the head and tail of each miss, never writes.
fn load(stores: &Stores, ask: &Ask, live: &dyn Fn(&[PathBuf]) -> HashSet<String>) -> Detail {
    let mut d = Detail {
        key: project_key(&ask.path),
        branch: git_branch(Path::new(&ask.path)),
        ..Detail::default()
    };
    let slugs: Vec<String> = ask.slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
    // No slug = no session; an empty slug list would list every project's.
    if slugs.is_empty() {
        return d;
    }
    let files = session_files(&stores.root, &slugs);
    d.count = files.len();
    let Some(plan) = SessionCache::new(true).plan(&stores.root, Scope::Files(&files), 0, LIMIT, &cache_path(&stores.root)) else {
        return d;
    };
    let ids = live(&stores.live_dirs);
    d.sessions = plan
        .rows
        .into_iter()
        .filter_map(|r| match r {
            Row::Hit(_, s) => Some(*s),
            Row::Miss(e) => e.quick(),
        })
        .map(|s| SessionLine {
            live: ids.contains(&s.session_id),
            title: s.title,
            modified_ms: s.modified_ms,
        })
        .collect();
    d
}

fn live_ids(dirs: &[PathBuf]) -> HashSet<String> {
    crate::sessions::holder::live_ids(dirs, &crate::sessions::win::process_start)
}

/// How the screen gets details: a worker thread off preview, in place in preview (it never idles).
#[derive(Debug)]
pub(crate) enum DetailSource {
    Worker {
        asks: Sender<Ask>,
        done: Receiver<Detail>,
        asked: Option<Ask>,
    },
    InPlace {
        stores: Stores,
        asked: Option<Ask>,
    },
}

impl DetailSource {
    /// Starts the worker; it ends with the screen (its inbox closes).
    pub(crate) fn worker(stores: Stores) -> Self {
        let (asks, inbox) = channel::<Ask>();
        let (tx, done) = channel();
        std::thread::spawn(move || {
            while let Ok(mut ask) = inbox.recv() {
                // Only the newest ask matters: the cursor moved on past the older ones.
                while let Ok(newer) = inbox.try_recv() {
                    ask = newer;
                }
                if tx.send(load(&stores, &ask, &live_ids)).is_err() {
                    return;
                }
            }
        });
        Self::Worker { asks, done, asked: None }
    }

    pub(crate) fn in_place(stores: Stores) -> Self {
        Self::InPlace { stores, asked: None }
    }

    /// Asks for the project at `path` unless it was the last ask. In place, the answer comes back
    /// now; from the worker, `poll` brings it.
    pub(crate) fn ask(&mut self, path: &str, slugs: &[String]) -> Option<Detail> {
        let ask = Ask {
            path: path.to_owned(),
            slugs: slugs.to_vec(),
        };
        match self {
            Self::Worker { asks, asked, .. } => {
                if asked.as_ref() != Some(&ask) && asks.send(ask.clone()).is_ok() {
                    *asked = Some(ask);
                }
                None
            }
            Self::InPlace { stores, asked } => {
                if asked.as_ref() == Some(&ask) {
                    return None;
                }
                let d = load(stores, &ask, &|_| HashSet::new());
                *asked = Some(ask);
                Some(d)
            }
        }
    }

    /// The newest detail the worker finished since the last poll.
    pub(crate) fn poll(&mut self) -> Option<Detail> {
        match self {
            Self::Worker { done, .. } => done.try_iter().last(),
            Self::InPlace { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cars-detail-{name}-{}", std::process::id()));
        if d.exists() {
            std::fs::remove_dir_all(&d).unwrap();
        }
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn git_branch_reads_head_and_a_worktree_link() {
        let d = temp("git");
        std::fs::create_dir_all(d.join("repo/.git")).unwrap();
        std::fs::write(d.join("repo/.git/HEAD"), "ref: refs/heads/fix/cockpit\n").unwrap();
        assert_eq!(git_branch(&d.join("repo")).as_deref(), Some("fix/cockpit"));
        std::fs::create_dir_all(d.join("wt-git")).unwrap();
        std::fs::write(d.join("wt-git/HEAD"), "0123456789abcdef\n").unwrap();
        std::fs::create_dir_all(d.join("wt")).unwrap();
        std::fs::write(d.join("wt/.git"), format!("gitdir: {}\n", d.join("wt-git").display())).unwrap();
        assert_eq!(
            git_branch(&d.join("wt")).as_deref(),
            Some("0123456"),
            "a detached head: its short commit"
        );
        assert_eq!(git_branch(&d.join("none")), None);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_detail_counts_the_projects_sessions_newest_first_and_marks_the_live_one() {
        let d = temp("load");
        let slug = d.join("projects").join("C--p");
        std::fs::create_dir_all(&slug).unwrap();
        let t0 = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        for (i, (id, prompt)) in [("old", "first task"), ("new", "second task")].into_iter().enumerate() {
            let f = slug.join(format!("{id}.jsonl"));
            std::fs::write(&f, format!("{{\"type\":\"user\",\"message\":{{\"content\":\"{prompt}\"}}}}\n")).unwrap();
            let at = t0 + std::time::Duration::from_secs(60 * u64::try_from(i).unwrap());
            std::fs::File::options().write(true).open(&f).unwrap().set_modified(at).unwrap();
        }
        let stores = Stores {
            root: d.join("projects"),
            live_dirs: Vec::new(),
        };
        let ask = Ask {
            path: r"C:\p".into(),
            slugs: vec!["C--p".into()],
        };
        let got = load(&stores, &ask, &|_| HashSet::from(["new".to_owned()]));
        assert_eq!(got.count, 2);
        let titles: Vec<(&str, bool)> = got.sessions.iter().map(|s| (s.title.as_str(), s.live)).collect();
        assert_eq!(titles, [("second task", true), ("first task", false)]);
        let none = load(&stores, &Ask { slugs: Vec::new(), ..ask }, &|_| HashSet::new());
        assert_eq!(none.count, 0, "no slug lists nothing, never every project");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
