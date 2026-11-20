//! Sessions and the session picker. The entry points `app.rs` calls; the work lives in
//! `sessions/`: `transcript` (records), `listing` (summaries, snapshot, cache), `frame` (the
//! picker frame and memo), `picker` (the screen), `holder` (another live claude on a session).

pub mod chat;
pub mod cross_account;
pub mod frame;
pub mod holder;
pub mod listing;
pub mod picker;
pub mod scan;
pub mod summaries;
pub mod transcript;
pub(crate) mod win;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{Config, PathCtx};
use crate::frame::Style;
use crate::ui::{Term, UiStop};

use self::cross_account::{OtherStore, Transfer, joined_snapshot, tag_sessions, transfer_of};
use self::frame::Clock;
use self::listing::{Scope, SessionCache, cache_path, session_files};
use self::picker::PickerScreen;

/// Rows per page: the first frame costs one page of summarising.
const PAGE_SIZE: usize = 10;

/// The longest session id taken from outside (`--launcher-run`, a source plugin).
const MAX_SESSION_ID: usize = 128;

/// A session id from outside: it becomes an argument of `claude`, so one leading `-` would read as
/// an option; letters, digits, `-` and `_` only, at most `MAX_SESSION_ID` long.
pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_SESSION_ID
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// What the picker returns: the session to resume, and whether to fork it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedSession {
    pub session_id: String,
    pub fork: bool,
    /// Set when the session lives in another account's store.
    pub transfer: Option<Transfer>,
}

/// Every other account's projects store, for the picker. With `sharing` on all accounts read one
/// root, so there is nothing else to fold in and the picker stays as it was.
pub fn other_stores(cfg: &Config, account: &str) -> Vec<OtherStore> {
    if cfg.sharing {
        return Vec::new();
    }
    cfg.accounts
        .iter()
        .filter(|a| !a.key.eq_ignore_ascii_case(account))
        .map(|a| OtherStore {
            account: a.key.clone(),
            root: Path::new(&a.root).join("projects"),
        })
        .collect()
}

/// Everything the picker is opened with.
#[derive(Debug, Clone)]
pub struct PickerRequest<'a> {
    pub sessions_root: PathBuf,
    /// The other accounts' stores folded into the list (`other_stores`).
    pub others: &'a [OtherStore],
    /// Every slug of the chosen project; empty = the whole account.
    pub project_slugs: &'a [String],
    pub project_name: &'a str,
    pub style: Style,
    /// Every account's `sessions\` folder: a session a live process has open gets the green dot.
    pub live_dirs: &'a [PathBuf],
    pub preview: bool,
}

/// The sessions root with its inputs explicit: sharing on = one root for every account (the
/// `CLAUDE_AUTO_PROJECTS_ROOT` override, else `~\.claude\projects`); off = the account's own.
pub fn sessions_root_from(cfg: &Config, account: &str, projects_override: Option<&str>, home: &Path) -> PathBuf {
    if cfg.sharing {
        return match projects_override {
            Some(o) if !o.is_empty() => PathBuf::from(o),
            _ => home.join(".claude").join("projects"),
        };
    }
    // An unknown account degrades to the canonical root: the launch goes on.
    cfg.accounts
        .iter()
        .find(|a| a.key.eq_ignore_ascii_case(account))
        .or_else(|| cfg.canonical())
        .map_or_else(|| home.join(".claude").join("projects"), |a| Path::new(&a.root).join("projects"))
}

pub fn sessions_root(cfg: &Config, account: &str) -> PathBuf {
    let over = std::env::var("CLAUDE_AUTO_PROJECTS_ROOT").ok();
    sessions_root_from(cfg, account, over.as_deref(), &PathCtx::from_env().home)
}

fn clock_now() -> Clock {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Clock {
        now_ms,
        zone: frame::Zone::machine(),
    }
}

/// The picker over paged, snapshotted lists: the file list is taken once per scope
/// and every page skips over THAT list. `Ok(None)` = Esc.
pub fn run_picker(term: &mut dyn Term, req: &PickerRequest<'_>) -> Result<Option<PickedSession>, UiStop> {
    run_picker_with(term, req, &|_| Vec::new())
}

/// `run_picker` with sessions from outside the stores (`extra(slugs)`, plugin sources): tagged by
/// their source, they join each scope's first page newest first and resume without a transfer.
pub fn run_picker_with(
    term: &mut dyn Term,
    req: &PickerRequest<'_>,
    extra: &dyn Fn(&[String]) -> Vec<listing::Session>,
) -> Result<Option<PickedSession>, UiStop> {
    let tm = term.logger().timings.clone();
    let load = tm.begin(&["load", "picker"]);
    let root = req.sessions_root.clone();
    let cache_file = cache_path(&root);
    let mut cache = SessionCache::new(req.preview);
    let mut snapshots: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut extras: HashMap<String, Vec<listing::Session>> = HashMap::new();
    // Off preview the UI thread lists cache hits and quick rows only; the worker summarises the
    // misses and their rows update as each page lands. Preview lists in place and writes nothing.
    let (landed_tx, landed) = std::sync::mpsc::channel();
    let worker = (!req.preview).then(|| {
        let full: summaries::Summarize = std::sync::Arc::new(listing::plan::Entry::summarize);
        summaries::Summarizer::start(full, landed_tx)
    });
    let mut fetch = |have: usize, slugs: &[String]| {
        let added = extras.entry(slugs.join("|")).or_insert_with(|| extra(slugs));
        let snap = snapshots
            .entry(slugs.join("|"))
            .or_insert_with(|| joined_snapshot(session_files(&root, slugs), req.others, slugs));
        // `have` counts the extra rows the first page carried; the file snapshot never held them.
        let from_files = if have == 0 { 0 } else { have.saturating_sub(added.len()) };
        let mut page = match &worker {
            Some(w) => w.page(&mut cache, &root, snap, from_files, PAGE_SIZE, &cache_file),
            None => cache.list(&root, Scope::Files(snap), from_files, PAGE_SIZE, &cache_file),
        };
        tag_sessions(&mut page, req.others);
        if have == 0 && !added.is_empty() {
            page.extend(added.iter().cloned());
            page.sort_by_key(|s| std::cmp::Reverse(s.modified_ms));
        }
        page
    };
    let slugs: Vec<String> = req.project_slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
    let first = fetch(0, &slugs);
    let dirs = req.live_dirs.to_vec();
    let live = move || holder::live_ids(&dirs, &win::process_start);
    let mut screen =
        PickerScreen::new(first, Some(&mut fetch), PAGE_SIZE, &slugs, req.project_name, req.style, clock_now()).with_live(Box::new(live));
    // Preview never polls idle (scripted keys only), so a background load would never land:
    // it shows the summary's recent messages instead.
    if !req.preview {
        screen = screen.with_chat(chat::pane::ChatPane::new(tm.clone())).with_landed(landed);
    }
    tm.end(load);
    let picked = crate::ui::screen_loop::run(&mut screen, term)?;
    Ok(picked.map(|p| PickedSession {
        // An extra session has no transcript file here: nothing to copy.
        transfer: (!p.session.path.as_os_str().is_empty()).then(|| transfer_of(&p.session)).flatten(),
        session_id: p.session.session_id,
        fork: p.fork,
    }))
}

/// The warning line for the claude.exe holding the session
/// `claude -c` would attach to, or `None`. Any failure is `None`.
pub fn continue_session_holder(sessions_dirs: &[PathBuf], slugs: &[String], projects_root: &Path) -> Option<String> {
    let id = holder::continue_session_id(projects_root, slugs, sessions_dirs);
    session_holder(sessions_dirs, &id)
}

/// The warning line for the claude.exe holding this session id.
pub fn session_holder(sessions_dirs: &[PathBuf], session_id: &str) -> Option<String> {
    let h = holder::find_holder(sessions_dirs, session_id)?;
    let offset = crate::sys::local_now().map_or(0, |t| t.offset_minutes);
    Some(holder::warning(&h, offset))
}

/// The holder functions above already return the formatted line.
pub fn held_warning(holder: &str) -> String {
    holder.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Account;

    fn cfg(sharing: bool) -> Config {
        let ctx = PathCtx {
            home: PathBuf::from(r"C:\Users\x"),
            cwd: PathBuf::from(r"C:\"),
        };
        let mut c = Config::defaults(&ctx, "");
        c.accounts.push(Account {
            key: "second".into(),
            root: r"C:\roots\second".into(),
            label: "second".into(),
            tint: "Cyan".into(),
            hidden: false,
            canonical: false,
        });
        c.sharing = sharing;
        c
    }

    #[test]
    fn sharing_reads_one_root_and_off_reads_the_accounts_own() {
        let home = Path::new(r"C:\Users\x");
        assert_eq!(
            sessions_root_from(&cfg(true), "second", None, home),
            home.join(".claude").join("projects")
        );
        assert_eq!(
            sessions_root_from(&cfg(true), "second", Some(r"D:\over"), home),
            PathBuf::from(r"D:\over")
        );
        assert_eq!(
            sessions_root_from(&cfg(false), "second", Some(r"D:\over"), home),
            PathBuf::from(r"C:\roots\second\projects")
        );
        assert_eq!(
            sessions_root_from(&cfg(false), "nope", None, home),
            PathBuf::from(r"C:\Users\x\.claude\projects"),
            "an unknown account degrades to the canonical root"
        );
    }

    #[test]
    fn sharing_off_lists_and_caches_per_account() {
        let base = std::env::temp_dir().join(format!("cars-w1-root-{}", std::process::id()));
        let second = base
            .join("second")
            .join("projects")
            .join("C--Users-someone-Desktop-Projects-SecondOnly");
        let work = base
            .join("work")
            .join("projects")
            .join("C--Users-someone-Desktop-Projects-WorkOnly");
        for (d, id) in [(&second, "bbbbbbbb"), (&work, "aaaaaaaa")] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(
                d.join(format!("{id}.jsonl")),
                "{\"type\":\"user\",\"message\":{\"content\":\"p\"}}\n",
            )
            .unwrap();
        }
        let root = base.join("second").join("projects");
        let rows = SessionCache::default().list(&root, Scope::All, 0, 40, &cache_path(&root));
        assert_eq!(rows.iter().map(|s| s.session_id.as_str()).collect::<Vec<_>>(), ["bbbbbbbb"]);
        assert!(base.join("second").join("claude-auto-sessions.json").exists());
        assert!(!base.join("work").join("claude-auto-sessions.json").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn preview_picker_writes_no_cache() {
        let base = std::env::temp_dir().join(format!("cars-w1-prev-{}", std::process::id()));
        let d = base.join("projects").join("C--src-p");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("s1.jsonl"), "{\"type\":\"user\",\"message\":{\"content\":\"p\"}}\n").unwrap();
        let req = PickerRequest {
            sessions_root: base.join("projects"),
            others: &[],
            project_slugs: &["C--src-p".to_owned()],
            project_name: "p",
            style: Style { color: false, ascii: true },
            live_dirs: &[],
            preview: true,
        };
        let keys = crate::ui::parse_preview_keys("Enter").unwrap();
        let mut term = crate::ui::PreviewTerm::new((100, 30), keys, true);
        let got = run_picker(&mut term, &req).unwrap().unwrap();
        assert_eq!(got.session_id, "s1");
        assert!(!base.join("claude-auto-sessions.json").exists(), "preview is side-effect-free");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn other_stores_are_every_other_account_only_while_sharing_is_off() {
        let off = cfg(false);
        let stores = other_stores(&off, "SECOND");
        assert_eq!(
            stores,
            [OtherStore {
                account: off.accounts[0].key.clone(),
                root: Path::new(&off.accounts[0].root).join("projects"),
            }],
            "the launching account is not its own other store"
        );
        assert!(
            other_stores(&cfg(true), "second").is_empty(),
            "sharing on: one root, nothing to fold"
        );
    }

    /// `own` holds one session of the launching account, `other` one of another account.
    fn two_stores(tag: &str) -> (PathBuf, Vec<OtherStore>) {
        let base = std::env::temp_dir().join(format!("cars-w1-cross-{tag}-{}", std::process::id()));
        let line = "{\"type\":\"user\",\"message\":{\"content\":\"p\"}}\n";
        for (store, id, age) in [("own", "mine", 100), ("other", "theirs", 10)] {
            let d = base.join(store).join("projects").join("C--src-p");
            std::fs::create_dir_all(&d).unwrap();
            let f = d.join(format!("{id}.jsonl"));
            std::fs::write(&f, line).unwrap();
            let t = UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000 - age);
            std::fs::File::options().write(true).open(&f).unwrap().set_modified(t).unwrap();
        }
        let others = vec![OtherStore {
            account: "other".into(),
            root: base.join("other").join("projects"),
        }];
        (base, others)
    }

    fn pick(base: &Path, others: &[OtherStore]) -> (PickedSession, String) {
        let req = PickerRequest {
            sessions_root: base.join("own").join("projects"),
            others,
            project_slugs: &["C--src-p".to_owned()],
            project_name: "p",
            style: Style { color: false, ascii: true },
            live_dirs: &[],
            preview: true,
        };
        let keys = crate::ui::parse_preview_keys("Enter").unwrap();
        let mut term = crate::ui::PreviewTerm::new((100, 30), keys, true);
        let got = run_picker(&mut term, &req).unwrap().unwrap();
        let frame = term.frames.unwrap_or_default().concat().join("\n");
        (got, frame)
    }

    #[test]
    fn sharing_off_lists_another_accounts_session_by_name_and_transfers_its_pick() {
        let (base, others) = two_stores("off");
        let (got, frame) = pick(&base, &others);
        assert_eq!(got.session_id, "theirs", "the newest row, from the other store");
        assert!(frame.contains("[other]"), "the row names its account:\n{frame}");
        assert!(frame.contains("2 sessions"), "both stores are listed:\n{frame}");
        let t = got.transfer.unwrap();
        assert_eq!(t.from_account, "other");
        assert!(t.source.starts_with(base.join("other")));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_launching_account_without_a_projects_folder_still_lists_the_others() {
        let (base, others) = two_stores("noown");
        std::fs::remove_dir_all(base.join("own").join("projects")).unwrap();
        let (got, frame) = pick(&base, &others);
        assert_eq!(got.session_id, "theirs", "AC29: the other account's session is offered");
        assert!(frame.contains("1 sessions"), "{frame}");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn sharing_on_leaves_the_picker_as_it_was() {
        let (base, _) = two_stores("on");
        let (got, frame) = pick(&base, &[]);
        assert_eq!(got.session_id, "mine");
        assert_eq!(got.transfer, None);
        assert!(!frame.contains("[other]"), "{frame}");
        assert!(frame.contains("1 sessions"), "{frame}");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
