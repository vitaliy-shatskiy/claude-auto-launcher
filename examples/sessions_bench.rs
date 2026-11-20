//! Times the session picker's cold first page against a real projects root, read-only: the summary
//! cache goes to the path given as the second argument, never beside the projects root. A dev tool,
//! never in the release exe. `cargo run --release --example sessions_bench -- <projects root>
//! <cache.json> [runs] [--cold] [--rows]` (`--cold` deletes the cache before every run, `--rows`
//! prints each summarised session's count and title). The page loop is
//! the picker's: pages of 10 until 10 resumable rows. Exit 0 · 2 bad arguments.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use claude_auto::sessions::frame::resumable;
use claude_auto::sessions::listing::{Scope, SessionCache, session_files};

#[expect(clippy::print_stdout, reason = "the bench's only output")]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(root), Some(cache)) = (args.first().map(PathBuf::from), args.get(1).map(PathBuf::from)) else {
        return ExitCode::from(2);
    };
    let runs: usize = args.get(2).and_then(|r| r.parse().ok()).unwrap_or(3);
    let cold = args.iter().any(|a| a == "--cold");
    for run in 1..=runs {
        if cold && cache.exists() && std::fs::remove_file(&cache).is_err() {
            return ExitCode::from(2);
        }
        let t = Instant::now();
        let snap = session_files(&root, &[]);
        let listed = Instant::now();
        let mut store = SessionCache::new(false);
        let mut rows = Vec::new();
        while rows.iter().filter(|s| resumable(s)).count() < 10 {
            let page = store.list(&root, Scope::Files(&snap), rows.len(), 10, &cache);
            if page.is_empty() {
                break;
            }
            rows.extend(page);
        }
        if args.iter().any(|a| a == "--rows") {
            for s in &rows {
                println!("{} {} msgs | {}", s.path.display(), s.message_count, s.title);
            }
        }
        let bytes: u64 = rows.iter().map(|s| s.size_bytes).sum();
        println!(
            "run {run}: {:.1} ms total, snapshot {:.1} ms, {} files, {} summarised ({} MB)",
            t.elapsed().as_secs_f64() * 1000.0,
            listed.duration_since(t).as_secs_f64() * 1000.0,
            snap.len(),
            rows.len(),
            bytes / 1_048_576
        );
    }
    ExitCode::SUCCESS
}
