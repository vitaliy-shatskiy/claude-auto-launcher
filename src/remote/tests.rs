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
