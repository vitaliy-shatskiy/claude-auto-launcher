//! Times the project registry build against the real projects root, read-only: the derived-data
//! cache goes to the path given as the first argument, never beside the projects root. A dev tool,
//! never in the release exe; a file-system timing, so a plain loop, not Criterion (no new crate).
//! `cargo run --release --example registry_bench -- <cache.json> [runs] [--cold]` (`--cold` deletes
//! the cache before every run). Exit 0 · 2 bad arguments.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

#[expect(clippy::print_stdout, reason = "the bench's only output")]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cache) = args.first().map(PathBuf::from) else {
        return ExitCode::from(2);
    };
    let runs: usize = args.get(1).and_then(|r| r.parse().ok()).unwrap_or(5);
    let cold = args.iter().any(|a| a == "--cold");
    for run in 1..=runs {
        if cold && cache.exists() && std::fs::remove_file(&cache).is_err() {
            return ExitCode::from(2);
        }
        let t = Instant::now();
        let reg = std::hint::black_box(claude_auto::projects::project_registry(Some(&cache), &[], false));
        let total = t.elapsed();
        let ms = |p: &claude_auto::projects::Phase| p.end.duration_since(p.start).as_secs_f64() * 1000.0;
        let phases: Vec<String> = reg.phases.iter().map(|p| format!("{} {:.1}", p.name, ms(p))).collect();
        println!(
            "run {run}: {:.1} ms, {} projects | {}",
            total.as_secs_f64() * 1000.0,
            reg.projects.len(),
            phases.join(" | ")
        );
    }
    ExitCode::SUCCESS
}
