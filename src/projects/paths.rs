//! Path keys and checks (`ConvertTo-ProjectKey`, `ConvertTo-ProjectScratchKey`,
//! `Test-ProjectScratchPath`, `Get-ProjectTempRoots`, `Test-ProjectPathPresent`). Everything but the
//! presence check is string work: nothing here touches the disk or a network.

use std::collections::BTreeMap;
use std::path::Path;

use super::win::{self, DriveClass};

/// `ConvertTo-ProjectKey`: the one "same directory" comparison. Trailing separators, slash
/// direction and case do not matter.
pub fn project_key(path: &str) -> String {
    path.trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase()
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| s.get(prefix.len()..)).flatten()
}

fn is_drive_rooted(p: &str) -> bool {
    let mut c = p.chars();
    matches!((c.next(), c.next(), c.next()), (Some(_), Some(':'), Some('\\')))
}

/// `ConvertTo-ProjectScratchKey`: the key for the scratch PREFIX check, which has to see one
/// spelling: `\\?\` dropped, `..` and doubled separators resolved, an 8.3 name expanded. A path
/// that is not rooted is left alone: resolving it would read THIS process's cwd into it.
pub(crate) fn scratch_key(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let mut p = path.replace('/', "\\");
    if let Some(rest) = strip_prefix_ci(&p, r"\\?\UNC\") {
        p = format!(r"\\{rest}");
    } else if let Some(rest) = p.strip_prefix(r"\\?\").or_else(|| p.strip_prefix(r"\\.\")) {
        p = rest.to_owned();
    }
    let local = is_drive_rooted(&p);
    if local || (p.starts_with(r"\\") && !p.contains('~')) {
        if let Ok(full) = std::path::absolute(&p) {
            p = full.to_string_lossy().into_owned();
        }
        // The short-name expansion is a disk lookup: asked of a local path only.
        if local && p.contains('~') {
            p = expand_short(&p);
        }
    }
    project_key(&p)
}

/// Expands the longest existing prefix, keeping the rest as written.
fn expand_short(p: &str) -> String {
    let mut head = p.to_owned();
    let mut tail = String::new();
    loop {
        if let Some(long) = win::long_path(&head) {
            return format!("{long}{tail}");
        }
        let Some(cut) = head.rfind('\\') else { return p.to_owned() };
        let (h, t) = head.split_at(cut);
        if h.len() < 3 {
            return p.to_owned();
        }
        tail = format!("{t}{tail}");
        head = h.to_owned();
    }
}

/// `Test-ProjectRootKeyDeep`: a drive or share root is never a scratch root.
pub(crate) fn root_key_deep(key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    let parts = key.split('\\').filter(|s| !s.is_empty()).count();
    if key.starts_with(r"\\") { parts >= 3 } else { parts >= 2 }
}

/// `Get-ProjectTempRoots`: candidate temp directories as keys, minus any that could not be one (a
/// drive or share root, the home directory or anything above it).
pub(crate) fn temp_roots(candidates: &[String], home: &str) -> Vec<String> {
    let home_key = scratch_key(home);
    let mut keys: Vec<String> = Vec::new();
    for c in candidates {
        let k = scratch_key(c);
        if !root_key_deep(&k) {
            continue;
        }
        if !home_key.is_empty() && (home_key == k || home_key.starts_with(&format!("{k}\\"))) {
            continue;
        }
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys
}

/// The default candidates: `GetTempPath` and the profile's own `Temp`.
pub(crate) fn default_temp_candidates() -> Vec<String> {
    let mut c = vec![std::env::temp_dir().to_string_lossy().into_owned()];
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        c.push(Path::new(&local).join("Temp").to_string_lossy().into_owned());
    }
    c
}

/// `Test-ProjectScratchPath` over roots already normalised and depth-checked once per build.
pub(crate) fn is_scratch_key(key: &str, temp_keys: &[String], profile_keys: &[String]) -> bool {
    if key.is_empty() {
        return false;
    }
    if temp_keys.iter().any(|r| key == r || key.starts_with(&format!("{r}\\"))) {
        return true;
    }
    profile_keys.iter().filter(|r| !r.is_empty()).any(|r| {
        let Some(rest) = key.strip_prefix(&format!("{r}\\jobs\\")) else {
            return false;
        };
        let mut seg = rest.split('\\');
        matches!((seg.next(), seg.next()), (Some(id), Some("tmp")) if !id.is_empty())
    })
}

/// `Test-ProjectScratchPath` with raw roots: a temp root at a drive or share root hides nothing.
#[cfg(test)]
pub(crate) fn is_scratch_path(path: &str, temp_roots: &[&str], profile_roots: &[&str]) -> bool {
    let (t, p) = scratch_keys(temp_roots, profile_roots);
    is_scratch_key(&scratch_key(path), &t, &p)
}

/// The roots of one build as keys: temp roots depth-checked, empty profile roots dropped.
pub(crate) fn scratch_keys<S: AsRef<str>>(temp_roots: &[S], profile_roots: &[S]) -> (Vec<String>, Vec<String>) {
    let t = temp_roots
        .iter()
        .map(|r| scratch_key(r.as_ref()))
        .filter(|k| root_key_deep(k))
        .collect();
    let p = profile_roots
        .iter()
        .map(|r| scratch_key(r.as_ref()))
        .filter(|k| !k.is_empty())
        .collect();
    (t, p)
}

/// One build's drive answers, by lower-case letter.
pub(crate) type DriveCache = BTreeMap<char, DriveClass>;

/// `Test-ProjectPathPresent`: `Test-Path` that never waits on a network. A UNC path or a path on a
/// network drive is taken as present unprobed; `\\?\X:\` is local and probed; a drive letter that
/// is not there (or not ready) is absent without a look.
pub(crate) fn path_present(path: &str, drives: &mut DriveCache) -> bool {
    path_present_with(path, drives, win::drive_class)
}

pub(crate) fn path_present_with(path: &str, drives: &mut DriveCache, classify: fn(char) -> DriveClass) -> bool {
    if path.is_empty() || path.contains('\0') {
        return false;
    }
    let bytes = path.as_bytes();
    let sep = |b: u8| b == b'\\' || b == b'/';
    let mut p = path;
    if bytes.len() >= 4 && sep(bytes[0]) && sep(bytes[1]) && (bytes[2] == b'?' || bytes[2] == b'.') && sep(bytes[3]) {
        p = path.get(4..).unwrap_or_default();
        let pb = p.as_bytes();
        if pb.len() >= 4 && p.get(..3).is_some_and(|h| h.eq_ignore_ascii_case("UNC")) && sep(pb[3]) {
            return true;
        }
    } else if path.starts_with(r"\\") || path.starts_with("//") {
        return true;
    }
    let mut chars = p.chars();
    if let (Some(letter), Some(':')) = (chars.next(), chars.next())
        && letter.is_alphabetic()
    {
        let key = letter.to_ascii_lowercase();
        let class = *drives.entry(key).or_insert_with(|| classify(key));
        match class {
            DriveClass::Skip => return true,
            DriveClass::Absent => return false,
            DriveClass::Probe => {}
        }
    }
    Path::new(p).exists()
}

/// The drive class a presence check recorded for a path's letter (`skip` = network).
pub(crate) fn on_network(path: &str, drives: &DriveCache) -> bool {
    if path.starts_with(r"\\") || path.starts_with("//") {
        return true;
    }
    path.chars()
        .next()
        .is_some_and(|c| drives.get(&c.to_ascii_lowercase()) == Some(&DriveClass::Skip))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMP: &str = r"C:\Users\x\AppData\Local\Temp";
    const PROFILES: [&str; 2] = [r"C:\Users\x\.claude", r"C:\Users\x\.claude-personal"];

    fn scratch(p: &str) -> bool {
        is_scratch_path(p, &[TEMP], &PROFILES)
    }

    #[test]
    fn project_key_trims_lowercases_and_unifies_slashes() {
        assert_eq!(project_key(r"C:\W\Alpha\"), r"c:\w\alpha");
        assert_eq!(project_key(r"C:\w\alpha"), project_key("c:/w/alpha/"));
        assert_eq!(project_key(""), "");
    }

    #[test]
    fn scratch_rules() {
        assert!(scratch(r"C:\Users\x\AppData\Local\Temp\claude\slug\sess\rules\child"));
        assert!(is_scratch_path(
            "c:/users/X/appdata/local/TEMP/probe-319/",
            &[&format!("{TEMP}\\")],
            &PROFILES
        ));
        assert!(scratch(TEMP), "the temp root itself");
        assert!(!scratch(r"C:\Users\x\AppData\Local\Temporary\repo"), "a prefix sibling");
        assert!(scratch(r"C:\Users\x\.claude-personal\jobs\76c3aa80\tmp\w585"));
        assert!(scratch(r"C:\Users\x\.claude\jobs\76c3aa80\tmp"));
        assert!(!scratch(r"C:\Users\x\.claude\jobs\76c3aa80\work\repo"));
        assert!(!scratch(r"C:\Users\x\Projects\jobs\1\tmp\repo"));
        assert!(!scratch(r"C:\Users\x"), "home stays");
        assert!(
            !is_scratch_path(r"C:\Users\x\Projects\a", &["", ""], &[]),
            "an empty root hides nothing"
        );
    }

    #[test]
    fn degenerate_temp_roots_hide_nothing() {
        let home = r"C:\Users\x";
        assert_eq!(temp_roots(&[home.into()], home), [] as [&str; 0]);
        assert_eq!(temp_roots(&[r"C:\Users\".into()], home), [] as [&str; 0]);
        assert_eq!(temp_roots(&[r"B:\".into()], home), [] as [&str; 0]);
        assert_eq!(temp_roots(&[r"\\srv\share".into()], home), [] as [&str; 0]);
        assert_eq!(
            temp_roots(&[r"C:\Users\x\AppData\Local\Temp\".into(), home.into()], home),
            [r"c:\users\x\appdata\local\temp"]
        );
        assert!(!is_scratch_path(r"B:\wt\repo", &[r"B:\"], &[]));
    }

    #[test]
    fn scratch_check_normalises_first() {
        let t = [TEMP];
        assert!(is_scratch_path(r"C:\Users\x\Projects\..\AppData\Local\Temp\probe", &t, &[]));
        assert!(!is_scratch_path(r"C:\Users\x\AppData\Local\Temp\..\..\..\Projects\real", &t, &[]));
        assert!(is_scratch_path(r"C:\Users\x\AppData\\Local\Temp\\probe", &t, &[]));
        assert!(is_scratch_path(r"\\?\C:\Users\x\AppData\Local\Temp\probe", &t, &[]));
    }

    #[test]
    fn presence_never_probes_a_network() {
        let mut d = DriveCache::new();
        let never = |_: char| -> DriveClass { panic!("a UNC path must not consult the drive table") };
        assert!(path_present_with(r"\\claude-auto-no-such-host-604\share\repo", &mut d, never));
        assert!(path_present_with(r"\\?\UNC\claude-auto-no-such-host-604\share\repo", &mut d, never));
        let mut net = DriveCache::from([('q', DriveClass::Skip)]);
        assert!(
            path_present(r"Q:\src\repo", &mut net),
            "a drive classed network is not probed again"
        );
        let mut gone = DriveCache::from([('q', DriveClass::Absent)]);
        assert!(!path_present(r"Q:\src\repo", &mut gone));
        let here = env!("CARGO_MANIFEST_DIR");
        assert!(path_present(here, &mut DriveCache::new()));
        assert!(!path_present(&format!(r"{here}\no-such-dir-604"), &mut DriveCache::new()));
        assert!(path_present(&format!(r"\\?\{here}"), &mut DriveCache::new()), r"\\?\X:\ is local");
        assert!(!path_present(&format!(r"\\?\{here}\no-such-dir-604"), &mut DriveCache::new()));
        assert!(!path_present("C:\\a\0b", &mut DriveCache::new()), "a NUL is absent, quietly");
    }

    #[test]
    fn drive_table_classes_the_suite_drive_and_a_missing_letter() {
        let here = env!("CARGO_MANIFEST_DIR");
        let mut d = DriveCache::new();
        path_present(here, &mut d);
        let letter = here.chars().next().unwrap().to_ascii_lowercase();
        assert_eq!(d.get(&letter), Some(&DriveClass::Probe));
        let free = ('c'..='z').rev().find(|c| !Path::new(&format!("{c}:\\")).exists()).unwrap();
        assert!(!path_present(&format!(r"{free}:\src\repo"), &mut d));
        assert_eq!(d.get(&free), Some(&DriveClass::Absent));
    }

    #[test]
    fn short_name_spelling_is_one_key() {
        let long = std::env::temp_dir().join(format!("claude-auto long name fixture {}", std::process::id()));
        std::fs::create_dir_all(&long).unwrap();
        let long_s = long.to_string_lossy().into_owned();
        // On a volume with short names off both spellings are one and this holds trivially.
        let short = win::short_path(&long_s).unwrap_or_else(|| long_s.clone());
        assert!(
            is_scratch_path(&format!(r"{short}\child\deeper"), &[&long_s], &[]),
            "8.3 cwd under a long root ({short})"
        );
        assert!(
            is_scratch_path(&format!(r"{long_s}\child\deeper"), &[&short], &[]),
            "long cwd under an 8.3 root"
        );
        std::fs::remove_dir_all(&long).unwrap();
    }
}
