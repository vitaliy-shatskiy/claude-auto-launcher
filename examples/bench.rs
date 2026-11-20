//! The speed budget's table: every path of `tests/perf` on the same synthetic world, its work
//! counts and its wall time (median and p80 of `runs`). The counts are what `perf/budget.json`
//! gates; the times are a reference only (load-dependent). A dev tool, never in the release exe;
//! a file-system timing, so a plain loop, not Criterion (no new crate). Build the exe first:
//! `cargo build --release --bins --examples`, then `target\release\examples\bench.exe [runs] [--json]`
//! (`--json` prints the counts as `perf/budget.json` `paths`). Exit 0 · 1 a path failed · 2 bad arguments.

#[path = "../tests/perf/world.rs"]
mod world;

use std::process::ExitCode;
use std::time::Duration;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// The value at `pct` % of sorted `xs` (nearest rank).
fn quantile(xs: &[f64], pct: usize) -> f64 {
    let rank = (pct * xs.len()).div_ceil(100).max(1);
    xs.get(rank - 1).copied().unwrap_or(f64::NAN)
}

#[expect(clippy::print_stdout, reason = "the bench's only output")]
#[expect(clippy::print_stderr, reason = "the bench's failure line")]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Ok(runs) = args.first().filter(|a| !a.starts_with("--")).map_or(Ok(10), |r| r.parse::<usize>()) else {
        return ExitCode::from(2);
    };
    let json = args.iter().any(|a| a == "--json");
    // target\<profile>\examples\bench.exe: the exe two levels up, the world in target\tmp.
    let profile = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.parent()?.to_path_buf()));
    let exe = profile.as_ref().map(|p| p.join("claude-auto.exe")).filter(|p| p.is_file());
    let w = profile.as_ref().and_then(|p| p.parent()).map(|t| world::world(&t.join("tmp")));
    let (Some(exe), Some(Ok(w))) = (exe, w) else {
        eprintln!("no claude-auto.exe beside examples\\ (build --bins first), or no world");
        return ExitCode::from(2);
    };
    let mut budget = serde_json::Map::new();
    println!("{:<18} {:>10} {:>10}  counts", "path", "median ms", "p80 ms");
    for (name, run) in world::PATHS {
        let mut times = Vec::with_capacity(runs);
        let mut counts = Vec::new();
        for _ in 0..runs.max(1) {
            match run(&exe, &w) {
                Ok((c, took)) => {
                    counts = c;
                    times.push(ms(took));
                }
                Err(e) => {
                    eprintln!("{name}: {e}");
                    return ExitCode::from(1);
                }
            }
        }
        times.sort_by(f64::total_cmp);
        let shown: Vec<String> = counts.iter().map(|(m, v)| format!("{m}={v}")).collect();
        println!(
            "{name:<18} {:>10.3} {:>10.3}  {}",
            quantile(&times, 50),
            quantile(&times, 80),
            shown.join(" ")
        );
        let c: serde_json::Map<String, serde_json::Value> = counts.iter().map(|(m, v)| ((*m).to_owned(), (*v).into())).collect();
        budget.insert(name.to_owned(), serde_json::json!({ "counts": c }));
    }
    if json {
        println!("{}", serde_json::Value::Object(budget));
    }
    ExitCode::SUCCESS
}
