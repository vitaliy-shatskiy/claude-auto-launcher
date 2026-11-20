//! The session picker over every account's store: with `sharing` off a session may live in another
//! profile, and resuming it there is a file copy into the launching profile's own store. Exports
//! `OtherStore`, `Transfer`, the snapshot join and tag, and the copy.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::listing::{Session, session_files};

/// Another account's projects store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherStore {
    pub account: String,
    pub root: PathBuf,
}

/// A picked session that lives in another account's store, copied before claude starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    pub session_id: String,
    pub source: PathBuf,
    pub from_account: String,
}

fn modified(p: &Path) -> SystemTime {
    std::fs::metadata(p).and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH)
}

/// The launching account's snapshot plus every other store's, newest first,
/// one row per session id: a session transferred once exists twice, the newer copy wins.
pub fn joined_snapshot(primary: Vec<PathBuf>, others: &[OtherStore], slugs: &[String]) -> Vec<PathBuf> {
    if others.is_empty() {
        return primary;
    }
    let mut all: Vec<(PathBuf, SystemTime)> = primary
        .into_iter()
        .chain(others.iter().flat_map(|o| session_files(&o.root, slugs)))
        .map(|p| {
            let t = modified(&p);
            (p, t)
        })
        .collect();
    all.sort_by_key(|x| std::cmp::Reverse(x.1));
    let mut seen = HashSet::new();
    all.into_iter()
        .filter(|(p, _)| seen.insert(p.file_name().map(|n| n.to_string_lossy().to_lowercase())))
        .map(|(p, _)| p)
        .collect()
}

/// Names the other account whose store holds this session; a session of
/// the launching account keeps an empty tag.
pub fn tag_sessions(sessions: &mut [Session], others: &[OtherStore]) {
    for s in sessions {
        s.account = others
            .iter()
            .find(|o| s.path.starts_with(&o.root))
            .map_or_else(String::new, |o| o.account.clone());
    }
}

/// The transfer a pick needs, when the session lives in another account's store.
pub fn transfer_of(s: &Session) -> Option<Transfer> {
    (!s.account.is_empty()).then(|| Transfer {
        session_id: s.session_id.clone(),
        source: s.path.clone(),
        from_account: s.account.clone(),
    })
}

/// The transcript into the target store, slug directory preserved (the
/// slug is how claude maps a transcript back to its directory). An up-to-date target is never
/// touched; an older one is renamed to a `.bak` sibling, which the `*.jsonl` scan never offers.
pub fn copy_session_to_profile(source: &Path, target_root: &Path) -> std::io::Result<PathBuf> {
    let missing = || std::io::Error::new(std::io::ErrorKind::InvalidInput, "transcript path has no slug directory");
    let slug = source.parent().and_then(Path::file_name).ok_or_else(missing)?;
    let name = source.file_name().ok_or_else(missing)?;
    let source_time = std::fs::metadata(source)?.modified()?;
    let dir = target_root.join(slug);
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(name);
    if target.exists() {
        if modified(&target) >= source_time {
            return Ok(target);
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        std::fs::rename(&target, format!("{}.{stamp}.bak", target.display()))?;
    }
    std::fs::copy(source, &target)?;
    Ok(target)
}

/// The step before claude starts on a picked foreign session: the line to show, or the reason the
/// launch stops. Preview copies nothing. A failed copy is `Err`: claude would open on "no such
/// session", which reads like the picker lied.
pub fn transfer_before_launch(t: &Transfer, target_root: &Path, launching: &str, preview: bool) -> Result<String, String> {
    if preview {
        return Ok(format!(
            "session transfer   : {} from {} -> {launching} (copied on a real launch)",
            t.session_id, t.from_account
        ));
    }
    match copy_session_to_profile(&t.source, target_root) {
        Ok(_) => {
            let short: String = t.session_id.chars().take(8).collect();
            Ok(format!("  session {short} copied: {} -> {launching}", t.from_account))
        }
        Err(e) => Err(format!("  session transfer failed, not launching: {e}")),
    }
}

#[cfg(test)]
mod tests;
