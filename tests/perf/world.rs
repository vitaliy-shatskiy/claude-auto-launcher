//! The speed budget's world and paths, shared by `tests/perf` (the budget checks) and
//! `examples/bench.rs` (the table): a synthetic session store built once per machine in the temp
//! dir, and one function per measured path returning its work counts. Never the real `~/.claude`.

use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use claude_auto::frame::Style;
use claude_auto::perf::IoCounts;
use claude_auto::sessions::chat::pane::{ChatKey, ChatPane, Fill};
use claude_auto::sessions::frame::resumable;
use claude_auto::sessions::listing::plan::Entry;
use claude_auto::sessions::listing::{Scope, SessionCache, session_files};
use claude_auto::sessions::summaries::{Summarize, Summarizer};
use claude_auto::sessions::{PickerRequest, run_picker};
use claude_auto::ui::{PreviewTerm, Shown, parse_preview_keys};

/// Bump on any change to what `build` writes: an older world on disk is then rebuilt.
const VERSION: &str = "perf-world-v4";
/// The terminal the frame paths render at.
pub(crate) const SIZE: (usize, usize) = (250, 68);
const SLUGS: usize = 7;
const PER_SLUG: usize = 10;
/// (slug, session, MB): the newest is the 83 MB one the chat paths stream; it ends in a chatty
/// tail (`chatty`), the others in one exchange after their tool output.
const BIG: [(usize, usize, usize); 4] = [(0, 0, 83), (1, 0, 20), (2, 0, 5), (3, 5, 40)];
/// The newest session's mtime (Unix seconds); every other one is a minute older per rank.
const NEWEST_S: u64 = 1_790_000_000;
const MB: usize = 1 << 20;

/// One path's measured counts, by metric name (the keys `perf/budget.json` uses).
pub(crate) type Counts = Vec<(&'static str, u64)>;
/// A path's counts and the wall time of its measured section (a reference, never gated).
pub(crate) type Measured = (Counts, Duration);

/// The store on disk: `home/.claude/projects/<slug>/<id>.jsonl`, each slug the transcripts of
/// one project folder `work/Demo<i>` (present, outside every temp root, so the registry lists it).
#[derive(Debug, Clone)]
pub(crate) struct World {
    pub(crate) root: PathBuf,
}

impl World {
    pub(crate) fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    pub(crate) fn projects(&self) -> PathBuf {
        self.home().join(".claude").join("projects")
    }

    fn work(&self, i: usize) -> PathBuf {
        self.root.join("work").join(format!("Demo{i}"))
    }

    /// Claude Code's slug: the folder path, every character but a letter or digit a `-`.
    pub(crate) fn slug(&self, i: usize) -> String {
        let p = self.work(i).to_string_lossy().into_owned();
        p.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
    }

    pub(crate) fn newest(&self) -> PathBuf {
        self.projects().join(self.slug(0)).join(format!("{}.jsonl", id(0, 0)))
    }
}

fn id(slug: usize, n: usize) -> String {
    format!("{slug:02}{n:02}aaaa-0000-4000-8000-00000000{slug:02}{n:02}")
}

fn line(kind: &str, cwd: &str, n: usize, text: &str) -> String {
    let t = "2026-11-20T10:00:00.000Z";
    match kind {
        "user" => {
            format!(r#"{{"type":"user","message":{{"role":"user","content":"{text} {n}"}},"uuid":"u{n}","timestamp":"{t}","cwd":"{cwd}"}}"#)
        }
        "assistant" => format!(
            r#"{{"type":"assistant","uuid":"a{n}","message":{{"id":"msg_{n}","role":"assistant","content":[{{"type":"text","text":"{text} {n}"}}]}},"timestamp":"{t}"}}"#
        ),
        _ => format!(r#"{{"type":"ai-title","aiTitle":"{text} {n}"}}"#),
    }
}

/// A transcript: a few turns, then `mb` megabytes of tool progress (no chat line), then one turn,
/// so the chat walk finds two messages in its last 8 MB and stops at the cap.
fn transcript(cwd: &Path, mb: usize, chatty: bool) -> Vec<u8> {
    let cwd = cwd.to_string_lossy().replace('\\', r"\\");
    let mut s = String::new();
    for n in 0..3 {
        s.push_str(&line("user", &cwd, n, "please look at the build"));
        s.push('\n');
        s.push_str(&line("assistant", &cwd, n, "done, the build is green"));
        s.push('\n');
    }
    s.push_str(&line("ai-title", &cwd, 0, "Synthetic session"));
    s.push('\n');
    let mut bytes = s.into_bytes();
    let filler = format!("{{\"type\":\"progress\",\"data\":\"{}\"}}\n", "x".repeat(1000));
    let block: Vec<u8> = filler.repeat(MB / filler.len()).into_bytes();
    for _ in 0..mb {
        bytes.extend_from_slice(&block);
    }
    let tail = format!(
        "{}\n{}\n",
        line("user", &cwd, 9, "one more thing"),
        line("assistant", &cwd, 9, "all set")
    );
    bytes.extend_from_slice(tail.as_bytes());
    if chatty {
        bytes.extend_from_slice(chatty_tail(&cwd).as_bytes());
    }
    bytes
}

/// 40 turns the way a working session ends: a prompt, three 2 KB tool lines, a markdown reply of
/// six 100-character lines.
fn chatty_tail(cwd: &str) -> String {
    let tool = format!("{{\"type\":\"progress\",\"data\":\"{}\"}}\n", "t".repeat(2000));
    let reply = ["- a list item that says what changed and why it matters here"; 6].join(r"\n") + " plus the closing line of the reply";
    let mut s = String::new();
    for n in 10..50 {
        s.push_str(&line("user", cwd, n, "please check the next step of the build and report"));
        s.push('\n');
        s.push_str(&tool.repeat(3));
        s.push_str(&line("assistant", cwd, n, &reply));
        s.push('\n');
    }
    s
}

/// Writes `w`'s files under `dir`, which is renamed to `w.root` afterwards.
fn build(w: &World, dir: &Path) -> io::Result<()> {
    for s in 0..SLUGS {
        std::fs::create_dir_all(dir.join("work").join(format!("Demo{s}")))?;
        let d = dir.join("home").join(".claude").join("projects").join(w.slug(s));
        std::fs::create_dir_all(&d)?;
        for n in 0..PER_SLUG {
            let mb = BIG.iter().find(|b| b.0 == s && b.1 == n).map_or(0, |b| b.2);
            let f = d.join(format!("{}.jsonl", id(s, n)));
            std::fs::write(&f, transcript(&w.work(s), mb, (s, n) == (0, 0)))?;
            let rank = u64::try_from(n * SLUGS + s).unwrap_or(0);
            let t = UNIX_EPOCH + Duration::from_secs(NEWEST_S - rank * 60);
            std::fs::File::options().write(true).open(&f)?.set_modified(t)?;
        }
    }
    std::fs::write(dir.join("done"), VERSION)
}

/// Every world root is this many characters long, wherever the checkout is: the summary cache
/// holds each session's path and each slug spells its project's path, so the bytes written grew
/// with the target dir's path (a checkout 26 characters deeper wrote 5.4 KB more and failed).
/// Short enough that the deepest transcript path stays under `MAX_PATH`.
const ROOT_LEN: usize = 88;

/// The world's root under `base`, padded to `ROOT_LEN`; an error when `base` is too long for it.
fn root_in(base: &Path) -> io::Result<PathBuf> {
    let name = format!("cap-{VERSION}-");
    let used = base.to_string_lossy().chars().count() + 1 + name.len();
    let pad = ROOT_LEN.checked_sub(used).ok_or_else(|| {
        io::Error::other(format!(
            "{} leaves no room for a {ROOT_LEN}-character world root: build from a shorter path",
            base.display()
        ))
    })?;
    Ok(base.join(format!("{name}{}", "x".repeat(pad))))
}

/// The world under `base` (the cargo target's `tmp`), built on first use and kept for later runs:
/// it never changes, so every reader shares it. Built under a private name and renamed into place:
/// two builders race safely, the loser uses the winner's copy.
pub(crate) fn world(base: &Path) -> io::Result<World> {
    let w = World { root: root_in(base)? };
    let ready = |r: &Path| std::fs::read_to_string(r.join("done")).is_ok_and(|v| v == VERSION);
    if !ready(&w.root) {
        let tmp = base.join(format!("claude-auto-{VERSION}.{}.tmp", std::process::id()));
        if tmp.exists() {
            std::fs::remove_dir_all(&tmp)?;
        }
        build(&w, &tmp)?;
        if std::fs::rename(&tmp, &w.root).is_err() {
            std::fs::remove_dir_all(&tmp)?;
        }
        if !ready(&w.root) {
            return Err(io::Error::other(format!("{} is not a complete world", w.root.display())));
        }
    }
    Ok(w)
}
/// A scratch dir of this process (prefs, caches), emptied first.
pub(crate) fn scratch(name: &str) -> io::Result<PathBuf> {
    let d = std::env::temp_dir().join(format!("claude-auto-perf-{name}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d)?;
    }
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// The launcher in preview over the world, `keys` scripted; returns its I/O and its stdout. It ends
/// when the keys run out, right after the frame the last key produced.
pub(crate) fn launcher(exe: &Path, w: &World, keys: &str) -> io::Result<(IoCounts, String, Duration)> {
    let d = scratch("launcher")?;
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let t = Instant::now();
    let mut child = Command::new(exe)
        .env("USERPROFILE", w.home())
        .env("PATH", format!(r"{system}\System32"))
        .env("CLAUDE_AUTO_CONFIG", d.join("none.json"))
        .env("CLAUDE_AUTO_PREFS", d.join("prefs.json"))
        .env("CLAUDE_AUTO_PREVIEW", "1")
        .env("CLAUDE_AUTO_PREVIEW_SIZE", format!("{}x{}", SIZE.0, SIZE.1))
        .env("CLAUDE_AUTO_PREVIEW_KEYS", keys)
        .env("CLAUDE_AUTO_NO_SELF_UPDATE", "1")
        .env("NO_COLOR", "1")
        .env("CLAUDE_NO_ROAM", "1")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_AUTO_LAUNCHED")
        .env_remove("CLAUDE_AUTO_DEBUG")
        .env_remove("CLAUDE_AUTO_PROJECTS_ROOT")
        .current_dir(&d)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut out = String::new();
    if let Some(mut o) = child.stdout.take() {
        o.read_to_string(&mut out)?;
    }
    child.wait()?;
    let took = t.elapsed();
    let io = IoCounts::of_child(&child).ok_or_else(|| io::Error::other("no I/O counters for the child"))?;
    std::fs::remove_dir_all(&d)?;
    Ok((io, out, took))
}

/// The picker's first page as `run_picker` fetches it: pages of 10 until 10 resumable rows.
pub(crate) fn picker_page(w: &World, cache: &Path) -> usize {
    let root = w.projects();
    let snap = session_files(&root, &[]);
    let mut store = SessionCache::new(false);
    let mut rows = Vec::new();
    while rows.iter().filter(|s| resumable(s)).count() < 10 {
        let page = store.list(&root, Scope::Files(&snap), rows.len(), 10, cache);
        if page.is_empty() {
            break;
        }
        rows.extend(page);
    }
    rows.len()
}

fn own() -> io::Result<IoCounts> {
    IoCounts::own().ok_or_else(|| io::Error::other("no I/O counters for this process"))
}

fn io_counts(io: IoCounts, took: Duration) -> Measured {
    (vec![("read_bytes", io.read_bytes), ("read_ops", io.read_ops)], took)
}

/// (a) Start to the launch screen's first frame.
pub(crate) fn startup(exe: &Path, w: &World) -> io::Result<Measured> {
    let (io, _, took) = launcher(exe, w, "")?;
    Ok(io_counts(io, took))
}

/// (b) Start through the project screen's first frame (Enter on the launch screen).
pub(crate) fn project_screen(exe: &Path, w: &World) -> io::Result<Measured> {
    let (io, out, took) = launcher(exe, w, "Enter")?;
    if !out.contains("Demo0") {
        return Err(io::Error::other(format!("the project screen did not show the world:\n{out}")));
    }
    Ok(io_counts(io, took))
}

/// (c) The picker's first page with `warm` = a summary cache from an earlier open. This process's
/// I/O: the caller runs nothing else meanwhile.
pub(crate) fn picker_open(w: &World, warm: bool) -> io::Result<Measured> {
    let d = scratch(if warm { "picker-warm" } else { "picker-cold" })?;
    let cache = d.join("claude-auto-sessions.json");
    if warm {
        picker_page(w, &cache);
    }
    let (before, t) = (own()?, Instant::now());
    let rows = picker_page(w, &cache);
    let (io, took) = (before.since(own()?), t.elapsed());
    std::fs::remove_dir_all(&d)?;
    if rows == 0 {
        return Err(io::Error::other("the picker listed nothing"));
    }
    Ok((
        vec![
            ("read_bytes", io.read_bytes),
            ("read_ops", io.read_ops),
            ("write_bytes", io.write_bytes),
        ],
        took,
    ))
}

/// (d) One cursor move in the picker at `SIZE`, colour on: the bytes the line-diff writer sends.
pub(crate) fn picker_cursor(w: &World) -> io::Result<Measured> {
    let slugs = [w.slug(0)];
    let req = PickerRequest {
        sessions_root: w.projects(),
        others: &[],
        project_slugs: &slugs,
        project_name: "Demo0",
        style: Style { color: true, ascii: false },
        live_dirs: &[],
        preview: true,
        select: None,
    };
    let keys = parse_preview_keys("DownArrow").map_err(io::Error::other)?;
    let mut term = PreviewTerm::new(SIZE, keys, true);
    // The scripted keys run out after the move: that ends the picker, the frames stay.
    let _stopped = run_picker(&mut term, &req);
    let frames = term.frames.unwrap_or_default();
    // The last paint is the move; the one before it is what the screen showed.
    let [.., first, moved] = frames.as_slice() else {
        return Err(io::Error::other(format!("{} frames, want 2", frames.len())));
    };
    let mut shown = Shown::default();
    let full = shown.update(SIZE, first).len();
    let t = Instant::now();
    let diff = shown.update(SIZE, moved).len();
    let took = t.elapsed();
    let to_u64 = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    Ok((vec![("frame_bytes", to_u64(diff)), ("full_frame_bytes", to_u64(full))], took))
}

/// The chat pane at 200x50: what a stream fills.
const CHAT_PANE: Fill = Fill { rows: 50, cols: 100 };

fn chat_key(path: &Path) -> io::Result<ChatKey> {
    Ok(ChatKey {
        path: path.to_path_buf(),
        size: std::fs::metadata(path)?.len(),
        modified_ms: 1,
    })
}

/// Idle polls until the pane's workers have nothing left.
fn settle(p: &mut ChatPane) {
    while !p.quiet() {
        p.idle();
        std::thread::yield_now();
    }
}

/// The highlighted newest transcript streamed until the pane is filled plus one screen.
fn stream_newest(w: &World) -> io::Result<(ChatPane, ChatKey)> {
    let key = chat_key(&w.newest())?;
    let mut p = ChatPane::new(claude_auto::timing::Timings::default());
    p.view(&key, CHAT_PANE);
    settle(&mut p);
    Ok((p, key))
}

/// (e) The chat pane filling 200x50 from the newest (83 MB) transcript: the workers' own count
/// and this process's.
pub(crate) fn chat_load(w: &World) -> io::Result<Measured> {
    let (before, t) = (own()?, Instant::now());
    let (p, _) = stream_newest(w)?;
    let (io, took) = (before.since(own()?), t.elapsed());
    Ok((vec![("chat_bytes_read", p.bytes_read()), ("read_bytes", io.read_bytes)], took))
}

/// (e2) Back to a session streamed before: this process reads nothing.
pub(crate) fn chat_revisit(w: &World) -> io::Result<Measured> {
    let (mut p, key) = stream_newest(w)?;
    let (before, t) = (own()?, Instant::now());
    p.view(&key, CHAT_PANE);
    for _ in 0..10 {
        p.idle();
    }
    settle(&mut p);
    let (io, took) = (before.since(own()?), t.elapsed());
    Ok((vec![("read_bytes", io.read_bytes)], took))
}

/// (e3) The picker's open prefetch: the newest 5 sessions, each to one pane height.
pub(crate) fn chat_prefetch_open(w: &World) -> io::Result<Measured> {
    let keys = (0..5)
        .map(|s| chat_key(&w.projects().join(w.slug(s)).join(format!("{}.jsonl", id(s, 0)))))
        .collect::<io::Result<Vec<_>>>()?;
    let (before, t) = (own()?, Instant::now());
    let mut p = ChatPane::new(claude_auto::timing::Timings::default());
    p.prefetch(keys, CHAT_PANE);
    settle(&mut p);
    let (io, took) = (before.since(own()?), t.elapsed());
    Ok((vec![("chat_bytes_read", p.bytes_read()), ("read_bytes", io.read_bytes)], took))
}
/// Which thread's work `picker_interactive` returns.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Side {
    /// The UI thread, start to the first frame: cache hits and quick rows only.
    Ui,
    /// The worker, the full summaries of the first frame's pages (the cache write included).
    Worker,
}

/// (f) The interactive picker's cold open as `run_picker` runs it off preview: the UI thread
/// pages through `Summarizer::page` until 10 resumable rows while the worker is held at its first
/// full summary, so the process counters hold the UI thread's work alone; then the worker is let
/// go and measured until it has stored every page.
pub(crate) fn picker_interactive(w: &World, side: Side) -> io::Result<Measured> {
    let d = scratch("picker-interactive")?;
    let cache_file = d.join("claude-auto-sessions.json");
    let root = w.projects();
    let gate = Arc::new(RwLock::new(()));
    let held = gate.write().unwrap_or_else(PoisonError::into_inner);
    let worker_gate = Arc::clone(&gate);
    // The production summary, run once the test lets the worker go.
    let full: Summarize = Arc::new(move |e: &Entry| {
        drop(worker_gate.read());
        e.summarize()
    });
    let (done, landed) = channel();
    let worker = Summarizer::start(full, done);
    let (before, t) = (own()?, Instant::now());
    let snap = session_files(&root, &[]);
    let mut store = SessionCache::new(false);
    let mut rows = Vec::new();
    while rows.iter().filter(|s| resumable(s)).count() < 10 {
        let page = worker.page(&mut store, &root, &snap, rows.len(), 10, &cache_file);
        if page.is_empty() {
            break;
        }
        rows.extend(page);
    }
    let (ui, ui_took) = (before.since(own()?), t.elapsed());
    let (before, t) = (own()?, Instant::now());
    drop(held);
    // The worker ends once its queue is done and the last handle is gone: `landed` then closes.
    drop(worker);
    let landed_rows: usize = landed.iter().map(|batch| batch.len()).sum();
    let (full_io, full_took) = (before.since(own()?), t.elapsed());
    std::fs::remove_dir_all(&d)?;
    if rows.is_empty() || landed_rows != rows.len() {
        return Err(io::Error::other(format!("{} rows listed, {landed_rows} summarised", rows.len())));
    }
    Ok(match side {
        Side::Ui => (vec![("read_bytes", ui.read_bytes), ("read_ops", ui.read_ops)], ui_took),
        Side::Worker => (
            vec![
                ("read_bytes", full_io.read_bytes),
                ("read_ops", full_io.read_ops),
                ("write_bytes", full_io.write_bytes),
            ],
            full_took,
        ),
    })
}

/// A path's runner; `exe` is the built `claude-auto.exe`.
pub(crate) type Run = fn(&Path, &World) -> io::Result<Measured>;

/// Every measured path by its budget name, in table order.
pub(crate) const PATHS: [(&str, Run); 10] = [
    ("startup", startup),
    ("project_screen", project_screen),
    ("picker_open_cold", |_, w| picker_open(w, false)),
    ("picker_open_warm", |_, w| picker_open(w, true)),
    ("picker_ui_open_cold", |_, w| picker_interactive(w, Side::Ui)),
    ("picker_worker_scan_cold", |_, w| picker_interactive(w, Side::Worker)),
    ("picker_cursor", |_, w| picker_cursor(w)),
    ("chat_load", |_, w| chat_load(w)),
    ("chat_revisit", |_, w| chat_revisit(w)),
    ("chat_prefetch_open", |_, w| chat_prefetch_open(w)),
];
