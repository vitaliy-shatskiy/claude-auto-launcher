//! The session picker screen: the scope and its
//! page buckets, the filter box (`filter`), paging on Down and the wheel, and what a chosen row means.

mod filter;
mod landed;

use std::collections::HashSet;

use serde_json::{Map, Value, json};

use self::filter::scope_name;
use super::chat::pane::{ChatKey, ChatPane, ChatState, Fill};
use super::chat::{ChatMessage, bounded};
use super::frame::{ChatView, Clock, Memo, PickerScope, PickerView, list_capacity, matches, picker_frame, resumable};
use super::listing::Session;
use crate::frame::{Frame, Style};
use crate::input::KeyPress;
use crate::ui::screen_loop::{Cursor, Flow, Screen};
use crate::ui::{Hit, Term};

/// Reads the ids of the sessions a running Claude Code has open (`holder::live_ids`).
pub type LiveScan<'a> = Box<dyn FnMut() -> HashSet<String> + 'a>;

/// Idle polls (~60 ms each) between two reads of the live set.
const LIVE_POLLS: u32 = 50;

/// The next page of a scope: called with the rows fetched so far and the scope's slugs.
pub type Fetch<'a> = &'a mut dyn FnMut(usize, &[String]) -> Vec<Session>;

/// One scope's sessions and its paging offset.
#[derive(Debug, Clone, Default)]
pub struct Bucket {
    pub sessions: Vec<Session>,
    /// Rows FETCHED so far (the snapshot offset), not the deduplicated count.
    pub fetched: usize,
    pub exhausted: bool,
}

/// Appends the next page, skipping rows already listed. An EMPTY page is
/// the end; a page of overlap only is not. Returns how many rows were added.
pub fn expand_page(bucket: &mut Bucket, fetch: Fetch<'_>, scope: &[String]) -> usize {
    let page = fetch(bucket.fetched, scope);
    bucket.fetched += page.len();
    bucket.exhausted = page.is_empty();
    let mut added = 0;
    for s in page {
        if !bucket.sessions.iter().any(|o| o.session_id == s.session_id && o.path == s.path) {
            bucket.sessions.push(s);
            added += 1;
        }
    }
    added
}

/// What the picker answers: the session and whether to fork it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    pub session: Session,
    pub fork: bool,
}

/// The picker's state around the screen loop.
pub struct PickerScreen<'a> {
    cursor: Cursor,
    filter: String,
    scope: PickerScope,
    has_scope: bool,
    slugs: Vec<String>,
    title: String,
    first_page: Vec<Session>,
    project: Bucket,
    all: Bucket,
    none: Bucket,
    started: [bool; 3],
    fetch: Option<Fetch<'a>>,
    fill_rows: usize,
    /// The rows the last frame had room for: idle polls load pages until they are filled.
    want: usize,
    pool: Vec<usize>,
    resumable_count: usize,
    items: Vec<usize>,
    can_page: bool,
    clock: Clock,
    style: Style,
    memo: Memo,
    /// Loads the highlighted session's chat off the UI thread; `None` shows the summary's
    /// recent messages instead (tests, a session from a plugin source).
    chat: Option<ChatPane>,
    /// The pane size the last frame had: what a stream fills.
    chat_fill: Fill,
    /// Full summaries for rows first shown quick (`summaries`); `None` = every row is final.
    landed: Option<std::sync::mpsc::Receiver<Vec<Session>>>,
    /// Reads the ids a live process has open: once at open, again every `LIVE_POLLS` idle polls.
    live_scan: Option<LiveScan<'a>>,
    live: HashSet<String>,
    live_polls: u32,
}

impl std::fmt::Debug for PickerScreen<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PickerScreen")
            .field("scope", &self.scope)
            .field("items", &self.items.len())
            .finish_non_exhaustive()
    }
}

fn slot(scope: PickerScope) -> usize {
    match scope {
        PickerScope::Project => 0,
        PickerScope::All => 1,
        PickerScope::None => 2,
    }
}

impl<'a> PickerScreen<'a> {
    pub fn new(
        first_page: Vec<Session>,
        fetch: Option<Fetch<'a>>,
        fill_rows: usize,
        slugs: &[String],
        title: &str,
        style: Style,
        clock: Clock,
    ) -> Self {
        let slugs: Vec<String> = slugs.iter().filter(|s| !s.is_empty()).cloned().collect();
        let has_scope = !slugs.is_empty() || !title.is_empty();
        let scope = if has_scope { PickerScope::Project } else { PickerScope::None };
        let open = Bucket {
            sessions: first_page.clone(),
            fetched: first_page.len(),
            exhausted: fetch.is_none(),
        };
        let mut s = Self {
            cursor: Cursor::at(0),
            filter: String::new(),
            scope,
            has_scope,
            slugs,
            title: title.to_owned(),
            first_page,
            project: Bucket::default(),
            all: Bucket::default(),
            none: Bucket::default(),
            started: [false; 3],
            fetch,
            fill_rows,
            want: 0,
            pool: Vec::new(),
            resumable_count: 0,
            items: Vec::new(),
            can_page: false,
            clock,
            style,
            memo: Memo::default(),
            chat: None,
            chat_fill: Fill { rows: 0, cols: 0 },
            landed: None,
            live_scan: None,
            live: HashSet::new(),
            live_polls: 0,
        };
        *s.bucket_mut() = open;
        s.started[slot(scope)] = true;
        s
    }

    /// The preview pane shows the chat `pane` loads.
    #[must_use]
    pub fn with_chat(mut self, pane: ChatPane) -> Self {
        self.chat = Some(pane);
        self
    }

    /// Rows a running process has open get the green dot; `scan` runs now and on idle refreshes.
    #[must_use]
    pub fn with_live(mut self, mut scan: LiveScan<'a>) -> Self {
        self.live = scan();
        self.live_scan = Some(scan);
        self
    }

    /// A re-read of the live set every `LIVE_POLLS` idle polls; `true` when it changed.
    fn refresh_live(&mut self) -> bool {
        self.live_polls += 1;
        let Some(scan) = self.live_scan.as_mut().filter(|_| self.live_polls >= LIVE_POLLS) else {
            return false;
        };
        self.live_polls = 0;
        let now = scan();
        let changed = now != self.live;
        self.live = now;
        changed
    }

    /// Rows shown quick take their full summaries from `landed` as the worker sends them.
    #[must_use]
    pub fn with_landed(mut self, landed: std::sync::mpsc::Receiver<Vec<Session>>) -> Self {
        self.landed = Some(landed);
        self
    }

    /// The chat key of the row at `item` (an index into `items`); `None` without a transcript.
    fn chat_key(&self, item: usize) -> Option<ChatKey> {
        let s = self.bucket().sessions.get(*self.items.get(item)?)?;
        (!s.path.as_os_str().is_empty()).then(|| ChatKey {
            path: s.path.clone(),
            size: s.size_bytes,
            modified_ms: s.modified_ms,
        })
    }

    /// The highlighted session's chat from the pane (streamed so far or loading); `None` when
    /// there is no pane or no transcript, and the summary's recent messages stand in.
    fn chat_state(&mut self, size: (usize, usize)) -> Option<ChatState> {
        let key = self.chat_key(usize::try_from(self.cursor.index).ok()?)?;
        self.chat_fill = Fill {
            rows: size.1,
            cols: size.0 / 2,
        };
        let fill = self.chat_fill;
        Some(self.chat.as_mut()?.view(&key, fill))
    }

    /// The sessions the pane prefetches: 2 either side of the cursor, then the newest 5.
    fn prefetch_keys(&self) -> Vec<ChatKey> {
        let at = usize::try_from(self.cursor.index).unwrap_or(0);
        let near = [at + 1, at.wrapping_sub(1), at + 2, at.wrapping_sub(2)];
        let mut keys: Vec<ChatKey> = Vec::new();
        for i in near.into_iter().chain(0..5) {
            if let Some(k) = self.chat_key(i)
                && !keys.contains(&k)
            {
                keys.push(k);
            }
        }
        keys
    }
    fn recent(&self) -> Vec<ChatMessage> {
        let Some(i) = usize::try_from(self.cursor.index).ok() else {
            return Vec::new();
        };
        let at = self.items.get(i).copied();
        at.and_then(|at| self.bucket().sessions.get(at))
            .map(|s| {
                s.recent
                    .iter()
                    .map(|m| ChatMessage {
                        speaker: m.speaker,
                        text: bounded(&m.text),
                        at_ms: None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn bucket(&self) -> &Bucket {
        match self.scope {
            PickerScope::Project => &self.project,
            PickerScope::All => &self.all,
            PickerScope::None => &self.none,
        }
    }

    fn bucket_mut(&mut self) -> &mut Bucket {
        match self.scope {
            PickerScope::Project => &mut self.project,
            PickerScope::All => &mut self.all,
            PickerScope::None => &mut self.none,
        }
    }

    fn scope_slugs(&self) -> Vec<String> {
        if self.scope == PickerScope::Project {
            self.slugs.clone()
        } else {
            Vec::new()
        }
    }

    /// The bucket's rows this scope shows, before the resumable drop.
    fn pool_of(&self) -> Vec<usize> {
        let b = self.bucket();
        (0..b.sessions.len())
            .filter(|&i| {
                let s = &b.sessions[i];
                if self.scope != PickerScope::Project || !self.has_scope {
                    true
                } else if !self.slugs.is_empty() {
                    self.slugs.iter().any(|x| x.eq_ignore_ascii_case(&s.slug))
                } else {
                    s.project.eq_ignore_ascii_case(&self.title)
                }
            })
            .collect()
    }

    fn expand(&mut self) -> usize {
        let scope = self.scope_slugs();
        let Some(fetch) = self.fetch.as_mut() else { return 0 };
        let bucket = match self.scope {
            PickerScope::Project => &mut self.project,
            PickerScope::All => &mut self.all,
            PickerScope::None => &mut self.none,
        };
        expand_page(bucket, &mut **fetch, &scope)
    }

    /// Loads pages until the scope holds `want` resumable rows or the fetcher runs out.
    fn fill_to(&mut self, want: usize) -> usize {
        let mut total = 0;
        while self.fetch.is_some() && !self.bucket().exhausted {
            let b = self.bucket();
            if self.pool_of().iter().filter(|&&i| resumable(&b.sessions[i])).count() >= want {
                break;
            }
            total += self.expand();
        }
        total
    }

    fn step_down(&mut self) {
        if usize::try_from(self.cursor.index).is_ok_and(|i| i + 1 < self.items.len()) {
            self.cursor.index += 1;
            return;
        }
        if !self.can_page {
            return;
        }
        let mut added = self.expand();
        if self.fill_rows > 0 {
            added += self.fill_to(self.resumable_count + 1);
        }
        if added > 0 && !self.items.is_empty() {
            self.cursor.index += 1;
        }
    }

    fn take(&self, fork: bool) -> Option<Picked> {
        let i = usize::try_from(self.cursor.index).ok()?;
        let at = *self.items.get(i)?;
        Some(Picked {
            session: self.bucket().sessions.get(at)?.clone(),
            fork,
        })
    }

    fn chosen(&self, fork: bool) -> Flow<Picked> {
        self.take(fork).map_or(Flow::Continue, |p| Flow::Done(Some(p)))
    }
}

impl Screen for PickerScreen<'_> {
    type Output = Picked;

    fn name(&self) -> &'static str {
        "picker"
    }

    fn cursor(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    fn draw(&mut self, size: (usize, usize)) -> Frame {
        let mut memo = std::mem::take(&mut self.memo);
        let state = self.chat_state(size);
        let fallback = if state.is_none() { self.recent() } else { Vec::new() };
        let chat = match &state {
            Some(ChatState::Loading) => ChatView::Loading,
            Some(ChatState::Ready(m)) => ChatView::Messages(m),
            Some(ChatState::Updating(m)) => ChatView::Updating(m),
            None => ChatView::Messages(&fallback),
        };
        let b = self.bucket();
        let pool: Vec<&Session> = self.pool.iter().filter_map(|&i| b.sessions.get(i)).collect();
        let v = PickerView {
            sessions: &pool,
            index: usize::try_from(self.cursor.index).unwrap_or(0),
            filter: &self.filter,
            width: size.0,
            height: size.1,
            clock: self.clock,
            style: self.style,
            scope: self.scope,
            project_name: &self.title,
            hover: self.cursor.hover,
            hover_row: self.cursor.hover_row,
            chat,
        };
        let f = picker_frame(&v, &mut memo);
        self.memo = memo;
        self.want = list_capacity(size.0, size.1, self.scope, self.style);
        f
    }

    /// AC27: a tall terminal lists more than the first page. One page per idle poll, so the
    /// first frame never waits for the pages a big window wants.
    fn idle(&mut self) -> bool {
        // The chat first: collecting a finished load is cheap, a page of summaries is not.
        let keys = if self.chat.is_some() { self.prefetch_keys() } else { Vec::new() };
        let fill = self.chat_fill;
        if let Some(pane) = self.chat.as_mut() {
            pane.prefetch(keys, fill);
        }
        if self.chat.as_mut().is_some_and(ChatPane::idle) || self.take_landed() || self.refresh_live() {
            return true;
        }
        if self.fetch.is_none() || self.bucket().exhausted || self.resumable_count >= self.want {
            return false;
        }
        self.expand();
        true
    }

    fn before(&mut self) {
        let k = slot(self.scope);
        if !self.started[k] {
            self.started[k] = true;
            *self.bucket_mut() = if self.fetch.is_some() {
                Bucket::default()
            } else {
                Bucket {
                    sessions: self.first_page.clone(),
                    fetched: self.first_page.len(),
                    exhausted: true,
                }
            };
            if self.fetch.is_some() {
                self.expand();
            }
        }
        if self.fill_rows > 0 {
            self.fill_to(self.fill_rows);
        }
        let bucket = match self.scope {
            PickerScope::Project => &mut self.project,
            PickerScope::All => &mut self.all,
            PickerScope::None => &mut self.none,
        };
        for s in &mut bucket.sessions {
            s.live = self.live.contains(&s.session_id);
        }
        self.pool = self.pool_of();
        let b = self.bucket();
        let resumable_rows: Vec<usize> = self.pool.iter().copied().filter(|&i| resumable(&b.sessions[i])).collect();
        let items: Vec<usize> = resumable_rows
            .iter()
            .copied()
            .filter(|&i| matches(&b.sessions[i], &self.filter))
            .collect();
        self.can_page = self.fetch.is_some() && !b.exhausted && (!items.is_empty() || resumable_rows.is_empty());
        self.resumable_count = resumable_rows.len();
        self.items = items;
        let n = i32::try_from(self.items.len()).unwrap_or(i32::MAX);
        if self.cursor.index >= n {
            self.cursor.index = (n - 1).max(0);
        }
    }

    fn rows(&self) -> Option<i32> {
        Some(i32::try_from(self.items.len()).unwrap_or(i32::MAX))
    }

    fn down(&mut self) -> bool {
        self.step_down();
        true
    }

    fn wheel(&mut self, delta: i32) -> bool {
        if delta < 0 {
            if self.cursor.index > 0 {
                self.cursor.index -= 1;
            }
        } else {
            self.step_down();
        }
        true
    }

    fn enter(&mut self) -> Option<Flow<Picked>> {
        Some(self.chosen(false))
    }

    fn tab(&mut self) -> Option<Flow<Picked>> {
        if !self.has_scope {
            return None;
        }
        self.scope = if self.scope == PickerScope::Project {
            PickerScope::All
        } else {
            PickerScope::Project
        };
        self.cursor.index = 0;
        Some(Flow::Continue)
    }

    fn hotkeys(&self) -> &[char] {
        &['f', '/']
    }

    fn hotkey(&mut self, c: char) -> Flow<Picked> {
        if c == 'f' {
            return self.chosen(true);
        }
        self.cursor.typing = true;
        Flow::Continue
    }

    fn on_key(&mut self, key: &KeyPress, term: &mut dyn Term) -> bool {
        if !self.cursor.typing {
            return false;
        }
        self.type_filter(key, term);
        true
    }

    fn click(&mut self, hit: &Hit) -> Flow<Picked> {
        if let Hit::ListRow(r) = hit
            && usize::try_from(*r).is_ok_and(|r| r < self.items.len())
        {
            self.cursor.index = *r;
        }
        Flow::Continue
    }

    fn activate(&mut self, _hit: &Hit) -> Option<Flow<Picked>> {
        self.take(false).map(|p| Flow::Done(Some(p)))
    }

    fn log_fields(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("scope".into(), json!(scope_name(self.scope)));
        m
    }
}

#[cfg(test)]
mod tests;
