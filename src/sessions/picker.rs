//! The session picker screen (`Invoke-SessionPicker`, `Expand-SessionPage`): the scope and its
//! page buckets, the filter box, paging on Down and the wheel, and what a chosen row means.

use serde_json::{Map, Value, json};

use super::frame::{Clock, Memo, PickerScope, PickerView, matches, picker_frame, resumable};
use super::listing::Session;
use crate::frame::{Frame, Style};
use crate::input::{KeyPress, vk};
use crate::ui::screen_loop::{Cursor, Flow, Screen};
use crate::ui::{Hit, Term};

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

/// `Expand-SessionPage`: appends the next page, skipping rows already listed. An EMPTY page is
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
    pool: Vec<usize>,
    resumable_count: usize,
    items: Vec<usize>,
    can_page: bool,
    clock: Clock,
    style: Style,
    memo: Memo,
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
            pool: Vec::new(),
            resumable_count: 0,
            items: Vec::new(),
            can_page: false,
            clock,
            style,
            memo: Memo::default(),
        };
        *s.bucket_mut() = open;
        s.started[slot(scope)] = true;
        s
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

    fn log_typing(&mut self, term: &dyn Term, fields: &[(&str, Value)]) {
        let mut d = Map::new();
        d.insert("screen".into(), json!("picker"));
        for (k, v) in fields {
            d.insert((*k).into(), v.clone());
        }
        d.insert("scope".into(), json!(scope_name(self.scope)));
        term.logger().write("key", d);
    }

    /// The filter box owns every key while open; only its two closes are logged, never the text.
    fn type_filter(&mut self, key: &KeyPress, term: &dyn Term) {
        if key.is(vk::ENTER) {
            self.cursor.typing = false;
            let n = self.filter.chars().count();
            self.log_typing(
                term,
                &[("key", json!("Enter")), ("filter", json!("close")), ("filterLength", json!(n))],
            );
        } else if key.is(vk::ESCAPE) || key.is_ctrl(vk::C) {
            let n = self.filter.chars().count();
            let index = self.cursor.index;
            self.log_typing(
                term,
                &[
                    ("key", json!("Escape")),
                    ("filter", json!("clear")),
                    ("filterLength", json!(n)),
                    ("index", json!(index)),
                ],
            );
            self.cursor.typing = false;
            self.filter.clear();
            self.cursor.index = 0;
        } else if key.is(vk::BACKSPACE) {
            self.filter.pop();
        } else if key.ch.is_alphanumeric() || key.ch == ' ' || key.ch == '-' {
            self.filter.push(key.ch);
            self.cursor.index = 0;
        }
    }
}

fn scope_name(s: PickerScope) -> &'static str {
    match s {
        PickerScope::None => "none",
        PickerScope::Project => "project",
        PickerScope::All => "all",
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
        };
        let f = picker_frame(&v, &mut memo);
        self.memo = memo;
        f
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
