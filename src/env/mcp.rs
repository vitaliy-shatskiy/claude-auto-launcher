//! `Get-McpConfigPaths`, `Get-RiderMcpUrl`, `Test-McpPort`: the `--mcp-config` files for a launch.
//! Configured extras that exist, plus a per-PID file pointing at the Rider MCP endpoint when the
//! mode asks for it. Only the MCP endpoint answers 200 with `text/event-stream`; several Rider
//! ports accept TCP and then stall, so the scan runs in parallel behind a cached hit.

use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use super::{CREATE_NO_WINDOW, Note};
use crate::config::RiderMcp;
use crate::out::Fg;

const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

/// The parts of the outside world this module reads, so the decision is testable.
pub(crate) struct Probe<'a> {
    pub(crate) rider_running: bool,
    /// Ports Rider listens on at 127.0.0.1, read only when needed.
    pub(crate) ports: &'a dyn Fn() -> Vec<u16>,
    pub(crate) is_mcp: fn(u16) -> bool,
    pub(crate) temp: PathBuf,
    pub(crate) pid: u32,
}

impl std::fmt::Debug for Probe<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Probe")
            .field("rider_running", &self.rider_running)
            .field("temp", &self.temp)
            .finish_non_exhaustive()
    }
}

pub(crate) fn is_rider(process_name: &str) -> bool {
    process_name.to_ascii_lowercase().starts_with("rider")
}

/// `Test-McpPort`: GET /stream answers 200 with `Content-Type: text/event-stream` within a second.
pub(crate) fn is_mcp_port(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT) else {
        return false;
    };
    if s.set_read_timeout(Some(PROBE_TIMEOUT)).is_err() || s.set_write_timeout(Some(PROBE_TIMEOUT)).is_err() {
        return false;
    }
    let req = format!("GET /stream HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: */*\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    // An event stream never ends: read the head only.
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16 * 1024 {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(buf.get(..n).unwrap_or_default()),
        }
    }
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.split("\r\n");
    let ok = lines.next().is_some_and(|l| l.split_whitespace().nth(1) == Some("200"));
    ok && lines.any(|l| {
        l.split_once(':')
            .is_some_and(|(k, v)| k.trim().eq_ignore_ascii_case("content-type") && v.to_ascii_lowercase().contains("text/event-stream"))
    })
}

/// The ports Rider processes listen on at 127.0.0.1 (`Get-NetTCPConnection -State Listen`), read
/// from `netstat -ano -p TCP`.
pub(crate) fn rider_ports(rider_pids: &[u32]) -> Vec<u16> {
    let Ok(out) = std::process::Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        return Vec::new();
    };
    parse_listening(&String::from_utf8_lossy(&out.stdout), rider_pids)
}

pub(crate) fn parse_listening(netstat: &str, pids: &[u32]) -> Vec<u16> {
    let mut ports: Vec<u16> = netstat
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let [proto, local, _, state, pid] = f.as_slice() else { return None };
            let port = local.strip_prefix("127.0.0.1:")?.parse().ok()?;
            (proto.eq_ignore_ascii_case("TCP") && *state == "LISTENING" && pids.contains(&pid.parse().ok()?)).then_some(port)
        })
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// `Get-RiderMcpUrl`: the cached port first, else a parallel probe of every port; the first that
/// answers wins and is cached.
fn rider_url(p: &Probe<'_>) -> Option<String> {
    let ports = (p.ports)();
    if ports.is_empty() {
        return None;
    }
    let cache = p.temp.join("claude-rider-mcp-port.txt");
    let cached: Option<u16> = std::fs::read_to_string(&cache)
        .ok()
        .and_then(|t| t.lines().next().and_then(|l| l.trim().parse().ok()));
    if let Some(port) = cached.filter(|c| ports.contains(c))
        && (p.is_mcp)(port)
    {
        return Some(format!("http://127.0.0.1:{port}/stream"));
    }
    // Detached probes: the first answer wins and the launch goes on; a stalled port's thread
    // ends at its own timeout.
    let (tx, rx) = mpsc::channel();
    for &port in &ports {
        let (tx, is_mcp) = (tx.clone(), p.is_mcp);
        std::thread::spawn(move || {
            if tx.send(is_mcp(port).then_some(port)).is_err() {
                // The receiver already has its answer.
            }
        });
    }
    drop(tx);
    let hit = rx.iter().flatten().next()?;
    if std::fs::write(&cache, format!("{hit}\r\n")).is_err() {
        // The cache only saves the next launch a scan.
    }
    Some(format!("http://127.0.0.1:{hit}/stream"))
}

/// Yesterday's per-PID files; a live session still holds its own.
fn sweep_old(temp: &Path) {
    let Ok(rd) = std::fs::read_dir(temp) else { return };
    let day_ago = SystemTime::now()
        .checked_sub(Duration::from_hours(24))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        let json = Path::new(&name).extension().is_some_and(|x| x.eq_ignore_ascii_case("json"));
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < day_ago);
        if name.starts_with("claude-mcp-rider-") && json && old && std::fs::remove_file(e.path()).is_err() {
            // Best effort, like Remove-Item -ErrorAction SilentlyContinue.
        }
    }
}

/// The whole decision. Preview names the file it WOULD pass without writing or sweeping.
pub(crate) fn config_paths(extras: &[String], mode: RiderMcp, preview: bool, p: &Probe<'_>) -> (Vec<String>, Vec<Note>) {
    let mut paths: Vec<String> = extras.iter().filter(|e| !e.is_empty() && Path::new(e).exists()).cloned().collect();
    let mut notes = Vec::new();
    match mode {
        RiderMcp::Off => return (paths, notes),
        RiderMcp::Auto if !p.rider_running => return (paths, notes),
        RiderMcp::Auto | RiderMcp::On => {}
    }
    let Some(url) = rider_url(p) else {
        notes.push(Note::new("  rider MCP: not found (is the IDE MCP server enabled?)", Fg::DarkYellow));
        return (paths, notes);
    };
    let tmp = p.temp.join(format!("claude-mcp-rider-{}.json", p.pid));
    if !preview {
        sweep_old(&p.temp);
        let body = serde_json::json!({ "mcpServers": { "rider": { "type": "http", "url": url } } });
        let written = serde_json::to_string_pretty(&body)
            .map_err(std::io::Error::other)
            .and_then(|t| std::fs::write(&tmp, t));
        if let Err(e) = written {
            notes.push(Note::new(format!("  MCP config skipped: {e}"), Fg::DarkYellow));
            return (paths, notes);
        }
    }
    paths.push(tmp.to_string_lossy().into_owned());
    notes.push(Note::new(format!("  rider MCP: {url}"), Fg::DarkGray));
    (paths, notes)
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("claude-auto-rs-mcp-{tag}-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn probe<'a>(dir: &Path, running: bool, ports: &'a dyn Fn() -> Vec<u16>, is_mcp: fn(u16) -> bool) -> Probe<'a> {
        Probe {
            rider_running: running,
            ports,
            is_mcp,
            temp: dir.to_path_buf(),
            pid: 4242,
        }
    }

    #[test]
    fn extras_only_when_present_and_rider_off_says_nothing() {
        let dir = temp("extras");
        let extra = dir.join("extra.json");
        std::fs::write(&extra, "{}").unwrap();
        let extras = vec![extra.to_string_lossy().into_owned(), r"C:\nope\absent.json".to_owned()];
        let never = || -> Vec<u16> { panic!("rider off must not look for ports") };
        let (paths, notes) = config_paths(&extras, RiderMcp::Off, false, &probe(&dir, true, &never, |_| false));
        assert_eq!(
            paths,
            vec![extras[0].clone()],
            "an existing extra is passed, a missing one skipped silently"
        );
        assert!(notes.is_empty(), "rider off: no rider line at all");
        let (_, notes) = config_paths(&[], RiderMcp::Auto, false, &probe(&dir, false, &never, |_| false));
        assert!(notes.is_empty(), "auto without a running Rider: no line");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rider_found_writes_a_per_pid_file_preview_writes_nothing() {
        let dir = temp("rider");
        let ports = || vec![5000, 5001, 5002];
        let is_mcp: fn(u16) -> bool = |p| p == 5001;
        let (paths, notes) = config_paths(&[], RiderMcp::On, true, &probe(&dir, false, &ports, is_mcp));
        let file = dir.join("claude-mcp-rider-4242.json");
        assert_eq!(paths, vec![file.to_string_lossy().into_owned()], "preview still names the file");
        assert!(!file.exists(), "preview writes nothing");
        assert_eq!(notes[0].text, "  rider MCP: http://127.0.0.1:5001/stream");
        config_paths(&[], RiderMcp::On, false, &probe(&dir, false, &ports, is_mcp));
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["rider"]["url"], "http://127.0.0.1:5001/stream");
        assert_eq!(
            std::fs::read_to_string(dir.join("claude-rider-mcp-port.txt")).unwrap().trim(),
            "5001",
            "the hit is cached"
        );
        let (_, notes) = config_paths(&[], RiderMcp::On, false, &probe(&dir, false, &ports, |_| false));
        assert_eq!(notes[0].text, "  rider MCP: not found (is the IDE MCP server enabled?)");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn netstat_listening_ports_of_rider_only() {
        let text = "  Proto  Local Address  Foreign Address  State  PID\n  TCP    127.0.0.1:63342  0.0.0.0:0  LISTENING  10\n  \
                    TCP    127.0.0.1:64000  0.0.0.0:0  LISTENING  10\n  TCP    0.0.0.0:7000  0.0.0.0:0  LISTENING  10\n  \
                    TCP    127.0.0.1:5555  0.0.0.0:0  LISTENING  99\n  TCP    127.0.0.1:64001  127.0.0.1:1  ESTABLISHED  10\n";
        assert_eq!(parse_listening(text, &[10]), vec![63342, 64000]);
        assert!(is_rider("rider64.exe") && is_rider("Rider.Backend.exe") && !is_rider("pwsh.exe"));
    }

    #[test]
    fn only_an_event_stream_answer_is_the_mcp_port() {
        let serve = |reply: &'static str| {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            let h = std::thread::spawn(move || {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = [0u8; 1024];
                let _read = s.read(&mut buf).unwrap();
                s.write_all(reply.as_bytes()).unwrap();
            });
            (port, h)
        };
        let (sse, h1) = serve("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n");
        assert!(is_mcp_port(sse));
        let (other, h2) = serve("HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\n\r\n");
        assert!(!is_mcp_port(other));
        h1.join().unwrap();
        h2.join().unwrap();
    }
}
