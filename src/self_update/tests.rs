use super::*;
use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, channel};

/// A temp directory dropped with the test.
pub(in crate::self_update) struct Tmp(pub(in crate::self_update) PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

pub(in crate::self_update) fn tmp(tag: &str) -> Tmp {
    let d = std::env::temp_dir().join(format!("claude-auto-rs-selfupd-{tag}-{}", crate::log::new_run_id()));
    std::fs::create_dir_all(&d).unwrap();
    Tmp(d)
}

/// Answers by URL; an unknown URL is a 404. Every call is recorded.
pub(in crate::self_update) struct Stub {
    pub(in crate::self_update) answers: BTreeMap<String, Result<Vec<u8>, FetchFailed>>,
    pub(in crate::self_update) calls: Mutex<Vec<String>>,
}

impl Stub {
    pub(in crate::self_update) fn new(answers: &[(&str, Result<Vec<u8>, FetchFailed>)]) -> Self {
        Self {
            answers: answers.iter().map(|(u, a)| ((*u).to_owned(), a.clone())).collect(),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub(in crate::self_update) fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Net for Stub {
    fn get(&self, url: &str, cap: usize) -> Result<Vec<u8>, FetchFailed> {
        self.calls.lock().unwrap().push(url.to_owned());
        let a = self
            .answers
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(FetchFailed::Reason("HTTP 404".into())))?;
        if a.len() > cap {
            return Err(FetchFailed::Reason("too large".into()));
        }
        Ok(a)
    }
}

/// Fails the test if any request is made.
struct NoNet;

impl Net for NoNet {
    fn get(&self, url: &str, _cap: usize) -> Result<Vec<u8>, FetchFailed> {
        panic!("no request may be made, got {url}")
    }
}

/// Holds every answer until the test sends one.
struct Held(Mutex<Receiver<()>>, Stub);

impl Net for Held {
    fn get(&self, url: &str, cap: usize) -> Result<Vec<u8>, FetchFailed> {
        self.0.lock().unwrap().recv().unwrap();
        self.1.get(url, cap)
    }
}

pub(in crate::self_update) const API: &str = "https://api.github.com/repos/vitaliy-shatskiy/claude-auto-launcher/releases/latest";
pub(in crate::self_update) const EXE_URL: &str =
    "https://github.com/vitaliy-shatskiy/claude-auto-launcher/releases/download/v99.0.0/claude-auto.exe";
pub(in crate::self_update) const SHA_URL: &str =
    "https://github.com/vitaliy-shatskiy/claude-auto-launcher/releases/download/v99.0.0/claude-auto.exe.sha256";

/// A `releases/latest` answer for `tag` with both assets at `EXE_URL`/`SHA_URL`.
pub(in crate::self_update) fn release_json(tag: &str) -> Vec<u8> {
    serde_json::json!({
        "tag_name": tag,
        "draft": false,
        "prerelease": false,
        "assets": [
            { "name": "claude-auto.exe", "browser_download_url": EXE_URL },
            { "name": "claude-auto.exe.sha256", "browser_download_url": SHA_URL },
        ]
    })
    .to_string()
    .into_bytes()
}

/// The `.sha256` file the release workflow writes for `bytes`, hashed by the production hasher.
pub(in crate::self_update) fn sha_file(dir: &Path, bytes: &[u8]) -> Vec<u8> {
    let f = dir.join("hash-me");
    std::fs::write(&f, bytes).unwrap();
    let h = crate::maintenance::win::sha256_file(&f).unwrap().to_lowercase();
    std::fs::remove_file(&f).unwrap();
    format!("{h}  claude-auto.exe\n").into_bytes()
}

/// An installed exe in `dir`, the given mode, the state beside it.
pub(in crate::self_update) fn local(dir: &Path, mode: Mode) -> Local {
    let exe = dir.join("claude-auto.exe");
    std::fs::write(&exe, b"current build").unwrap();
    Local {
        mode,
        repo: Repo::default(),
        exe,
        state: dir.join(STATE_FILE),
    }
}

fn full_stub(dir: &Path) -> Stub {
    Stub::new(&[
        (API, Ok(release_json("v99.0.0"))),
        (SHA_URL, Ok(sha_file(dir, b"new build"))),
        (EXE_URL, Ok(b"new build".to_vec())),
    ])
}

const NOW: i64 = 1_800_000_000_000;

#[test]
fn notify_line_comes_in_without_holding_up_the_screen() {
    let t = tmp("notify");
    let (go, rx) = channel();
    let net = Arc::new(Held(Mutex::new(rx), full_stub(&t.0)));
    let mut w = Watch::start_with(local(&t.0, Mode::Notify), NOW, net, Arc::new(RealFiles));
    assert!(!w.news(), "the held check has not answered");
    assert_eq!(w.line(false), None, "the first frame draws without the line");
    go.send(()).unwrap();
    while !w.news() {
        std::thread::yield_now();
    }
    assert_eq!(w.line(false).as_deref(), Some("claude-auto 99.0.0 available · u → update"));
    assert_eq!(w.line(true).as_deref(), Some("claude-auto 99.0.0 available - u -> update"));
    assert!(!w.news(), "news is told once");
}

#[test]
fn off_makes_no_request() {
    let t = tmp("off");
    let l = local(&t.0, Mode::Off);
    let w = Watch::start_with(l.clone(), NOW, Arc::new(NoNet), Arc::new(RealFiles));
    assert!(w.job.is_none(), "no thread for off");
    assert_eq!(run(&l, NOW, &NoNet, &RealFiles, &Mutex::new(false)), None);
    assert_eq!(l.act_with(UPDATE_KEY, &NoNet, &RealFiles), None, "no offer, no request");
}

#[test]
fn auto_swaps_in_the_background_and_says_when_it_takes_effect() {
    let t = tmp("auto");
    let l = local(&t.0, Mode::Auto);
    let mut w = Watch::start_with(l.clone(), NOW, Arc::new(full_stub(&t.0)), Arc::new(RealFiles));
    while !w.news() {
        std::thread::yield_now();
    }
    assert_eq!(
        w.line(false).as_deref(),
        Some("claude-auto updated to 99.0.0 - takes effect next start")
    );
    assert_eq!(std::fs::read(&l.exe).unwrap(), b"new build");
    assert_eq!(std::fs::read(swap::sibling(&l.exe, ".old")).unwrap(), b"current build");
    let s = State::read(&l.state);
    assert_eq!((s.installed, s.previous), (Version::parse("99.0.0"), Version::current()));
}

#[test]
fn a_quitting_launcher_never_starts_the_swap() {
    let t = tmp("quit");
    let l = local(&t.0, Mode::Auto);
    let n = run(&l, NOW, &full_stub(&t.0), &RealFiles, &Mutex::new(true));
    assert!(matches!(n, Some(Notice::Failed(..))), "{n:?}");
    assert_eq!(std::fs::read(&l.exe).unwrap(), b"current build");
    assert!(!swap::sibling(&l.exe, ".new").exists(), "the download is cleaned up");
}

#[test]
fn maintenance_offers_update_then_roll_back_and_both_work() {
    let t = tmp("maint");
    let l = local(&t.0, Mode::Notify);
    let stub = full_stub(&t.0);
    assert_eq!(
        run(&l, NOW, &stub, &RealFiles, &Mutex::new(false)),
        Some(Notice::Available(Version([99, 0, 0])))
    );
    assert_eq!(l.entries(), vec![(UPDATE_KEY, "update claude-auto to 99.0.0".to_owned())]);
    assert_eq!(
        l.act_with(UPDATE_KEY, &stub, &RealFiles).as_deref(),
        Some("updated to 99.0.0 - takes effect next start")
    );
    let current = Version::current().unwrap();
    assert_eq!(
        l.entries(),
        vec![(ROLLBACK_KEY, format!("roll back to {current}"))],
        "no second update offered"
    );
    assert_eq!(
        l.act_with(ROLLBACK_KEY, &NoNet, &RealFiles).as_deref(),
        Some("rolled back - takes effect next start")
    );
    assert_eq!(std::fs::read(&l.exe).unwrap(), b"current build");
    assert_eq!(l.entries().last(), Some(&(ROLLBACK_KEY, "roll back to 99.0.0".to_owned())));
    assert_eq!(l.act_with('x', &NoNet, &RealFiles), None, "not a key of this module");
}

#[test]
fn config_values_parse_strictly() {
    assert_eq!(Mode::parse(" Auto "), Some(Mode::Auto));
    assert_eq!(Mode::parse("daily"), None);
    assert_eq!(
        Repo::parse("me/claude-auto.fork").map(|r| r.0),
        Some("me/claude-auto.fork".to_owned())
    );
    for bad in ["me", "me/", "/x", "a/b/c", "../x", "me/..", "me/x?y", "https://github.com/me/x"] {
        assert_eq!(Repo::parse(bad), None, "{bad}");
    }
}
