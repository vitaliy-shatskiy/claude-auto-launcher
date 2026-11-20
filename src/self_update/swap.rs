//! Download, verify and swap the launcher's own exe, and the rollback. Windows lets a running exe
//! be renamed, never overwritten: the running one moves to `.old`, the verified download takes its
//! name, and every failed step puts back what it moved. Exports `Files`, `RealFiles`, `download`,
//! `swap_in`, `roll_back`, `refusal`, `sibling`.

use std::io;
use std::path::{Path, PathBuf};

use super::Net;
use super::check::Release;

/// A release exe is about 10 MB; a larger answer is no release of ours.
const MAX_EXE: usize = 50 * 1024 * 1024;
const MAX_SHA: usize = 4096;

/// The renames of a swap; the seam that lets a test fail one.
pub(crate) trait Files: Send + Sync {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
}

/// `std::fs::rename`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RealFiles;

impl Files for RealFiles {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
}

/// `exe` with `suffix` appended to its full name (`claude-auto.exe.old`).
pub(crate) fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Why this exe may not replace itself: a build under `target\` is a development build.
pub(crate) fn refusal(exe: &Path) -> Option<String> {
    exe.components().any(|c| c.as_os_str().eq_ignore_ascii_case("target")).then(|| {
        format!(
            "{} is a build directory; self-update replaces an installed exe only",
            exe.parent().unwrap_or(exe).display()
        )
    })
}

/// The first token of a `.sha256` file when it is 64 hex digits, upper case.
fn expected_hash(text: &[u8]) -> Option<String> {
    let first = std::str::from_utf8(text).ok()?.split_whitespace().next()?;
    (first.len() == 64 && first.bytes().all(|b| b.is_ascii_hexdigit())).then(|| first.to_ascii_uppercase())
}

fn remove_quietly(path: &Path) {
    if std::fs::remove_file(path).is_err() {
        // Already gone, or a later download overwrites it.
    }
}

/// Downloads the release exe beside `exe` as `.<pid>-<id>.new` and checks it against the published SHA-256:
/// the verified file, or why not. A file that fails the check is deleted; `exe` is never touched.
pub(crate) fn download(exe: &Path, release: &Release, net: &dyn Net) -> Result<PathBuf, String> {
    if let Some(r) = refusal(exe) {
        return Err(r);
    }
    let sums = net
        .get(&release.sha_url, MAX_SHA)
        .map_err(|e| format!("the checksum download failed: {e}"))?;
    let want = expected_hash(&sums).ok_or("the published checksum is not a SHA-256")?;
    let body = net
        .get(&release.exe_url, MAX_EXE)
        .map_err(|e| format!("the download failed: {e}"))?;
    // A name of its own per download: another launcher process never writes into the file this one
    // hashed and then moves in.
    let new = sibling(exe, &format!(".{}-{}.new", std::process::id(), crate::log::new_run_id()));
    std::fs::write(&new, &body).map_err(|e| format!("cannot write into {}: {e}", exe.parent().unwrap_or(exe).display()))?;
    // A file this process wrote carries no mark of the web; one left by another tool would make
    // SmartScreen ask on the next start.
    remove_quietly(&sibling(&new, ":Zone.Identifier"));
    let got = crate::maintenance::win::sha256_file(&new);
    if !got.is_some_and(|g| g.eq_ignore_ascii_case(&want)) {
        remove_quietly(&new);
        return Err(format!(
            "the download does not match its published SHA-256 and was deleted; {} is untouched",
            exe.display()
        ));
    }
    Ok(new)
}

/// Puts the verified `new` in `exe`'s place and keeps the current exe as `.old`. A failed step
/// puts back what it moved, so `exe` always holds a runnable launcher.
pub(crate) fn swap_in(exe: &Path, new: &Path, files: &dyn Files) -> Result<(), String> {
    let old = sibling(exe, ".old");
    if old.exists()
        && let Err(e) = std::fs::remove_file(&old)
    {
        remove_quietly(new);
        return Err(format!(
            "the previous build {} cannot be removed ({e}); restart claude-auto and try again",
            old.display()
        ));
    }
    if let Err(e) = files.rename(exe, &old) {
        remove_quietly(new);
        return Err(format!("cannot move {} aside: {e}", exe.display()));
    }
    if let Err(e) = files.rename(new, exe) {
        let back = files.rename(&old, exe);
        remove_quietly(new);
        return Err(match back {
            Ok(()) => format!("cannot move the new build in ({e}); the current one was put back"),
            Err(b) => format!(
                "cannot move the new build in ({e}) nor put the current one back ({b}); it is at {}",
                old.display()
            ),
        });
    }
    Ok(())
}

/// Swaps `.old` back in; the current exe becomes `.old`. A failed step puts back what it moved.
pub(crate) fn roll_back(exe: &Path, files: &dyn Files) -> Result<(), String> {
    if let Some(r) = refusal(exe) {
        return Err(r);
    }
    let old = sibling(exe, ".old");
    if !old.exists() {
        return Err("there is no previous build to roll back to".into());
    }
    let parked = sibling(exe, ".swap");
    files
        .rename(exe, &parked)
        .map_err(|e| format!("cannot move {} aside: {e}", exe.display()))?;
    if let Err(e) = files.rename(&old, exe) {
        return Err(match files.rename(&parked, exe) {
            Ok(()) => format!("cannot move the previous build in ({e}); nothing changed"),
            Err(b) => format!(
                "cannot move the previous build in ({e}) nor put the current one back ({b}); it is at {}",
                parked.display()
            ),
        });
    }
    files
        .rename(&parked, &old)
        .map_err(|e| format!("rolled back, but the newer build stays at {} ({e})", parked.display()))
}

#[cfg(test)]
mod tests;
