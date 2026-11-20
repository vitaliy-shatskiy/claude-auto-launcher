//! What the launcher prints or logs beside the launch itself: the start record, the `--timings`
//! table, the preview banner and the preview summary.

use std::path::Path;

use serde_json::{Value, json};

use crate::log;
use crate::log_data;
use crate::out::{self, Fg};
use crate::types::ChildEnv;
use crate::{sys, timing};

use super::{Run, VERSION, var};

/// `--timings [--days N]`: the table, then exit without the UI.
pub(super) fn print_timings(log_root: &Path, days: Result<u32, String>) -> i32 {
    let days = match days {
        Ok(d) => d,
        Err(e) => {
            out::err_line(&format!("claude-auto: {e}"));
            return 1;
        }
    };
    let today = sys::local_now().map_or((1970, 1, 1), |t| (i64::from(t.year), i64::from(t.month), i64::from(t.day)));
    for l in timing::summary::report(&timing::summary::log_files(log_root, today, days)) {
        out::line(&l, Fg::Plain);
    }
    0
}

pub(super) fn log_start(run: &Run, input_redirected: bool, output_redirected: bool) {
    log::prune(&run.logger.root, 14);
    let env_names = [
        "TERMINAL_EMULATOR",
        "TERM_PROGRAM",
        "WT_SESSION",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_NO_ROAM",
        "CLAUDE_CODE_SSE_PORT",
    ];
    let env_map: serde_json::Map<String, Value> = env_names.iter().map(|n| ((*n).to_owned(), json!(var(n)))).collect();
    run.logger.write(
        "start",
        log_data! {
            "cwd" => run.ctx.cwd.to_string_lossy(),
            "moduleMs" => 0,
            "args" => run.args,
            "inputRedirected" => input_redirected,
            "outputRedirected" => output_redirected,
            "psVersion" => format!("rust {VERSION}"),
            "configDir" => var("CLAUDE_CONFIG_DIR"),
            "env" => env_map,
            "parents" => log::parent_chain(6),
        },
    );
}

pub(super) fn preview_banner(title: &str) {
    out::line("", Fg::Plain);
    out::line(title, Fg::Cyan);
    out::line(
        "in an interactive session this returns to the launch screen; preview cannot loop because the scripted key list is finite",
        Fg::Plain,
    );
}

#[expect(clippy::too_many_arguments, reason = "one line per preview field")]
pub(super) fn preview_summary(
    choice: &str,
    child: &ChildEnv,
    remote: bool,
    qr: bool,
    stop: bool,
    launch_argv: &[String],
    claude: &Path,
    argv: &[String],
) {
    let ps_bool = |b: bool| if b { "True" } else { "False" };
    out::line("", Fg::Plain);
    out::line("--- preview, nothing launched ---", Fg::Cyan);
    out::line(&format!("account            : {choice}"), Fg::Plain);
    out::line(
        &format!("CLAUDE_CONFIG_DIR  : {}", child.effective("CLAUDE_CONFIG_DIR").unwrap_or_default()),
        Fg::Plain,
    );
    out::line(
        &format!(
            "remote             : {}   qr={}   stopServer={}",
            ps_bool(remote),
            ps_bool(qr),
            ps_bool(stop)
        ),
        Fg::Plain,
    );
    let advisor = if child.effective("CLAUDE_CODE_DISABLE_ADVISOR_TOOL").as_deref() == Some("1") {
        "disabled (CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1)"
    } else {
        "enabled"
    };
    out::line(&format!("advisor tool       : {advisor}"), Fg::Plain);
    out::line(&format!("launch args        : {}", launch_argv.join(" ")), Fg::Plain);
    out::line(&format!("command            : {} {}", claude.display(), argv.join(" ")), Fg::Plain);
    out::line(&format!("argv count         : {}", argv.len()), Fg::Plain);
}
