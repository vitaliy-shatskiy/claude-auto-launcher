//! The cache file's neighbours: the temps a write leaves and the cache under its pre-0.9 name.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::CACHE_NAME;

pub(super) const LEGACY_CACHE_NAME: &str = "claude-auto-rs-sessions.json";

/// The cache under its pre-0.9 name beside `path`, removed once a write under the new name landed.
/// Exactly that name: its stranded temps and look-alikes are not ours to judge.
pub(super) fn remove_legacy_cache(path: &Path) {
    let old = path.with_file_name(LEGACY_CACHE_NAME);
    if path.file_name().is_some_and(|n| n == CACHE_NAME) && old.is_file() && std::fs::remove_file(&old).is_err() {
        // Held by another instance: the next write retries.
    }
}

pub(super) fn temp_suffix() -> u128 {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    t ^ (u128::from(std::process::id()) << 64)
}

pub(super) fn sweep_stale_temps(path: &Path) {
    let (Some(dir), Some(leaf)) = (path.parent(), path.file_name()) else {
        return;
    };
    let prefix = format!("{}.", leaf.to_string_lossy());
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let cutoff = SystemTime::now().checked_sub(Duration::from_secs(60)).unwrap_or(UNIX_EPOCH);
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < cutoff);
        let tmp = Path::new(&name).extension().is_some_and(|x| x.eq_ignore_ascii_case("tmp"));
        if name.starts_with(&prefix) && tmp && old && std::fs::remove_file(e.path()).is_err() {
            // Another instance may be removing it at the same moment.
        }
    }
}
