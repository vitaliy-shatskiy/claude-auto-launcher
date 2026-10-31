//! Entry point: every argument goes to the launcher untouched; its result is the exit code.

fn main() {
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    std::process::exit(claude_auto::app::run(args));
}
