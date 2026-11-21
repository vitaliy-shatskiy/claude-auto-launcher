//! The running frame of an update with a launcher entry.

use super::*;

/// `FakeOps` with one launcher entry, as a known newer release gives.
struct WithLauncher(FakeOps);

impl Ops for WithLauncher {
    fn info(&mut self) -> InstallInfo {
        self.0.info()
    }
    fn update(&mut self, step: Step<'_>) -> Verdict {
        self.0.update(step)
    }
    fn rename_swap(&mut self) -> Verdict {
        self.0.rename_swap()
    }
    fn command_text(&mut self, args: &[&str], step: Step<'_>) -> Verdict {
        self.0.command_text(args, step)
    }
    fn prune(&mut self) -> Pruned {
        self.0.prune()
    }
    fn run(&mut self, script: &Path, step: Step<'_>) -> Result<ScriptRun, String> {
        self.0.run(script, step)
    }
    fn load_rows(&mut self) -> RowsConfig {
        self.0.load_rows()
    }
    fn save_rows(&mut self, rows: &[RowName], hidden: &BTreeMap<RowName, String>) -> Result<(), String> {
        self.0.save_rows(rows, hidden)
    }
    fn launcher_entries(&mut self) -> Vec<(char, String)> {
        vec![('c', "update claude-auto to 9.9.9".into())]
    }
}

#[test]
fn the_running_frame_keeps_the_launcher_rows() {
    let mut ops = WithLauncher(FakeOps {
        log: Rc::default(),
        exit_code: 0,
    });
    let mut t = T {
        inner: PreviewTerm::new((100, 30), VecDeque::new(), true),
        q: vec![key('u')].into(),
        clock: Vec::new(),
        now: None,
        frames: Vec::new(),
        drains: 0,
        size: (100, 30),
    };
    let mut s = MaintScreen::new(&mut ops, &[], plain());
    assert_eq!(run(&mut s, &mut t), Err(UiStop::KeysExhausted));
    let running = t.frames.iter().find(|f| f.contains("updating")).unwrap();
    assert!(running.contains("update claude-auto to 9.9.9"), "{running}");
}
