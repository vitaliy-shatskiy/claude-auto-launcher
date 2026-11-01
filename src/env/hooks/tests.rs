use super::*;
#[test]
fn hooks_run_in_order_and_a_failure_is_one_line() {
    let dir = std::env::temp_dir().join(format!("claude-auto-rs-hooks-{}", crate::log::new_run_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let put = |n: &str, body: &str| {
        let p = dir.join(n);
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().into_owned()
    };
    let one = put("one.ps1", "Write-Output \"one ran\"; Write-Output \"acct=$env:CLAUDE_CONFIG_DIR\"");
    let two = put("two.ps1", "Write-Output \"two broke\"; exit 3");
    let three = put("three.ps1", "Write-Output \"three ran\"");
    let four = put("four.hook", "not a script");
    let missing = dir.join("missing.ps1").to_string_lossy().into_owned();
    let mut env = ChildEnv::default();
    env.set("CLAUDE_CONFIG_DIR", r"C:\fixture-root");
    let tm = Timings::on(std::time::Instant::now());
    let lines: Vec<String> = run(&singles(&[&one, &two, &missing, &four, &three]), &env, &tm)
        .into_iter()
        .map(|n| n.text)
        .collect();
    std::fs::remove_dir_all(&dir).unwrap();
    let exits: Vec<(String, Option<i32>)> = tm.take_unlogged().into_iter().map(|r| (r.name, r.exit)).collect();
    for (name, exit) in [
        ("hooks:one.ps1", Some(0)),
        ("hooks:two.ps1", Some(3)),
        ("hooks:missing.ps1", None),
        ("hooks:four.hook", None),
        ("hooks:three.ps1", Some(0)),
    ] {
        assert!(exits.contains(&(name.to_owned(), exit)), "{name} {exit:?} in {exits:?}");
    }
    assert!(lines.contains(&"one ran".to_owned()), "hook stdout is printed verbatim: {lines:?}");
    assert!(
        lines.contains(&r"acct=C:\fixture-root".to_owned()),
        "a hook sees the session's environment"
    );
    assert_eq!(
        lines.iter().filter(|l| l.trim_start().starts_with("hook two.ps1 exit 3")).count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.contains("missing.ps1") && l.contains("not found"))
            .count(),
        1
    );
    assert_eq!(
        lines.iter().filter(|l| l.contains("four.hook")).count(),
        1,
        "an unknown extension is refused, one line"
    );
    assert!(lines.iter().any(|l| l.contains("four.hook") && l.contains("unsupported extension")));
    assert!(lines.contains(&"three ran".to_owned()), "later hooks still run");
    assert!(run(&[], &env, &Timings::default()).is_empty(), "an empty list prints nothing");
}

/// Two-party rendezvous: each fake hook checks in and waits for the other. Hooks run one
/// after another never meet, so the first gives up after the bound and says so.
#[derive(Default)]
struct Meet {
    arrived: std::sync::Mutex<usize>,
    cv: std::sync::Condvar,
}

impl Meet {
    fn arrive(&self) -> bool {
        let mut n = self.arrived.lock().unwrap();
        *n += 1;
        self.cv.notify_all();
        let (n, _) = self
            .cv
            .wait_timeout_while(n, std::time::Duration::from_millis(250), |n| *n < 2)
            .unwrap();
        *n >= 2
    }
}

fn note(t: &str) -> Vec<Note> {
    vec![Note::new(t, Fg::DarkGray)]
}

/// Each path its own top-level entry, as a plain string in the config gives.
fn singles(hooks: &[&str]) -> Vec<Vec<String>> {
    hooks.iter().map(|h| vec![(*h).to_owned()]).collect()
}

#[test]
fn a_chain_runs_in_order_while_other_entries_run_beside_it() {
    // `c` meets `a`: the chain runs beside the other entry. `b` starts only after `a` ended.
    let meet = Meet::default();
    let events = std::sync::Mutex::new(Vec::<String>::new());
    let chains = vec![vec!["a".to_owned(), "b".to_owned()], vec!["c".to_owned()]];
    let notes = run_with(&chains, &ChildEnv::default(), &Timings::default(), |h, _| {
        events.lock().unwrap().push(format!("start {h}"));
        let outcome = if h == "b" {
            "-"
        } else if meet.arrive() {
            "met"
        } else {
            "alone"
        };
        events.lock().unwrap().push(format!("end {h}"));
        note(&format!("{h} {outcome}"))
    });
    let lines: Vec<String> = notes.into_iter().map(|n| n.text).collect();
    assert_eq!(lines, ["a met", "b -", "c met"], "config order, flattened");
    let ev = events.into_inner().unwrap();
    let at = |e: &str| ev.iter().position(|x| x == e).unwrap();
    assert!(at("end a") < at("start b"), "b waited for a: {ev:?}");
}

#[test]
fn hooks_run_at_the_same_time() {
    let meet = Meet::default();
    let hooks = singles(&["a.ps1", "b.ps1"]);
    let lines: Vec<String> = run_with(&hooks, &ChildEnv::default(), &Timings::default(), |h, _| {
        note(&format!("{h} {}", if meet.arrive() { "met" } else { "alone" }))
    })
    .into_iter()
    .map(|n| n.text)
    .collect();
    assert_eq!(lines, ["a.ps1 met", "b.ps1 met"], "both hooks were running at once");
}

#[test]
fn output_keeps_config_order_whoever_finishes_first() {
    // The first hook finishes only after the last has returned its lines.
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let done_tx = std::sync::Mutex::new(done_tx);
    let done_rx = std::sync::Mutex::new(done_rx);
    let hooks = singles(&["first.ps1", "", "last.ps1"]);
    let notes = run_with(&hooks, &ChildEnv::default(), &Timings::default(), |h, _| {
        if h == "first.ps1" {
            let got = done_rx.lock().unwrap().recv_timeout(std::time::Duration::from_millis(250));
            return note(if got.is_ok() { "first 1" } else { "first ran alone" })
                .into_iter()
                .chain(note("first 2"))
                .collect();
        }
        done_tx.lock().unwrap().send(()).unwrap();
        note("last 1")
    });
    let lines: Vec<String> = notes.into_iter().map(|n| n.text).collect();
    assert_eq!(lines, ["first 1", "first 2", "last 1"]);
}

#[test]
fn a_hook_that_panics_is_one_line_and_the_others_still_print() {
    let hooks = vec![vec!["ok.ps1".to_owned()], vec!["boom.ps1".to_owned(), "after.ps1".to_owned()]];
    let notes = run_with(&hooks, &ChildEnv::default(), &Timings::default(), |h, _| {
        assert_ne!(h, "boom.ps1", "fake hook failure");
        note(&format!("{h} ran"))
    });
    let lines: Vec<(String, Fg)> = notes.into_iter().map(|n| (n.text, n.fg)).collect();
    assert_eq!(lines.first(), Some(&("ok.ps1 ran".to_owned(), Fg::DarkGray)));
    assert_eq!(lines.get(1).map(|l| &l.1), Some(&Fg::DarkYellow));
    assert!(
        lines
            .get(1)
            .is_some_and(|l| l.0.contains("hook boom.ps1") && l.0.contains("failed"))
    );
    assert_eq!(lines.get(2), Some(&("after.ps1 ran".to_owned(), Fg::DarkGray)));
    assert_eq!(lines.len(), 3);
}

/// Points this process's stdin at `file` until dropped: what `Stdio::inherit` hands a child.
struct StdinSwap(windows_sys::Win32::Foundation::HANDLE);

impl StdinSwap {
    #[expect(unsafe_code, reason = "GetStdHandle/SetStdHandle FFI")]
    fn to(file: &std::fs::File) -> Self {
        use std::os::windows::io::AsRawHandle as _;
        use windows_sys::Win32::System::Console::{GetStdHandle, STD_INPUT_HANDLE, SetStdHandle};
        // SAFETY: reads the process's current stdin handle.
        let old = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        // SAFETY: `file` outlives the swap, which restores `old` on drop.
        assert_ne!(unsafe { SetStdHandle(STD_INPUT_HANDLE, file.as_raw_handle()) }, 0);
        Self(old)
    }
}

impl Drop for StdinSwap {
    #[expect(unsafe_code, reason = "SetStdHandle FFI")]
    fn drop(&mut self) {
        use windows_sys::Win32::System::Console::{STD_INPUT_HANDLE, SetStdHandle};
        // SAFETY: puts back the handle `to` read.
        unsafe { SetStdHandle(STD_INPUT_HANDLE, self.0) };
    }
}

#[test]
fn a_hook_reads_the_launchers_stdin() {
    let dir = std::env::temp_dir().join(format!("claude-auto-rs-hook-stdin-{}", crate::log::new_run_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input.txt");
    std::fs::write(&input, "typed-into-the-console\n").unwrap();
    let hook = dir.join("reads.ps1");
    std::fs::write(&hook, "$l = [Console]::In.ReadLine(); Write-Output \"got=$l\"").unwrap();
    let file = std::fs::File::open(&input).unwrap();
    let lines: Vec<String> = {
        let _swap = StdinSwap::to(&file);
        run(
            &[vec![hook.to_string_lossy().into_owned()]],
            &ChildEnv::default(),
            &Timings::default(),
        )
        .into_iter()
        .map(|n| n.text)
        .collect()
    };
    drop(file);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        lines,
        vec!["got=typed-into-the-console".to_owned()],
        "the hook got the line piped in"
    );
}

/// A `timing` record as (name, parent, exit).
type Rec = (String, Option<String>, Option<i32>);

/// The screen lines and the `timing` records of one fake hook phase, in log order.
fn hook_records(tm: &Timings, chains: &[Vec<String>]) -> (Vec<String>, Vec<Rec>) {
    let phase = tm.begin(&["hooks"]);
    let notes = run_with(chains, &ChildEnv::default(), tm, |h, _| {
        assert_ne!(h, "boom.ps1", "fake hook failure");
        let exit = if h.ends_with("b.ps1") { 2 } else { 0 };
        Ran {
            notes: note(&format!("{h} ran")),
            exit: Some(exit),
        }
    });
    tm.end(phase);
    let records = tm.take_unlogged().into_iter().map(|r| (r.name, r.parent, r.exit)).collect();
    (notes.into_iter().map(|n| n.text).collect(), records)
}

fn fixture_chains() -> Vec<Vec<String>> {
    vec![
        vec!["a.ps1".to_owned()],
        vec![r"dir\b.ps1".to_owned(), "c.ps1".to_owned()],
        vec!["boom.ps1".to_owned()],
    ]
}

#[test]
fn timing_on_records_each_hook_and_chain_under_hooks_with_exit_codes() {
    let tm = Timings::on(std::time::Instant::now());
    let (_, mut got) = hook_records(&tm, &fixture_chains());
    got.sort();
    let r = |n: &str, p: Option<&str>, e: Option<i32>| (n.to_owned(), p.map(str::to_owned), e);
    let chain = "hooks:chain:b.ps1+c.ps1";
    assert_eq!(
        got,
        vec![
            r("hooks", None, None),
            r("hooks:a.ps1", Some("hooks"), Some(0)),
            r("hooks:b.ps1", Some(chain), Some(2)),
            r("hooks:boom.ps1", Some("hooks"), None),
            r("hooks:c.ps1", Some(chain), Some(0)),
            r(chain, Some("hooks"), None),
        ],
        "a panicking hook keeps its own record and the others'"
    );
}

#[test]
fn timing_off_records_nothing_and_reads_no_clock() {
    let tm = Timings::default();
    let (_, got) = hook_records(&tm, &fixture_chains());
    assert!(got.is_empty() && tm.spans().is_empty());
    let (_, clocks) = run_clocked(&fixture_chains()[..2], &ChildEnv::default(), false, &|h: &str, _: &ChildEnv| {
        note(h)
    });
    assert!(clocks.is_empty(), "timing off measures nothing");
}

#[test]
fn timing_changes_no_line_on_screen() {
    let (on, _) = hook_records(&Timings::on(std::time::Instant::now()), &fixture_chains());
    let (off, _) = hook_records(&Timings::default(), &fixture_chains());
    assert_eq!(on, off);
    assert_eq!(on.len(), 4, "{on:?}");
}
