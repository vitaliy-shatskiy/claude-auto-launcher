//! Sharing repair between the canonical profile root ("work") and every secondary root:
//! hardlinked files (`Repair-SharedLink`), the `CLAUDE.md` file symlink (`Repair-SharedSymlink`),
//! junctions with their ACL shield (`Repair-SharedJunction`, `Protect-SharedJunction`) and the
//! profile root itself (`New-ClaudeProfileRoot`). Every step yields notes; nothing panics.

use std::os::windows::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use regex::Regex;
use serde_json::Value;

use super::{Note, sorted_json, win_fs};
use crate::out::Fg;

/// What the sharing layer keeps in step: hardlinked files, file symlinks, junctioned directories.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SharedSet<'a> {
    pub(crate) files: &'a [&'a str],
    pub(crate) symlinks: &'a [&'a str],
    pub(crate) dirs: &'a [&'a str],
}

pub(crate) const SHARED: SharedSet<'static> = SharedSet {
    files: &["settings.json", "statusline.js"],
    // A symlink loads once everywhere; a hardlink under the home tree loads twice.
    symlinks: &["CLAUDE.md"],
    dirs: &[
        "projects",
        "plugins",
        "hooks",
        "agents",
        "skills",
        "output-styles",
        "rules",
        "sessions",
        "file-history",
        "session-env",
        "tasks",
        "shell-snapshots",
    ],
};

/// Keys an account may hold on its own: a copy differing ONLY in these stays a separate file.
const VOLATILE: &[(&str, &[&str])] = &[("settings.json", &["model", "effortLevel", "advisorModel"])];

/// icacls with its arguments: (exit code, stdout).
pub(crate) type Icacls<'a> = dyn FnMut(&[String]) -> std::io::Result<(i32, String)> + 'a;

/// The side effects the suite replaces: creating a file symlink, running icacls.
pub(crate) struct Seams<'a> {
    pub(crate) symlink: &'a dyn Fn(&Path, &Path) -> std::io::Result<()>,
    pub(crate) icacls: &'a mut Icacls<'a>,
}

impl std::fmt::Debug for Seams<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Seams")
    }
}

pub(crate) fn real_symlink(link: &Path, target: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

pub(crate) fn real_icacls(args: &[String]) -> std::io::Result<(i32, String)> {
    use std::os::windows::process::CommandExt as _;
    let out = std::process::Command::new("icacls.exe")
        .args(args)
        .creation_flags(super::CREATE_NO_WINDOW)
        .output()?;
    Ok((out.status.code().unwrap_or(1), String::from_utf8_lossy(&out.stdout).into_owned()))
}

fn leaf(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn parent_leaf(p: &Path) -> String {
    p.parent().map(leaf).unwrap_or_default()
}

fn one_line(e: &impl std::fmt::Display) -> String {
    e.to_string().replace("\r\n", " ").replace('\n', " ")
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// `yyyyMMdd-HHmmss` of a UTC time.
pub(crate) fn utc_stamp(time: SystemTime) -> String {
    let secs = time.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let doe = shifted - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn local_stamp() -> String {
    crate::sys::local_now().map_or_else(
        || utc_stamp(SystemTime::now()),
        |n| {
            format!(
                "{:04}{:02}{:02}-{:02}{:02}{:02}",
                n.year, n.month, n.day, n.hour, n.minute, n.second
            )
        },
    )
}

fn read_json(p: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// `Test-VolatileOnlyDifference`: the two copies differ in at least one volatile key and in
/// nothing else. Unparseable content, or any other difference, is not volatile-only.
pub(crate) fn volatile_only_difference(name: &str, a: &Path, b: &Path) -> bool {
    let Some((_, keys)) = VOLATILE.iter().find(|(n, _)| *n == name) else {
        return false;
    };
    if !Path::new(name).extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
        return false;
    }
    let (Some(Value::Object(mut a)), Some(Value::Object(mut b))) = (read_json(a), read_json(b)) else {
        return false;
    };
    let mut differs = false;
    for k in *keys {
        let (va, vb) = (a.remove(*k).unwrap_or(Value::Null), b.remove(*k).unwrap_or(Value::Null));
        differs |= sorted_json(&va) != sorted_json(&vb);
    }
    differs && sorted_json(&Value::Object(a)) == sorted_json(&Value::Object(b))
}

/// Moves an existing `.pre-relink` aside under its own write time and keeps the newest three.
fn rotate_backup(backup: &Path, dst: &Path) -> std::io::Result<()> {
    let stamp = utc_stamp(mtime(backup).ok_or_else(|| std::io::Error::other("the backup has no write time"))?);
    let mut rotated = with_suffix(backup, &format!(".{stamp}"));
    let mut n = 1;
    while rotated.exists() {
        rotated = with_suffix(backup, &format!(".{stamp}-{n}"));
        n += 1;
    }
    std::fs::rename(backup, &rotated)?;
    let prefix = format!("{}.", leaf(backup)).to_lowercase();
    let dir = dst.parent().ok_or_else(|| std::io::Error::other("no parent directory"))?;
    let mut olds: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && e.file_name().to_string_lossy().to_lowercase().starts_with(&prefix))
        .filter_map(|e| Some((mtime(&e.path())?, e.path())))
        .collect();
    olds.sort_by_key(|o| std::cmp::Reverse(o.0));
    for (_, p) in olds.iter().skip(3) {
        if std::fs::remove_file(p).is_err() {
            // Best effort, like Remove-Item -ErrorAction SilentlyContinue: one more backup is no harm.
        }
    }
    Ok(())
}

/// One divergent copy: backup (rotated), delete, hardlink onto `src`; the backup is put back when
/// the link does not land. `Ok(true)` = re-linked.
fn relink_one(name: &str, src: &Path, dst: &Path, notes: &mut Vec<Note>) -> bool {
    let backup = with_suffix(dst, ".pre-relink");
    if backup.exists()
        && let Err(e) = rotate_backup(&backup, dst)
    {
        notes.push(Note::new(
            format!(
                "  could not rotate the previous backup of {} ({e}); it is overwritten",
                dst.display()
            ),
            Fg::DarkYellow,
        ));
    }
    if let Err(e) = std::fs::copy(dst, &backup) {
        notes.push(Note::new(
            format!("  could not back up {} before re-linking ({e}); left alone", dst.display()),
            Fg::DarkYellow,
        ));
        return false;
    }
    if let Err(e) = std::fs::remove_file(dst).and_then(|()| std::fs::hard_link(src, dst)) {
        if !dst.exists() && std::fs::copy(&backup, dst).is_err() {
            // The .pre-relink beside it is the copy left to restore from by hand.
        }
        notes.push(Note::new(
            format!("  could not re-link {name} in {}: {e}", parent_leaf(dst)),
            Fg::DarkYellow,
        ));
        return false;
    }
    true
}

/// `Repair-SharedLink`: one File ID across every root that holds `name`, re-linked from the
/// NEWEST copy when they differ. A copy differing only in volatile keys stays separate.
pub(crate) fn repair_link(name: &str, roots: &[PathBuf]) -> Vec<Note> {
    let mut notes = Vec::new();
    let paths: Vec<PathBuf> = roots.iter().map(|r| r.join(name)).filter(|p| p.exists()).collect();
    if paths.len() < 2 {
        return notes;
    }
    let mut ids = Vec::with_capacity(paths.len());
    for p in &paths {
        let Some(id) = win_fs::file_id(p) else {
            notes.push(Note::new(
                format!("  could not read the File ID of {} - link state unknown for {name}", p.display()),
                Fg::DarkYellow,
            ));
            return notes;
        };
        ids.push(id);
    }
    if ids.iter().all(|i| *i == ids[0]) {
        return notes;
    }
    let mut src = 0;
    for (i, p) in paths.iter().enumerate() {
        if mtime(p) > mtime(&paths[src]) {
            src = i;
        }
    }
    let mut relinked = Vec::new();
    for (dst, id) in paths.iter().zip(&ids) {
        if *id == ids[src] || volatile_only_difference(name, &paths[src], dst) {
            continue;
        }
        if relink_one(name, &paths[src], dst, &mut notes) {
            relinked.push(parent_leaf(dst));
        }
    }
    if !relinked.is_empty() {
        notes.push(Note::new(
            format!(
                "  re-linked {name} in {} (kept the newest copy, discarded ones saved as .pre-relink)",
                relinked.join(", ")
            ),
            Fg::Yellow,
        ));
    }
    notes
}

fn full(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let s = abs.to_string_lossy().into_owned();
    s.strip_prefix(r"\\?\").map_or(s.clone(), str::to_owned).to_lowercase()
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// The body of `Repair-SharedSymlink`: at most one note; `None` = already correct.
fn symlink_note(name: &str, work: &Path, root: &Path, make_link: &dyn Fn(&Path, &Path) -> std::io::Result<()>) -> Option<Note> {
    let w = work.join(name);
    let p = root.join(name);
    let root_name = leaf(root);
    let warn = |t: String| Some(Note::new(t, Fg::DarkYellow));
    let mut backup = None;
    if let Ok(md) = std::fs::symlink_metadata(&p) {
        let ft = md.file_type();
        if md.is_dir() || ft.is_symlink_dir() {
            return warn(format!(
                "{name} is a directory in {root_name}, not a symlink to work - fix it by hand"
            ));
        }
        if ft.is_symlink_file() {
            let t = std::fs::read_link(&p).unwrap_or_default();
            let t = if t.is_absolute() { t } else { root.join(t) };
            if full(&t) == full(&w) {
                return None;
            }
        } else {
            // Unreadable either side = not proven equal = diverged: never replaced.
            if !same_bytes(&p, &w) {
                return warn(format!(
                    "{name} in {root_name} differs from the work copy - left in place, merge it by hand"
                ));
            }
            backup = Some(with_suffix(&p, &format!(".pre-symlink.{}", local_stamp())));
        }
    }
    let tmp = with_suffix(&p, &format!(".symlink-{}", crate::log::new_run_id()));
    let drop_tmp = || {
        if std::fs::symlink_metadata(&tmp).is_ok() && std::fs::remove_file(&tmp).is_err() {
            // A leftover temp link is litter, never a reason to stop the launch.
        }
    };
    let made = make_link(&tmp, &w).and_then(|()| {
        if tmp.exists() {
            Ok(())
        } else {
            Err(std::io::Error::other("the link was not created"))
        }
    });
    if let Err(e) = made {
        drop_tmp();
        return warn(format!(
            "could not symlink {name} in {root_name} to work ({}) - run once elevated or enable Developer Mode",
            one_line(&e)
        ));
    }
    // The backup only once the link exists: without the privilege this runs every launch.
    if let Some(b) = &backup
        && let Err(e) = std::fs::copy(&p, b)
    {
        drop_tmp();
        return warn(format!(
            "could not back up {name} in {root_name} before replacing it ({e}); left alone"
        ));
    }
    if let Err(e) = std::fs::rename(&tmp, &p) {
        drop_tmp();
        if let Some(b) = &backup
            && std::fs::remove_file(b).is_err()
        {
            // The byte-identical backup may stay; the next launch tries again.
        }
        return warn(format!(
            "could not replace {name} in {root_name} with the symlink ({e}); left alone"
        ));
    }
    let saved = backup.map(|b| format!(" (the copy saved as {})", leaf(&b))).unwrap_or_default();
    Some(Note::new(format!("symlinked {name} in {root_name} -> work{saved}"), Fg::Yellow))
}

/// `Repair-SharedSymlink`: `<root>\<name>` a file symlink to the canonical copy. Missing ->
/// created; elsewhere -> re-pointed; an identical regular file -> saved and replaced; a diverged
/// copy is the owner's edit, left and named. Lines go through `Write-SharingNote` (indented).
pub(crate) fn repair_symlink(name: &str, work: &Path, root: &Path, make_link: &dyn Fn(&Path, &Path) -> std::io::Result<()>) -> Vec<Note> {
    if !root.is_dir() || !work.join(name).is_file() {
        return Vec::new();
    }
    symlink_note(name, work, root, make_link)
        .map(|n| Note::new(format!("  {}", n.text), n.fg))
        .into_iter()
        .collect()
}

/// `Protect-SharedJunction`: deny DELETE on the junction (`/L`, never the target) and
/// `FILE_DELETE_CHILD` on the root that owns it, so Claude Code's swallowed `rmdir` fails instead
/// of deleting the link. The root deny is added only when absent. icacls exit codes are read.
pub(crate) fn protect_junction(path: &Path, root: &Path, icacls: &mut Icacls<'_>) -> Vec<Note> {
    let who = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    let mut notes = Vec::new();
    let (p, r) = (path.display().to_string(), root.display().to_string());
    let mut calls: Vec<(&str, Vec<String>)> = Vec::new();
    let existing = icacls(std::slice::from_ref(&r)).map(|(_, o)| o).unwrap_or_default();
    let has_root_deny = Regex::new(&format!(r"(?i){}\s*:\s*\(DENY\)\(DC\)", regex::escape(&who))).is_ok_and(|re| re.is_match(&existing));
    if !has_root_deny {
        calls.push(("profile root", vec![r, "/deny".into(), format!("{who}:(DC)")]));
    }
    calls.push(("junction", vec![p.clone(), "/L".into(), "/deny".into(), format!("{who}:(DE)")]));
    for (what, args) in calls {
        match icacls(&args) {
            Err(e) => notes.push(Note::new(
                format!("  could not run icacls for the {what} shield on {p}: {e}"),
                Fg::DarkYellow,
            )),
            Ok((0, _)) => {}
            Ok((code, _)) => notes.push(Note::new(
                format!("  {what} shield NOT applied for {p} (icacls exit {code}) - the retention sweep can delete this junction"),
                Fg::DarkYellow,
            )),
        }
    }
    notes
}

/// `Repair-SharedJunction`: a missing junction is created (and shielded); a real directory in its
/// place is reported, never merged. A missing canonical directory is put back silently first.
pub(crate) fn repair_junction(name: &str, work: &Path, root: &Path, icacls: &mut Icacls<'_>) -> std::io::Result<Vec<Note>> {
    let w = work.join(name);
    let p = root.join(name);
    if !root.exists() {
        return Ok(Vec::new());
    }
    if !w.exists() {
        if !work.is_dir() {
            return Ok(Vec::new());
        }
        std::fs::create_dir(&w)?;
    }
    let root_name = leaf(root);
    if let Ok(md) = std::fs::symlink_metadata(&p) {
        if md.file_attributes() & super::FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            return Ok(vec![Note::new(
                format!("  {name} is a real directory in {root_name}, not a junction - merge it by hand"),
                Fg::DarkYellow,
            )]);
        }
        return Ok(Vec::new());
    }
    win_fs::create_junction(&p, &w)?;
    let mut notes = protect_junction(&p, root, icacls);
    notes.push(Note::new(
        format!("  junction created and shielded: {root_name} {name} -> work"),
        Fg::Yellow,
    ));
    Ok(notes)
}

/// `New-ClaudeProfileRoot`: the root exists and holds a hardlink of each shared file the work root
/// has. Idempotent; `Ok(true)` = the root was created now.
pub(crate) fn new_profile_root(root: &Path, work: &Path, files: &[&str], notes: &mut Vec<Note>) -> std::io::Result<bool> {
    let created = !root.exists();
    if created {
        std::fs::create_dir_all(root)?;
    }
    for name in files {
        let (w, t) = (work.join(name), root.join(name));
        if !w.exists() || t.exists() {
            continue;
        }
        std::fs::hard_link(&w, &t)?;
        notes.push(Note::new(format!("  linked {name} into {}", leaf(root)), Fg::Yellow));
    }
    if created {
        notes.push(Note::new(
            format!(
                "  created profile root {} - log in there on the first session with that account",
                root.display()
            ),
            Fg::Yellow,
        ));
    }
    Ok(created)
}

/// `Repair-SharedProfiles` without the config and preview guards: roots, links, junctions,
/// symlinks, each step in its own fail-soft envelope.
pub(crate) fn repair_all(work: &Path, secondary: &[PathBuf], set: SharedSet<'_>, seams: &mut Seams<'_>) -> Vec<Note> {
    let mut notes = Vec::new();
    let note = |t: String| Note::new(format!("  {t}"), Fg::DarkYellow);
    for r in secondary {
        if let Err(e) = new_profile_root(r, work, set.files, &mut notes) {
            notes.push(note(format!("profile root check failed for {}: {e}", r.display())));
        }
    }
    let roots: Vec<PathBuf> = std::iter::once(work.to_path_buf()).chain(secondary.iter().cloned()).collect();
    for f in set.files {
        notes.extend(repair_link(f, &roots));
    }
    for r in secondary {
        for d in set.dirs {
            match repair_junction(d, work, r, seams.icacls) {
                Ok(n) => notes.extend(n),
                Err(e) => notes.push(note(format!("junction check failed for {d}: {e}"))),
            }
        }
        for f in set.symlinks {
            notes.extend(repair_symlink(f, work, r, seams.symlink));
        }
    }
    notes
}

#[cfg(test)]
mod tests;
