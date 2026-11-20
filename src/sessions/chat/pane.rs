//! The chat pane's state behind the picker: one worker thread loads the highlighted session's
//! chat, debounced (two idle polls after the cursor rests), and a small cache keyed by the
//! transcript's size and mtime. The UI thread never reads a transcript: `view` answers from the
//! cache or says loading, `idle` collects what the worker finished.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use super::{ChatLoad, ChatMessage};
use crate::timing::Timings;

/// What loads one chat: `load_chat` in the app, a stub in tests. Runs on the worker thread.
pub type Loader = Arc<dyn Fn(&Path, usize) -> ChatLoad + Send + Sync>;

/// A transcript as last seen: a grown or rewritten file is another key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChatKey {
    pub path: PathBuf,
    pub size: u64,
    pub modified_ms: i64,
}

/// What the pane shows for the highlighted session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatState {
    Loading,
    Ready(Arc<Vec<ChatMessage>>),
}

struct Reply {
    key: ChatKey,
    load: ChatLoad,
    start: Instant,
    end: Instant,
}

/// Idle polls the cursor must rest before a load starts (~60 ms each on the console).
const REST_POLLS: u8 = 2;
const CACHE_SIZE: usize = 32;

pub struct ChatPane {
    requests: Sender<(ChatKey, usize)>,
    replies: Receiver<Reply>,
    cache: VecDeque<(ChatKey, Arc<Vec<ChatMessage>>)>,
    wanted: Option<(ChatKey, usize)>,
    rests: u8,
    in_flight: Option<ChatKey>,
    timings: Timings,
}

impl std::fmt::Debug for ChatPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatPane")
            .field("cached", &self.cache.len())
            .field("in_flight", &self.in_flight)
            .finish_non_exhaustive()
    }
}

impl ChatPane {
    /// Starts the worker. It ends once the pane is dropped and its current load returns.
    pub fn new(loader: Loader, timings: Timings) -> Self {
        let (requests, work) = channel::<(ChatKey, usize)>();
        let (done, replies) = channel::<Reply>();
        std::thread::spawn(move || {
            while let Ok(mut job) = work.recv() {
                // Only the newest request matters: the cursor has moved past the others.
                while let Ok(newer) = work.try_recv() {
                    job = newer;
                }
                let start = Instant::now();
                let load = loader(&job.0.path, job.1);
                let reply = Reply {
                    key: job.0,
                    load,
                    start,
                    end: Instant::now(),
                };
                if done.send(reply).is_err() {
                    break;
                }
            }
        });
        Self {
            requests,
            replies,
            cache: VecDeque::new(),
            wanted: None,
            rests: 0,
            in_flight: None,
            timings,
        }
    }

    /// The chat for `key` from the cache, or `Loading` (and `key` becomes the one to load).
    /// `want` is how many messages would fill the pane.
    pub fn view(&mut self, key: &ChatKey, want: usize) -> ChatState {
        if let Some(i) = self.cache.iter().position(|(k, _)| k == key)
            && let Some(hit) = self.cache.remove(i)
        {
            let msgs = Arc::clone(&hit.1);
            self.cache.push_back(hit);
            return ChatState::Ready(msgs);
        }
        if self.wanted.as_ref().is_none_or(|(k, _)| k != key) {
            self.wanted = Some((key.clone(), want));
            self.rests = 0;
        }
        ChatState::Loading
    }

    /// One idle poll: takes finished loads, starts the wanted one once the cursor rested.
    /// `true` exactly when the wanted chat arrived (the pane needs one redraw).
    pub fn idle(&mut self) -> bool {
        let mut arrived = false;
        while let Ok(r) = self.replies.try_recv() {
            if self.in_flight.as_ref() == Some(&r.key) {
                self.in_flight = None;
            }
            let id = self.timings.add(&["picker.chat.load"], None, r.start, r.end, None);
            self.timings.set_field(id, "bytes", r.load.bytes_read);
            arrived |= self.wanted.as_ref().is_some_and(|(k, _)| *k == r.key);
            self.insert(r.key, r.load.messages);
        }
        if arrived {
            self.wanted = None;
            return true;
        }
        let Some((key, want)) = self.wanted.clone() else { return false };
        if self.in_flight.as_ref() == Some(&key) {
            return false;
        }
        self.rests = self.rests.saturating_add(1);
        if self.rests >= REST_POLLS && self.requests.send((key.clone(), want)).is_ok() {
            self.in_flight = Some(key);
        }
        false
    }

    fn insert(&mut self, key: ChatKey, messages: Vec<ChatMessage>) {
        self.cache.retain(|(k, _)| *k != key);
        if self.cache.len() >= CACHE_SIZE {
            self.cache.pop_front();
        }
        self.cache.push_back((key, Arc::new(messages)));
    }
}

#[cfg(test)]
mod tests;
