//! The last maintenance MCP check, kept so the launch page can name it without running the slow
//! `claude mcp list` itself: `~\.claude\claude-auto-mcp.json`. Exports `remember`, `last_check`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Seen {
    connected: usize,
    total: usize,
    at_ms: i64,
}

/// The file under the home folder.
pub(crate) fn path(home: &Path) -> PathBuf {
    home.join(".claude").join("claude-auto-mcp.json")
}

/// Stores one check's counts; a failed write only costs the launch page its MCP line.
pub(crate) fn remember(file: &Path, connected: usize, total: usize, now_ms: i64) {
    let seen = Seen {
        connected,
        total,
        at_ms: now_ms,
    };
    if let Ok(bytes) = serde_json::to_vec(&seen)
        && crate::atomic_file::write_atomic(file, &bytes).is_err()
    {
        // Read-only or held: the launch page shows no MCP line until the next good write.
    }
}

/// `2 of 2 · 3 h ago` from the file at `now_ms`; empty when none was stored or it does not parse.
pub(crate) fn last_check(file: &Path, now_ms: i64, dot: char) -> String {
    let Some(s) = std::fs::read(file).ok().and_then(|b| serde_json::from_slice::<Seen>(&b).ok()) else {
        return String::new();
    };
    let age = crate::projects::relative_age(s.at_ms, now_ms);
    let age = if age == "now" {
        "just now".to_owned()
    } else {
        format!("{age} ago")
    };
    format!("{} of {} {dot} {age}", s.connected, s.total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_check_reads_back_with_its_age_and_a_missing_one_is_empty() {
        let dir = std::env::temp_dir().join(format!("cars-mcp-seen-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("claude-auto-mcp.json");
        assert_eq!(last_check(&file, 0, '.'), "");
        remember(&file, 2, 3, 1_000_000);
        assert_eq!(last_check(&file, 1_000_000 + 3 * 3_600_000, '.'), "2 of 3 . 3 h ago");
        assert_eq!(last_check(&file, 1_000_000, '.'), "2 of 3 . just now");
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(last_check(&file, 0, '.'), "", "a broken file is no line");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
