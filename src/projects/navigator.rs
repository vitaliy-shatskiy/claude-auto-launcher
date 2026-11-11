//! The in-screen path navigator behind the project screen's `enter a path...` row: a text field,
//! the subfolders of the typed parent under it, Tab completion, and the create-folder confirm
//! (D5). A network folder is listed only on Tab, on a worker thread with a time limit.

use std::collections::HashMap;
use std::os::windows::fs::MetadataExt;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

use super::paths::{DriveCache, path_present_with};
use super::win::{self, DriveClass};

/// Rows a listing shows at most; a bigger folder is narrowed by typing.
const SHOWN_CAP: usize = 200;
/// Folders one listing reads at most.
const READ_CAP: usize = 10_000;
/// How long a network listing may take before the navigator stops waiting for it.
pub(crate) const NETWORK_LIMIT: Duration = Duration::from_secs(1);

/// Lists the subfolder names of a folder; `Err` says why it could not.
pub(crate) type Lister = fn(&str) -> Listing;
type Listing = Result<Vec<String>, String>;

const FULL_PATH_HINT: &str = r"type a full path (C:\... or \\server\share\)";
/// Preview writes nothing: the create question's yes only says so, and the navigator stays.
const PREVIEW_NOTE: &str = "preview - nothing was created";

/// One navigator row, in cursor order: use, up, then the folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NavRow {
    /// Start in the typed folder.
    Use(String),
    /// One level up.
    Up,
    /// A subfolder of the typed parent, or a drive (`C:`) when the field holds no parent.
    Folder(String),
}

/// What a navigator gesture asks of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NavStep {
    Stay,
    /// Start in this folder (absolute).
    Pick(String),
}

/// The navigator's state; the screen's cursor index picks among `rows`.
#[derive(Debug)]
pub(crate) struct Navigator {
    pub text: String,
    /// The text the rows were last built from.
    shown: String,
    /// One gesture's answer, cleared by the next key.
    pub notice: String,
    /// What the list cannot show: a network folder not listed yet, a listing error.
    pub hint: String,
    /// D5: the folder Enter creates; Esc drops the question.
    pub confirm: Option<String>,
    /// Preview mode: `create` never touches the disk.
    pub preview: bool,
    pub rows: Vec<NavRow>,
    drives: DriveCache,
    roots: Option<Vec<String>>,
    /// The last listing, keyed by its parent as typed.
    listed: Option<(String, Result<Vec<String>, String>)>,
    /// Tab's candidates for one parent and the one shown, so a second Tab cycles.
    tab: Option<(String, Vec<String>, usize)>,
    /// Network listings still running, one worker per parent as typed.
    pending: HashMap<String, Receiver<Listing>>,
    lister: Lister,
    limit: Duration,
    classify: fn(char) -> DriveClass,
}

/// The text as a path: a pasted path's quotes dropped, `/` as `\`.
fn clean(text: &str) -> String {
    text.trim().trim_matches('"').replace('/', "\\")
}

/// (the parent with its trailing `\`, the last segment); no `\` = no parent.
fn split(t: &str) -> (&str, &str) {
    match t.rfind('\\') {
        Some(i) => (t.get(..=i).unwrap_or_default(), t.get(i + 1..).unwrap_or_default()),
        None => ("", t),
    }
}

/// The folder above `parent` (`C:\a\b\` -> `C:\a\`); above a root, the drive list.
fn up_of(parent: &str) -> String {
    let p = parent.trim_end_matches('\\');
    p.rfind('\\').and_then(|i| p.get(..=i)).map_or_else(String::new, str::to_owned)
}

/// What the use row starts in: the text without its trailing `\`, but a drive root keeps it.
fn target(t: &str) -> String {
    let s = t.trim_end_matches('\\');
    if s.len() == 2 && s.ends_with(':') {
        format!("{s}\\")
    } else {
        s.to_owned()
    }
}

/// The names that start with the segment, then those holding it, case-insensitive.
fn matching(names: &[String], seg: &str) -> Vec<String> {
    let seg = seg.to_lowercase();
    let (starts, holds): (Vec<&String>, Vec<&String>) = names
        .iter()
        .filter(|n| n.to_lowercase().contains(&seg))
        .partition(|n| n.to_lowercase().starts_with(&seg));
    starts.into_iter().chain(holds).cloned().collect()
}

/// The real lister: subfolders (junctions and folder links included, never followed; a link to
/// a file is not one), an entry that cannot be read skipped, sorted case-insensitively.
pub(crate) fn list_dirs(parent: &str) -> Listing {
    let entries = std::fs::read_dir(parent).map_err(|e| format!("cannot list it: {}", e.kind()))?;
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        // On Windows the entry's metadata comes from the directory read itself: no disk access.
        .filter(|e| e.metadata().is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0))
        .filter_map(|e| e.file_name().into_string().ok())
        .take(READ_CAP)
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    Ok(names)
}

/// Lists on a worker thread: a share that never answers keeps its own detached thread, never
/// the screen.
fn spawn_listing(parent: &str, lister: Lister) -> Result<Receiver<Listing>, String> {
    let (tx, rx) = mpsc::channel();
    let p = parent.to_owned();
    std::thread::Builder::new()
        .name("claude-auto-list".into())
        // A send fails only when the screen dropped the navigator: nothing is left to tell.
        .spawn(move || tx.send(lister(&p)).unwrap_or_default())
        .map_err(|e| format!("cannot list it: {e}"))?;
    Ok(rx)
}

/// `X:\...` or `\\...`: anything else (`\a`, `a\`, `C:a`) would be read against the launcher's
/// own folder, on the screen's thread.
fn full_path(parent: &str) -> bool {
    let mut c = parent.chars();
    parent.starts_with(r"\\") || matches!((c.next(), c.next(), c.next()), (Some(l), Some(':'), Some('\\')) if l.is_ascii_alphabetic())
}

impl Navigator {
    pub(crate) fn new(lister: Lister, limit: Duration) -> Self {
        Self {
            text: String::new(),
            shown: String::new(),
            notice: String::new(),
            hint: String::new(),
            confirm: None,
            preview: false,
            rows: Vec::new(),
            drives: DriveCache::new(),
            roots: None,
            listed: None,
            tab: None,
            pending: HashMap::new(),
            lister,
            limit,
            classify: win::drive_class,
        }
    }

    /// A UNC path, or one on a drive classed network: never probed, never created.
    fn network(&mut self, path: &str) -> bool {
        if path.starts_with(r"\\") {
            return true;
        }
        let mut c = path.chars();
        match (c.next(), c.next()) {
            (Some(l), Some(':')) if l.is_ascii_alphabetic() => {
                let key = l.to_ascii_lowercase();
                *self.drives.entry(key).or_insert_with(|| (self.classify)(key)) == DriveClass::Skip
            }
            _ => false,
        }
    }

    /// The drives present (`C:`), network ones included: the drive table only, no disk access.
    fn roots(&mut self) -> Vec<String> {
        if self.roots.is_none() {
            let classify = self.classify;
            let found = ('a'..='z')
                .filter(|l| *self.drives.entry(*l).or_insert_with(|| classify(*l)) != DriveClass::Absent)
                .map(|l| format!("{}:", l.to_ascii_uppercase()))
                .collect();
            self.roots = Some(found);
        }
        self.roots.clone().unwrap_or_default()
    }

    /// The parent's folder names. A local parent is listed at once; a network one only when
    /// `tab` asks, and stays as listed until the parent changes.
    fn listing(&mut self, parent: &str, tab: bool) -> Vec<String> {
        if parent.is_empty() {
            return self.roots();
        }
        if let Some((p, r)) = &self.listed
            && p == parent
        {
            if let Err(e) = r {
                e.clone_into(&mut self.hint);
            }
            return r.clone().unwrap_or_default();
        }
        if !full_path(parent) {
            FULL_PATH_HINT.clone_into(&mut self.hint);
            return Vec::new();
        }
        let network = self.network(parent);
        let r = if network {
            let Some(r) = self.network_listing(parent, tab) else {
                return Vec::new();
            };
            r
        } else {
            (self.lister)(parent)
        };
        // A network folder that did not answer is asked again on the next Tab.
        if !network || r.is_ok() {
            self.listed = Some((parent.to_owned(), r.clone()));
        }
        if let Err(e) = &r {
            e.clone_into(&mut self.hint);
        }
        r.unwrap_or_default()
    }

    /// A network parent's listing: the worker already asked for it is checked first, so a late
    /// answer is used and a parent never has two; a new worker starts only on Tab, and only Tab
    /// waits (`limit`). `None` = nothing to show yet.
    fn network_listing(&mut self, parent: &str, tab: bool) -> Option<Listing> {
        if !self.pending.contains_key(parent) {
            if !tab {
                "a network folder: tab lists it".clone_into(&mut self.hint);
                return None;
            }
            match spawn_listing(parent, self.lister) {
                Ok(rx) => self.pending.insert(parent.to_owned(), rx),
                Err(e) => return Some(Err(e)),
            };
        }
        let wait = if tab { self.limit } else { Duration::ZERO };
        let r = match self.pending.get(parent)?.recv_timeout(wait) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) if tab => return Some(Err(format!("no answer in {} ms", wait.as_millis()))),
            Err(RecvTimeoutError::Timeout) => {
                "still listing: tab waits for it".clone_into(&mut self.hint);
                return None;
            }
            Err(RecvTimeoutError::Disconnected) => Err("the listing failed".into()),
        };
        self.pending.remove(parent);
        Some(r)
    }

    /// Where the cursor parks after the text changed: on the use row when the segment is empty
    /// or names a listed folder exactly, else on the first folder it matches, else on use.
    pub(crate) fn park(&self) -> usize {
        let t = clean(&self.text);
        let seg = split(&t).1.to_lowercase();
        if seg.is_empty() {
            return 0;
        }
        let has_use = matches!(self.rows.first(), Some(NavRow::Use(_)));
        let exact = self
            .rows
            .iter()
            .position(|r| matches!(r, NavRow::Folder(n) if n.to_lowercase() == seg));
        match exact {
            Some(_) if has_use => 0,
            Some(i) => i,
            None => self.rows.iter().position(|r| matches!(r, NavRow::Folder(_))).unwrap_or(0),
        }
    }

    /// Rebuilds the rows from the text; runs before every frame. `true` when the text changed
    /// since the last call: the cursor parks again.
    pub(crate) fn refresh(&mut self) -> bool {
        let changed = self.text != self.shown;
        self.shown.clone_from(&self.text);
        self.hint.clear();
        let t = clean(&self.text);
        let (parent, seg) = split(&t);
        let mut rows = Vec::new();
        if !parent.is_empty() {
            rows.push(NavRow::Use(target(&t)));
            rows.push(NavRow::Up);
        }
        let names = self.listing(parent, false);
        rows.extend(matching(&names, seg).into_iter().take(SHOWN_CAP).map(NavRow::Folder));
        self.rows = rows;
        changed
    }

    fn edited(&mut self) {
        self.tab = None;
        self.confirm = None;
    }

    pub(crate) fn type_char(&mut self, c: char) {
        self.edited();
        self.text.push(c);
    }

    /// Backspace drops a character, or goes up on an empty segment; with Ctrl it drops the whole
    /// segment first.
    pub(crate) fn backspace(&mut self, ctrl: bool) {
        self.edited();
        let t = clean(&self.text);
        let (parent, seg) = split(&t);
        self.text = if seg.is_empty() {
            up_of(parent)
        } else if ctrl {
            parent.to_owned()
        } else {
            let mut s = t.clone();
            s.pop();
            s
        };
    }

    pub(crate) fn up(&mut self) {
        self.edited();
        let t = clean(&self.text);
        self.text = up_of(split(&t).0);
    }

    /// Enter (or a second click) on a row: use, up, or into a folder.
    pub(crate) fn enter(&mut self, index: usize) -> NavStep {
        if self.confirm.is_some() {
            return self.create();
        }
        match self.rows.get(index).cloned() {
            Some(NavRow::Use(t)) => self.use_path(&t),
            Some(NavRow::Up) => {
                self.up();
                NavStep::Stay
            }
            Some(NavRow::Folder(n)) => {
                self.open(&n);
                NavStep::Stay
            }
            None => NavStep::Stay,
        }
    }

    /// Right: into the folder under the cursor; on any other row nothing.
    pub(crate) fn right(&mut self, index: usize) {
        if let Some(NavRow::Folder(n)) = self.rows.get(index).cloned() {
            self.open(&n);
        }
    }

    fn open(&mut self, name: &str) {
        self.edited();
        let t = clean(&self.text);
        self.text = format!("{}{name}\\", split(&t).0);
    }

    /// Tab completes the segment from the parent's folders: one candidate is entered, several
    /// are shown in turn by the next Tabs. A network parent is listed here, and only here.
    pub(crate) fn tab(&mut self) {
        self.confirm = None;
        self.hint.clear();
        let t = clean(&self.text);
        let (parent, seg) = split(&t);
        let parent = parent.to_owned();
        if let Some((p, cands, i)) = self.tab.as_mut()
            && *p == parent
            && !cands.is_empty()
        {
            *i = (*i + 1) % cands.len();
            self.text = format!("{parent}{}", cands.get(*i).map_or("", String::as_str));
            return;
        }
        let cands = matching(&self.listing(&parent, true), seg);
        if !self.hint.is_empty() {
            // A listing error or a share that did not answer: shown until the next key.
            self.notice.clone_from(&self.hint);
            return;
        }
        match cands.as_slice() {
            [] => "no folder matches".clone_into(&mut self.notice),
            [one] => self.text = format!("{parent}{one}\\"),
            [first, ..] => {
                self.text = format!("{parent}{first}");
                self.tab = Some((parent, cands, 0));
            }
        }
    }

    /// The use row: an existing folder is picked; a missing one under an existing local parent
    /// asks to be created (D5); a network path is picked unprobed and never created.
    fn use_path(&mut self, t: &str) -> NavStep {
        let full = std::path::absolute(t).map_or_else(|_| t.to_owned(), |p| p.to_string_lossy().into_owned());
        if !path_present_with(&full, &mut self.drives, self.classify) {
            if Path::new(&full).parent().is_some_and(Path::is_dir) {
                self.confirm = Some(full);
            } else {
                "path not found".clone_into(&mut self.notice);
            }
            return NavStep::Stay;
        }
        if self.network(&full) || Path::new(&full).is_dir() {
            return NavStep::Pick(full);
        }
        "not a folder".clone_into(&mut self.notice);
        NavStep::Stay
    }

    /// D5: one level, `create_dir`, never `create_dir_all`.
    fn create(&mut self) -> NavStep {
        let Some(path) = self.confirm.take() else { return NavStep::Stay };
        if self.preview {
            PREVIEW_NOTE.clone_into(&mut self.notice);
            return NavStep::Stay;
        }
        match std::fs::create_dir(&path) {
            Ok(()) => NavStep::Pick(path),
            Err(e) => {
                self.notice = format!("cannot create it: {}", e.kind());
                NavStep::Stay
            }
        }
    }

    /// Esc: drops the create question; `false` = nothing to drop, leave the navigator.
    pub(crate) fn escape(&mut self) -> bool {
        self.confirm.take().is_some()
    }

    /// The line under the field: the create question, a notice, or a hint.
    pub(crate) fn status(&self) -> String {
        match &self.confirm {
            Some(p) => format!("create {p}? enter yes / esc no"),
            None if !self.notice.is_empty() => self.notice.clone(),
            None => self.hint.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
