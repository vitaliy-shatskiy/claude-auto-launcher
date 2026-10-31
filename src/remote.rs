//! The remote/companion path (`Remote.ps1` and the crc branch of `claude-auto.ps1`): the
//! companion server the phone talks to, started on demand and stopped only when it is provably
//! ours; the tailnet line; the pairing QR; the session born through `crc.cmd`.

mod win;

use std::fs::File;
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::out::{self, Fg};
use crate::types::ChildEnv;

/// What the crc branch launches.
#[derive(Debug, Clone)]
pub struct CrcLaunch<'a> {
    pub argv: &'a [String],
    pub account: &'a str,
    pub show_qr: bool,
    pub child: &'a ChildEnv,
}

const DEFAULT_PORT: u16 = 8791;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// `Get-CompanionPort`: `port` of `%APPDATA%\claude-remote\config.json`, else 8791.
fn companion_port(appdata: Option<&Path>) -> u16 {
    appdata
        .map(|a| a.join("claude-remote").join("config.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
        .and_then(|v| {
            let p = v.get("port")?;
            p.as_u64().or_else(|| p.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
        .unwrap_or(DEFAULT_PORT)
}

fn live_port() -> u16 {
    companion_port(std::env::var_os("APPDATA").map(PathBuf::from).as_deref())
}

/// `Get-CompanionRoot`: `CLAUDE_REMOTE_ROOT` when set, else the documented checkout.
fn companion_root(env_root: Option<&str>, home: &Path) -> PathBuf {
    match env_root.filter(|r| !r.is_empty()) {
        Some(r) => PathBuf::from(r),
        None => home.join("Desktop").join("Projects").join("remote-control-claude-code"),
    }
}

fn live_root() -> PathBuf {
    companion_root(
        std::env::var("CLAUDE_REMOTE_ROOT").ok().as_deref(),
        &crate::config::PathCtx::from_env().home,
    )
}

/// A process as the stop guard sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcInfo {
    /// Without `.exe`, like `Get-Process`'s `ProcessName`.
    name: String,
    command_line: String,
}

/// `Test-CompanionProcess`: node, running out of the configured checkout. The port alone says
/// nothing about identity, and neither does "some node process".
fn is_companion(info: &ProcInfo, root: &str) -> bool {
    if !info.name.eq_ignore_ascii_case("node") || info.command_line.is_empty() {
        return false;
    }
    let needle = root.replace('\\', "/").trim_end_matches('/').to_lowercase();
    !needle.is_empty() && info.command_line.replace('\\', "/").to_lowercase().contains(&needle)
}

/// The process seams of the stop path, so tests never find or stop a real process.
trait Procs {
    fn listener(&mut self, port: u16) -> Option<u32>;
    fn info(&mut self, pid: u32) -> Option<ProcInfo>;
    fn stop(&mut self, pid: u32) -> Result<(), String>;
}

#[derive(Debug)]
struct LiveProcs;

impl Procs for LiveProcs {
    fn listener(&mut self, port: u16) -> Option<u32> {
        win::listener_pid(port)
    }

    fn info(&mut self, pid: u32) -> Option<ProcInfo> {
        let row = crate::sys::process_table().into_iter().find(|r| r.pid == pid)?;
        let name = row
            .name
            .strip_suffix(".exe")
            .or_else(|| row.name.strip_suffix(".EXE"))
            .unwrap_or(&row.name)
            .to_owned();
        Some(ProcInfo {
            name,
            command_line: win::command_line(pid).unwrap_or_default(),
        })
    }

    fn stop(&mut self, pid: u32) -> Result<(), String> {
        win::terminate(pid)
    }
}

/// `Stop-CompanionServer`: identify the listener first; anything that is not ours is named and
/// left running. Always a status line, never a failure: this runs while the launcher quits.
fn stop_with(root: &Path, port: u16, procs: &mut dyn Procs) -> String {
    let Some(pid) = procs.listener(port) else {
        return format!("nothing listening on {port}");
    };
    // A listen entry can outlive its pid for a moment: already gone, not a failure.
    let Some(info) = procs.info(pid) else {
        return format!("nothing listening on {port} (stale socket entry for pid {pid})");
    };
    if !is_companion(&info, &root.to_string_lossy()) {
        return format!(
            "port {port} is held by {} (pid {pid}), which is not the companion server - left alone",
            info.name
        );
    }
    match procs.stop(pid) {
        Ok(()) => format!("stopped pid {pid} on port {port}"),
        Err(e) => format!("failed to stop pid {pid}: {e}"),
    }
}

/// `Stop-CompanionServer` on the live machine; `Ok` carries the status line in every case.
pub fn stop_companion_server() -> Result<String, String> {
    Ok(stop_with(&live_root(), live_port(), &mut LiveProcs))
}

/// `Get-CompanionLogPath`: one log per launcher instance; `.err` and `.build` share its name.
fn log_path(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("claude-remote-server-{pid}.log"))
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    crate::invocation::find_on_path(name, &path, &pathext)
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).ok()?.modified().ok()
}

fn newest_ts(dir: &Path) -> Option<SystemTime> {
    let mut newest = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let t = if p.is_dir() {
            newest_ts(&p)
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ts")) {
            mtime(&p)
        } else {
            None
        };
        newest = newest.max(t);
    }
    newest
}

/// `dist/index.js` is missing or older than any source or config file.
fn dist_stale(server: &Path) -> bool {
    let mut newest = newest_ts(&server.join("src"));
    for extra in ["package.json", "tsconfig.json", "tsconfig.build.json"] {
        newest = newest.max(mtime(&server.join(extra)));
    }
    match mtime(&server.join("dist").join("index.js")) {
        None => true,
        Some(d) => newest.is_some_and(|n| d < n),
    }
}

fn has_build_script(server: &Path) -> Result<bool, String> {
    let text = std::fs::read_to_string(server.join("package.json")).map_err(|e| e.to_string())?;
    let pkg: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    Ok(pkg.pointer("/scripts/build").and_then(Value::as_str).is_some_and(|s| !s.is_empty()))
}

/// `dist` when the production build is current or rebuilds cleanly, `tsx` otherwise: a broken
/// build must never leave the phone without a server.
fn server_mode(server: &Path, log: &Path) -> &'static str {
    let attempt = || -> Result<&'static str, String> {
        if !has_build_script(server)? {
            return Ok("tsx");
        }
        if !dist_stale(server) {
            return Ok("dist");
        }
        let build_log = with_suffix(log, ".build");
        let npm = on_path("npm").ok_or("npm is not on PATH")?;
        let f = File::create(&build_log).map_err(|e| e.to_string())?;
        let f2 = f.try_clone().map_err(|e| e.to_string())?;
        let ok = Command::new(npm)
            .args(["run", "build", "--silent"])
            .current_dir(server)
            .stdin(Stdio::null())
            .stdout(f)
            .stderr(f2)
            .status()
            .map_err(|e| e.to_string())?
            .success();
        if ok && server.join("dist").join("index.js").exists() {
            return Ok("dist");
        }
        out::line(
            &format!("  server build failed - falling back to tsx (see {})", build_log.display()),
            Fg::DarkYellow,
        );
        Ok("tsx")
    };
    attempt().unwrap_or_else(|e| {
        out::line(&format!("  build check failed - falling back to tsx: {e}"), Fg::DarkYellow);
        "tsx"
    })
}

/// `Start-CompanionServer`: reports what actually happened, not what was intended. `Err` when
/// node itself could not be started.
fn start_companion_server(root: &Path, port: u16, log: &Path) -> Result<String, String> {
    let server = root.join("server");
    if !server.join("package.json").exists() {
        return Ok(format!("no checkout at {}", root.display()));
    }
    if win::listener_pid(port).is_some() {
        return Ok(format!("already listening on {port}"));
    }
    let mode = server_mode(&server, log);
    let args: &[&str] = if mode == "dist" {
        &["dist/index.js"]
    } else {
        &["--import", "tsx", "src/index.ts"]
    };
    let node = on_path("node").ok_or("node is not on PATH")?;
    let stdout = File::create(log).map_err(|e| e.to_string())?;
    let stderr = File::create(with_suffix(log, ".err")).map_err(|e| e.to_string())?;
    // Detached and windowless: the server outlives this launcher and never sees its Ctrl+C.
    Command::new(node)
        .args(args)
        .current_dir(&server)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map_err(|e| e.to_string())?;
    for _ in 0..24 {
        std::thread::sleep(Duration::from_millis(250));
        if win::listener_pid(port).is_some() {
            return Ok(format!("started on {port} ({mode})"));
        }
    }
    Ok(format!("did not come up within 6s - see {}", log.display()))
}

/// `Show-PairingQr`: `npm run pair` prints the code; only a code that verified gets the pause.
fn show_pairing_qr(root: &Path) {
    let run = on_path("npm").ok_or_else(|| "npm is not on PATH".to_owned()).and_then(|npm| {
        Command::new(npm)
            .args(["run", "pair", "--silent"])
            .current_dir(root.join("server"))
            .status()
            .map_err(|e| e.to_string())
    });
    match run {
        Ok(s) if s.success() => {
            out::inline("Scan it, then press Enter to start Claude : ", Fg::Cyan);
            out::flush();
            let mut line = String::new();
            drop(std::io::stdin().read_line(&mut line));
        }
        Ok(_) => out::line("  no QR printed (see the message above); starting Claude anyway", Fg::DarkYellow),
        Err(e) => out::line(&format!("  QR not shown: {e}"), Fg::DarkYellow),
    }
}

/// The first tailnet (100.x) IPv4 address.
fn tailnet(addrs: &[[u8; 4]]) -> Option<String> {
    addrs
        .iter()
        .find(|a| a[0] == 100)
        .map(|a| format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]))
}

/// The crc branch: companion server, tailnet address, pairing QR, then `crc.cmd -- argv` (the
/// `.cmd` shim, so `--` survives). `None` = crc is not on PATH (the caller prints that line and
/// falls back to plain claude); `Some(code)` = crc ran and exited with `code`.
pub fn launch_via_crc(req: &CrcLaunch<'_>) -> Option<i32> {
    let crc = crate::invocation::find_on_path("crc", &std::env::var("PATH").unwrap_or_default(), ".CMD")?;
    out::line(
        "  roaming window: session will be reachable from the phone (CLAUDE_NO_ROAM=1 to disable)",
        Fg::DarkGray,
    );
    let root = live_root();
    match start_companion_server(&root, live_port(), &log_path(std::process::id())) {
        Ok(state) if state.contains("(dist)") => {
            out::line(&format!("  companion server: {state} (serves all profiles)"), Fg::DarkGray);
            print_tailnet();
        }
        Ok(state) => {
            // The tsx fallback is pre-multi-profile code: it serves only the profile it inherits.
            out::line(
                &format!("  companion server: {state} (serving the {} profile)", req.account),
                Fg::DarkGray,
            );
            print_tailnet();
        }
        Err(e) => out::line(&format!("  companion server not started: {e}"), Fg::DarkYellow),
    }
    if req.show_qr {
        show_pairing_qr(&root);
    }
    let mut argv = vec!["--".to_owned()];
    argv.extend(req.argv.iter().cloned());
    match crate::invocation::run_child(&crc, &argv, req.child) {
        Ok(code) => Some(code),
        Err(e) => {
            out::line(&format!("  crc could not be started: {e}"), Fg::Red);
            Some(1)
        }
    }
}

fn print_tailnet() {
    match tailnet(&win::ipv4_addresses()) {
        Some(ip) => out::line(&format!("  phone over tailscale: http://{ip}:8791"), Fg::DarkGray),
        None => out::line(
            "  no tailnet address - the phone can reach this only over USB (adb reverse tcp:8791 tcp:8791)",
            Fg::DarkYellow,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = r"C:\Users\someone\Projects\remote-control-claude-code";

    /// Records what the stop seam saw.
    struct Fake {
        listener: Option<u32>,
        info: Option<ProcInfo>,
        fail: Option<String>,
        stopped: Vec<u32>,
    }

    impl Procs for Fake {
        fn listener(&mut self, _port: u16) -> Option<u32> {
            self.listener
        }
        fn info(&mut self, _pid: u32) -> Option<ProcInfo> {
            self.info.clone()
        }
        fn stop(&mut self, pid: u32) -> Result<(), String> {
            self.stopped.push(pid);
            self.fail.clone().map_or(Ok(()), Err)
        }
    }

    fn fake(listener: Option<u32>, name: &str, cmd: &str) -> Fake {
        Fake {
            listener,
            info: (!name.is_empty()).then(|| ProcInfo {
                name: name.into(),
                command_line: cmd.into(),
            }),
            fail: None,
            stopped: Vec::new(),
        }
    }

    fn stop(f: &mut Fake) -> String {
        stop_with(Path::new(ROOT), 8791, f)
    }

    #[test]
    fn nothing_listening_stops_nothing() {
        let mut f = fake(None, "", "");
        assert!(stop(&mut f).contains("nothing listening"));
        assert_eq!(f.stopped.len(), 0);
    }

    #[test]
    fn a_dead_pid_reads_as_already_stopped() {
        let mut f = fake(Some(4242), "", "");
        assert!(stop(&mut f).contains("stale socket entry"));
        assert_eq!(f.stopped.len(), 0);
    }

    #[test]
    fn a_node_from_another_checkout_is_left_alone_and_named() {
        let mut f = fake(Some(32620), "node", r"node C:\Users\someone\Projects\something-else\server.js");
        let r = stop(&mut f);
        assert_eq!(f.stopped.len(), 0);
        assert!(r.contains("not the companion server") && r.contains("32620"));
    }

    #[test]
    fn a_non_node_process_is_not_stopped_even_from_the_right_directory() {
        let mut f = fake(Some(4242), "python", &format!("python serve.py {ROOT}"));
        let r = stop(&mut f);
        assert_eq!(f.stopped.len(), 0);
        assert!(r.contains("python"));
    }

    #[test]
    fn an_unreadable_command_line_is_not_a_match() {
        let mut f = fake(Some(4242), "node", "");
        stop(&mut f);
        assert_eq!(f.stopped.len(), 0);
    }

    #[test]
    fn the_real_companion_is_stopped() {
        let mut f = fake(Some(777), "node", &format!(r"node {ROOT}\server\dist\index.js"));
        let r = stop(&mut f);
        assert_eq!(f.stopped, vec![777]);
        assert!(r.contains("stopped pid 777"));
    }

    #[test]
    fn forward_slashes_still_match_the_root() {
        let mut f = fake(
            Some(1),
            "node",
            "node C:/Users/someone/Projects/remote-control-claude-code/server/dist/index.js",
        );
        stop(&mut f);
        assert_eq!(f.stopped.len(), 1);
    }

    #[test]
    fn a_failing_stop_is_a_status_line() {
        let mut f = fake(Some(1), "node", &format!(r"node {ROOT}\server\dist\index.js"));
        f.fail = Some("access is denied".into());
        assert!(stop(&mut f).contains("access is denied"));
    }

    #[test]
    fn root_resolution() {
        let home = Path::new(r"C:\Users\x");
        assert_eq!(companion_root(Some(r"D:\checkouts\crc"), home), PathBuf::from(r"D:\checkouts\crc"));
        assert_eq!(
            companion_root(Some(""), home),
            home.join("Desktop").join("Projects").join("remote-control-claude-code")
        );
    }

    #[test]
    fn log_path_is_per_instance_in_temp() {
        let (a, b) = (log_path(101), log_path(202));
        assert_ne!(a, b);
        assert!(a.file_name().unwrap().to_string_lossy().contains("101"));
        assert_eq!(a.parent().unwrap(), std::env::temp_dir().as_path());
    }

    #[test]
    fn port_comes_from_the_config_or_defaults() {
        let d = std::env::temp_dir().join(format!("claude-auto-rs-w4-port-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(d.join("claude-remote")).unwrap();
        assert_eq!(companion_port(Some(&d)), 8791);
        std::fs::write(d.join("claude-remote").join("config.json"), r#"{"port": 9001}"#).unwrap();
        assert_eq!(companion_port(Some(&d)), 9001);
        std::fs::write(d.join("claude-remote").join("config.json"), "{not json").unwrap();
        assert_eq!(companion_port(Some(&d)), 8791);
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(companion_port(None), 8791);
    }

    #[test]
    fn tailnet_takes_the_first_100_address() {
        assert_eq!(
            tailnet(&[[127, 0, 0, 1], [100, 64, 1, 2], [100, 9, 9, 9]]).as_deref(),
            Some("100.64.1.2")
        );
        assert_eq!(tailnet(&[[192, 168, 1, 2]]), None);
    }

    #[test]
    fn a_stale_dist_is_rebuilt_and_a_fresh_one_is_not() {
        let d = std::env::temp_dir().join(format!("claude-auto-rs-w4-dist-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(d.join("src").join("lib")).unwrap();
        std::fs::create_dir_all(d.join("dist")).unwrap();
        std::fs::write(d.join("package.json"), r#"{"scripts":{"build":"tsc"}}"#).unwrap();
        std::fs::write(d.join("src").join("lib").join("a.ts"), "x").unwrap();
        assert!(dist_stale(&d), "no dist is stale");
        std::fs::write(d.join("dist").join("index.js"), "x").unwrap();
        let later = SystemTime::now() + Duration::from_secs(60);
        File::options()
            .write(true)
            .open(d.join("dist").join("index.js"))
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(!dist_stale(&d), "a dist newer than every source is current");
        File::options()
            .write(true)
            .open(d.join("src").join("lib").join("a.ts"))
            .unwrap()
            .set_modified(later + Duration::from_secs(60))
            .unwrap();
        assert!(dist_stale(&d), "a nested source newer than dist makes it stale");
        assert!(has_build_script(&d).unwrap());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
