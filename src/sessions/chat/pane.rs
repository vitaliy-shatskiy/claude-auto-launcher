//! The chat pane's state behind the picker. The UI thread never reads a transcript: `view`
//! answers from what the workers streamed so far, `idle` collects their progress and hands out
//! work. Two threads share the open streams (`ChatStream`, resumable between chunks):
//! - the demand worker streams the highlighted session backwards until the pane is filled plus
//!   one screen, sending what it has after every chunk; a newer target stops it at the next
//!   chunk boundary, its stream kept to resume;
//! - the prefetch worker, at below-normal priority, reads ONE chunk per idle poll for the
//!   sessions `prefetch` names, never while the demand worker has work, never past one pane
//!   height or `PREFETCH_BYTES` per session.
//!
//! Everything streamed stays for the life of the picker: going back to a session reads nothing.

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Instant;

use super::{ChatMessage, ChatStream};
use crate::timing::Timings;

/// A transcript as last listed: a grown or rewritten file is another key.
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
    /// An older version of the same transcript while the current one streams in.
    Updating(Arc<Vec<ChatMessage>>),
}

/// The pane's size: `rows` lines of `cols` cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    pub rows: usize,
    pub cols: usize,
}

/// Idle polls the cursor must rest before work starts (~60 ms each on the console).
const REST_POLLS: u8 = 2;
/// The prefetch reads at most this much of one session per picker open; the demand worker
/// resumes from there when the cursor lands on it.
pub const PREFETCH_BYTES: u64 = 1 << 20;

type Streams = Arc<Mutex<HashMap<ChatKey, ChatStream<File>>>>;

#[derive(Debug, Clone)]
struct Job {
    key: ChatKey,
    fill: Fill,
}

/// One step's news: the session's messages so far and where its stream stands.
struct Reply {
    key: ChatKey,
    messages: Arc<Vec<ChatMessage>>,
    rows: usize,
    done: bool,
    /// Bytes this step read, and the stream's total.
    bytes: u64,
    total: u64,
    /// The job ended (target met, done, or a newer job took over).
    last: bool,
}

/// What was streamed of one session.
#[derive(Debug)]
struct Shown {
    messages: Arc<Vec<ChatMessage>>,
    rows: usize,
    done: bool,
    total: u64,
}

pub struct ChatPane {
    demand: Sender<Job>,
    prefetch: Sender<Job>,
    replies: Receiver<Reply>,
    shown: HashMap<ChatKey, Shown>,
    viewing: Option<ChatKey>,
    wanted: Option<Job>,
    rests: u8,
    in_flight: Option<(ChatKey, Instant)>,
    prefetching: Option<ChatKey>,
    queue: Vec<ChatKey>,
    queue_fill: Fill,
    bytes: u64,
    timings: Timings,
    #[cfg_attr(not(test), expect(dead_code, reason = "held for the test that joins them"))]
    threads: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for ChatPane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatPane")
            .field("shown", &self.shown.len())
            .field("in_flight", &self.in_flight)
            .field("prefetching", &self.prefetching)
            .finish_non_exhaustive()
    }
}

/// The stream of `key`: the one kept from an earlier job, else a new one from the file's end.
fn take(streams: &Streams, key: &ChatKey) -> Option<ChatStream<File>> {
    let kept = streams.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
    kept.or_else(|| ChatStream::open(&key.path))
}

fn keep(streams: &Streams, key: ChatKey, s: ChatStream<File>) {
    streams.lock().unwrap_or_else(PoisonError::into_inner).insert(key, s);
}

fn reply(key: &ChatKey, s: Option<&ChatStream<File>>, fill: Fill, bytes: u64, last: bool) -> Reply {
    Reply {
        key: key.clone(),
        messages: Arc::new(s.map(ChatStream::messages).unwrap_or_default()),
        rows: s.map_or(0, |s| s.rows(fill.cols)),
        done: s.is_none_or(ChatStream::done),
        bytes,
        total: s.map_or(0, ChatStream::bytes_read),
        last,
    }
}

/// The demand worker: streams each job to its fill, a newer job taking over between chunks.
fn demand_loop(jobs: &Receiver<Job>, done: &Sender<Reply>, streams: &Streams) {
    let mut next = jobs.recv().ok();
    while let Some(mut job) = next.take() {
        while let Ok(newer) = jobs.try_recv() {
            job = newer;
        }
        let Some(mut s) = take(streams, &job.key) else {
            if done.send(reply(&job.key, None, job.fill, 0, true)).is_err() {
                return;
            }
            next = jobs.recv().ok();
            continue;
        };
        while s.rows(job.fill.cols) < job.fill.rows && !s.done() {
            let before = s.bytes_read();
            s.step();
            if done
                .send(reply(&job.key, Some(&s), job.fill, s.bytes_read() - before, false))
                .is_err()
            {
                return;
            }
            if let Ok(newer) = jobs.try_recv() {
                next = Some(newer);
                break;
            }
        }
        let last = reply(&job.key, Some(&s), job.fill, 0, true);
        keep(streams, job.key, s);
        if done.send(last).is_err() {
            return;
        }
        if next.is_none() {
            next = jobs.recv().ok();
        }
    }
}

/// The prefetch worker: one chunk per job, at below-normal priority.
fn prefetch_loop(jobs: &Receiver<Job>, done: &Sender<Reply>, streams: &Streams) {
    crate::sessions::win::lower_this_thread();
    while let Ok(job) = jobs.recv() {
        let Some(mut s) = take(streams, &job.key) else {
            if done.send(reply(&job.key, None, job.fill, 0, true)).is_err() {
                return;
            }
            continue;
        };
        let before = s.bytes_read();
        s.step();
        let r = reply(&job.key, Some(&s), job.fill, s.bytes_read() - before, true);
        keep(streams, job.key, s);
        if done.send(r).is_err() {
            return;
        }
    }
}

impl ChatPane {
    /// Starts both workers. They end once the pane is dropped (the picker closed), the demand
    /// worker at its next chunk boundary.
    pub fn new(timings: Timings) -> Self {
        let streams: Streams = Arc::default();
        let (demand, demand_jobs) = channel::<Job>();
        let (prefetch, prefetch_jobs) = channel::<Job>();
        let (done, replies) = channel::<Reply>();
        let (s1, d1) = (Arc::clone(&streams), done.clone());
        let threads = vec![
            std::thread::spawn(move || demand_loop(&demand_jobs, &d1, &s1)),
            std::thread::spawn(move || prefetch_loop(&prefetch_jobs, &done, &streams)),
        ];
        Self {
            demand,
            prefetch,
            replies,
            shown: HashMap::new(),
            viewing: None,
            wanted: None,
            rests: 0,
            in_flight: None,
            prefetching: None,
            queue: Vec::new(),
            queue_fill: Fill { rows: 0, cols: 0 },
            bytes: 0,
            timings,
            threads,
        }
    }

    /// The chat for `key` as far as it was streamed. Short of the pane plus one screen of margin
    /// (and not at the file's start or the cap), `key` becomes the one to stream.
    pub fn view(&mut self, key: &ChatKey, pane: Fill) -> ChatState {
        if self.viewing.as_ref() != Some(key) {
            self.viewing = Some(key.clone());
            self.rests = 0;
        }
        let target = Fill {
            rows: pane.rows * 2,
            cols: pane.cols,
        };
        let entry = self.shown.get(key);
        if entry.is_some_and(|e| e.done || e.rows >= target.rows) {
            self.wanted = None;
        } else if self.wanted.as_ref().is_none_or(|w| w.key != *key || w.fill != target) {
            self.wanted = Some(Job {
                key: key.clone(),
                fill: target,
            });
        }
        match entry {
            Some(e) if !e.messages.is_empty() || e.done => ChatState::Ready(Arc::clone(&e.messages)),
            _ => self
                .shown
                .iter()
                .find(|(k, e)| k.path == key.path && !e.messages.is_empty())
                .map_or(ChatState::Loading, |(_, e)| ChatState::Updating(Arc::clone(&e.messages))),
        }
    }

    /// The sessions to prefetch, most wanted first, each to one pane of `pane`'s size.
    pub fn prefetch(&mut self, keys: Vec<ChatKey>, pane: Fill) {
        self.queue = keys;
        self.queue_fill = pane;
    }

    /// Bytes the workers read so far.
    pub fn bytes_read(&self) -> u64 {
        self.bytes
    }

    /// Nothing wanted, nothing running, nothing left to prefetch.
    pub fn quiet(&self) -> bool {
        self.wanted.is_none() && self.in_flight.is_none() && self.prefetching.is_none() && self.next_prefetch().is_none()
    }

    /// One idle poll: takes the workers' progress, starts the wanted stream once the cursor
    /// rested, else grants the prefetch one chunk. `true` when the shown chat changed.
    pub fn idle(&mut self) -> bool {
        let changed = self.collect();
        self.rests = self.rests.saturating_add(1);
        if self.rests < REST_POLLS {
            return changed;
        }
        if let Some(job) = self.wanted.clone() {
            let running = self.in_flight.as_ref().is_some_and(|(k, _)| *k == job.key);
            if !running && self.prefetching.as_ref() != Some(&job.key) && self.demand.send(job.clone()).is_ok() {
                self.in_flight = Some((job.key, Instant::now()));
            }
            return changed;
        }
        if self.in_flight.is_none()
            && self.prefetching.is_none()
            && let Some(key) = self.next_prefetch()
        {
            let job = Job {
                key: key.clone(),
                fill: self.queue_fill,
            };
            if self.prefetch.send(job).is_ok() {
                self.prefetching = Some(key);
            }
        }
        changed
    }

    fn next_prefetch(&self) -> Option<ChatKey> {
        let fill = self.queue_fill;
        self.queue
            .iter()
            .find(|k| {
                self.shown
                    .get(*k)
                    .is_none_or(|e| !e.done && e.rows < fill.rows && e.total < PREFETCH_BYTES)
            })
            .cloned()
    }

    /// Takes every finished step; `true` when the viewed session's messages changed.
    fn collect(&mut self) -> bool {
        let mut changed = false;
        while let Ok(r) = self.replies.try_recv() {
            self.bytes += r.bytes;
            if r.last {
                if let Some((_, start)) = self.in_flight.take_if(|(k, _)| *k == r.key) {
                    let id = self.timings.add(&["picker.chat.load"], None, start, Instant::now(), None);
                    self.timings.set_field(id, "bytes", r.total);
                }
                // A stream that met its target or ended is not wanted again until the next view.
                self.wanted.take_if(|w| w.key == r.key && (r.done || r.rows >= w.fill.rows));
                self.prefetching.take_if(|k| *k == r.key);
            }
            let viewed = self.viewing.as_ref() == Some(&r.key);
            let before = self.shown.get(&r.key).map(|e| Arc::clone(&e.messages));
            changed |= viewed && before.is_none_or(|b| *b != *r.messages);
            self.shown.insert(
                r.key,
                Shown {
                    messages: r.messages,
                    rows: r.rows,
                    done: r.done,
                    total: r.total,
                },
            );
        }
        changed
    }

    /// The worker threads, for a test that proves they end with the pane.
    #[cfg(test)]
    fn close(mut self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut self.threads)
    }
}

#[cfg(test)]
mod tests;
