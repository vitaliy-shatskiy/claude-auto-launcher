//! Sessions and the session picker (`Sessions.ps1`, `Invoke-SessionPicker`, `Get-PickerFrame`,
//! `Expand-SessionPage`, the frame memo). The entry points `app.rs` calls; the work lives in
//! `sessions/`: `transcript` (records), `listing` (summaries, snapshot, cache), `frame` (the
//! picker frame and memo), `picker` (the screen), `holder` (another live claude on a session).

pub mod frame;
pub mod holder;
pub mod listing;
pub mod picker;
pub mod transcript;
mod win;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{Config, PathCtx};
use crate::frame::Style;
use crate::ui::{Term, UiStop};

use self::frame::Clock;
use self::listing::{Scope, SessionCache, cache_path, session_files};
use self::picker::PickerScreen;

/// Rows per page: the first frame costs one page of summarising.
const PAGE_SIZE: usize = 10;

/// What the picker returns: the session to resume, and whether to fork it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedSession {
    pub session_id: String,
    pub fork: bool,
}

/// Everything the picker is opened with.
#[derive(Debug, Clone)]
pub struct PickerRequest<'a> {
    pub sessions_root: PathBuf,
    /// Every slug of the chosen project; empty = the whole account.
    pub project_slugs: &'a [String],
    pub project_name: &'a str,
    pub style: Style,
    pub preview: bool,
}

/// `Get-SessionsRoot` with its inputs explicit: sharing on = one root for every account (the
/// `CLAUDE_AUTO_PROJECTS_ROOT` override, else `~\.claude\projects`); off = the account's own.
pub fn sessions_root_from(cfg: &Config, account: &str, projects_override: Option<&str>, home: &Path) -> PathBuf {
    if cfg.sharing {
        return match projects_override {
            Some(o) if !o.is_empty() => PathBuf::from(o),
            _ => home.join(".claude").join("projects"),
        };
    }
    // An unknown account (PowerShell throws) degrades to the canonical root: the launch goes on.
    cfg.accounts
        .iter()
        .find(|a| a.key.eq_ignore_ascii_case(account))
        .or_else(|| cfg.canonical())
        .map_or_else(|| home.join(".claude").join("projects"), |a| Path::new(&a.root).join("projects"))
}

/// `Get-SessionsRoot`.
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
        offset_minutes: crate::sys::local_now().map_or(0, |t| t.offset_minutes),
    }
}

/// `Invoke-SessionPicker` over paged, snapshotted lists: the file list is taken once per scope
/// and every page skips over THAT list. `Ok(None)` = Esc.
pub fn run_picker(term: &mut dyn Term, req: &PickerRequest<'_>) -> Result<Option<PickedSession>, UiStop> {
    let root = req.sessions_root.clone();
    let cache_file = cache_path(&root);
    let mut cache = SessionCache::new(req.preview);
    let mut snapshots: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut fetch = |have: usize, slugs: &[String]| {
        let snap = snapshots.entry(slugs.join("|")).or_insert_with(|| session_files(&root, slugs));
        cache.list(&root, Scope::Files(snap), have, PAGE_SIZE, &cache_file)
    };
    let slugs: Vec<String> = req.project_slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
    let first = fetch(0, &slugs);
    let mut screen = PickerScreen::new(first, Some(&mut fetch), PAGE_SIZE, &slugs, req.project_name, req.style, clock_now());
    let picked = crate::ui::screen_loop::run(&mut screen, term)?;
    Ok(picked.map(|p| PickedSession {
        session_id: p.session.session_id,
        fork: p.fork,
    }))
}

/// `Get-ContinueSessionHolder`: the warning line for the claude.exe holding the session
/// `claude -c` would attach to, or `None`. Any failure is `None`.
pub fn continue_session_holder(sessions_dirs: &[PathBuf], slugs: &[String], projects_root: &Path) -> Option<String> {
    let id = holder::continue_session_id(projects_root, slugs, sessions_dirs);
    session_holder(sessions_dirs, &id)
}

/// `Get-ClaudeSessionHolder`: the warning line for the claude.exe holding this session id.
pub fn session_holder(sessions_dirs: &[PathBuf], session_id: &str) -> Option<String> {
    let h = holder::find_holder(sessions_dirs, session_id)?;
    let offset = crate::sys::local_now().map_or(0, |t| t.offset_minutes);
    Some(holder::warning(&h, offset))
}

/// `Format-HeldSessionWarning`: the holder functions above already return the formatted line.
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
        assert!(base.join("second").join("claude-auto-rs-sessions.json").exists());
        assert!(!base.join("work").join("claude-auto-rs-sessions.json").exists());
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
            project_slugs: &["C--src-p".to_owned()],
            project_name: "p",
            style: Style { color: false, ascii: true },
            preview: true,
        };
        let keys = crate::ui::parse_preview_keys("Enter").unwrap();
        let mut term = crate::ui::PreviewTerm::new((100, 30), keys, true);
        let got = run_picker(&mut term, &req).unwrap().unwrap();
        assert_eq!(got.session_id, "s1");
        assert!(!base.join("claude-auto-rs-sessions.json").exists(), "preview is side-effect-free");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
