//! One plugin call: `<path> <args...> <verb>` windowless, the request as one JSON line on stdin, one
//! JSON value back on stdout, inside a job object that kills the whole tree on a timeout or an
//! oversized answer and lets go of it once the plugin has ended on its own. Exports `Runner` (the
//! seam tests replace), `Process` (the real one), `call` and `Failure`.

use std::io::{Read as _, Write as _};
use std::os::windows::io::AsRawHandle as _;
use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

use super::Verb;
use crate::config::PluginSpec;
use crate::env::CREATE_NO_WINDOW;
use crate::sys::Job;

/// What a plugin may print for one call; more is refused, never parsed.
const MAX_OUTPUT: u64 = 1024 * 1024;
/// How long a killed tree gets to close its pipe before the call returns anyway.
const KILL_GRACE: Duration = Duration::from_secs(2);

/// Why a call gave nothing. Each becomes one warning line; none stops a launch.
#[derive(Debug)]
pub enum Failure {
    /// The listed file is not there: nothing else is ever tried in its place.
    Missing,
    Start(std::io::Error),
    /// Killed with its tree after `ms`.
    Timeout {
        ms: u64,
        pid: u32,
    },
    Exit(i32),
    TooLarge,
    BadJson(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "not found"),
            Self::Start(e) => write!(f, "could not start ({e})"),
            Self::Timeout { ms, pid } => write!(f, "no answer in {ms} ms; killed (pid {pid})"),
            Self::Exit(code) => write!(f, "exit {code}"),
            Self::TooLarge => write!(f, "answer over {MAX_OUTPUT} bytes"),
            Self::BadJson(e) => write!(f, "bad JSON ({e})"),
        }
    }
}

/// The seam: the launcher calls plugins only through this.
pub trait Runner: Sync {
    fn call(&self, spec: &PluginSpec, verb: &Verb, input: &Value) -> Result<Value, Failure>;
}

/// Real processes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Process;

/// The verb's timeout unless the plugin's `timeoutMs` overrides it.
pub fn timeout_of(spec: &PluginSpec, verb: &Verb) -> u64 {
    spec.timeout_ms.unwrap_or(verb.timeout_ms)
}

/// The reader thread's answer: stdout and the exit code, or too much output.
type Ended = Option<(Vec<u8>, i32)>;

impl Runner for Process {
    fn call(&self, spec: &PluginSpec, verb: &Verb, input: &Value) -> Result<Value, Failure> {
        call(spec, verb, input, KILL_GRACE)
    }
}

/// One call on a real process; `kill_grace` is how long a tree killed on a timeout gets to close
/// its pipe before the call returns anyway (`KILL_GRACE` for `Process`; a test passes a long one,
/// so a call that skipped the kill would show as a late return).
pub fn call(spec: &PluginSpec, verb: &Verb, input: &Value, kill_grace: Duration) -> Result<Value, Failure> {
    let path = Path::new(&spec.path);
    if !path.is_file() {
        return Err(Failure::Missing);
    }
    let job = Job::new().ok_or_else(|| Failure::Start(std::io::Error::other("job object refused")))?;
    let mut child = Command::new(path)
        .args(&spec.args)
        .arg(verb.name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(Failure::Start)?;
    let pid = child.id();
    if !job.assign(child.as_raw_handle()) {
        // No tree kill without the job: a plugin that cannot be stopped does not run.
        if child.kill().is_err() {
            // Already gone.
        }
        return Err(Failure::Start(std::io::Error::other("could not join the job object")));
    }
    let body = format!("{input}\n");
    let stdin = child.stdin.take();
    // Its own thread: a plugin that never reads stdin must not block the call.
    std::thread::spawn(move || {
        if let Some(mut s) = stdin
            && s.write_all(body.as_bytes()).is_err()
        {
            // A plugin may exit without reading its input.
        }
    });
    let stdout = child.stdout.take();
    let (tx, rx) = mpsc::channel::<Ended>();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(out) = stdout
            && out.take(MAX_OUTPUT + 1).read_to_end(&mut buf).is_err()
        {
            buf.clear();
        }
        let ended = (u64::try_from(buf.len()).unwrap_or(u64::MAX) <= MAX_OUTPUT)
            .then(|| (buf, child.wait().ok().and_then(|s| s.code()).unwrap_or(1)));
        if tx.send(ended).is_err() {
            // The call already returned: a timeout.
        }
    });
    let ms = timeout_of(spec, verb);
    match rx.recv_timeout(Duration::from_millis(ms)) {
        // The plugin ended on its own: what it started (an app an action opened) is its user's now,
        // so the job lets go of it instead of killing it on close.
        Ok(Some((out, code))) => {
            job.release();
            if code == 0 { parse(&out) } else { Err(Failure::Exit(code)) }
        }
        Ok(None) => {
            job.terminate();
            Err(Failure::TooLarge)
        }
        Err(_) => {
            job.terminate();
            // The tree is gone once its stdout closes and the child is reaped.
            if rx.recv_timeout(kill_grace).is_err() {
                // Still closing: the job handle's drop kills what is left.
            }
            Err(Failure::Timeout { ms, pid })
        }
    }
}

/// One JSON object or array; anything else is bad JSON.
fn parse(out: &[u8]) -> Result<Value, Failure> {
    let text = String::from_utf8_lossy(out);
    let text = text.trim_start_matches('\u{feff}').trim();
    match serde_json::from_str::<Value>(text) {
        Ok(v @ (Value::Object(_) | Value::Array(_))) => Ok(v),
        Ok(_) => Err(Failure::BadJson("not an object or a list".into())),
        Err(e) => Err(Failure::BadJson(e.to_string())),
    }
}
