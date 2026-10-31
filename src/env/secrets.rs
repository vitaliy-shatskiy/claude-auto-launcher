//! `Import-ProjectSecrets`: one file per variable (file name = variable name, content = value),
//! tiers shared -> org -> project, later beating earlier. The org tier is a junction at
//! `<slug>\org`. Values go to the CHILD environment only and are never printed.

use std::os::windows::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use super::Note;
use crate::out::Fg;

/// Names the shell owns: a secret file called PATH is a mistake, never an instruction.
const RESERVED: [&str; 12] = [
    "PATH",
    "PATHEXT",
    "COMSPEC",
    "SYSTEMROOT",
    "WINDIR",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "OS",
    "PSMODULEPATH",
];
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

/// What one import decided: the variables to set (in order), the `NAME(tier)` list, the lines.
#[derive(Debug, Default)]
pub(crate) struct Import {
    pub(crate) vars: Vec<(String, String)>,
    pub(crate) loaded: Vec<String>,
    pub(crate) notes: Vec<Note>,
}

fn is_env_name(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// A secret read by PATH (`openrouter-api-key`, `teams-reader-app.json`): starts with a letter,
/// carries a dash or a dot. It costs no warning line.
fn is_path_secret(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic())
        && c.all(|x| x.is_ascii_alphanumeric() || matches!(x, '.' | '_' | '-'))
        && n.contains(['-', '.'])
}

/// `Get-Content -Raw` then `Trim`: a BOM is decoding, a trailing newline is not part of a token.
fn read_value(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let text = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let t = text.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// The visible files of a tier, in directory order (`Get-ChildItem -File` without `-Force`).
fn tier_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter(|e| {
            e.metadata()
                .is_ok_and(|m| m.is_file() && m.file_attributes() & FILE_ATTRIBUTE_HIDDEN == 0)
        })
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, _)| !Path::new(n).extension().is_some_and(|x| x.eq_ignore_ascii_case("md")))
        .collect()
}

/// The decision, separate from the child environment so the suite can read it.
pub(crate) fn import(working_directory: &str, root: &str) -> Import {
    let slug: String = working_directory
        .chars()
        .map(|c| if matches!(c, ':' | '\\' | '/' | '.') { '-' } else { c })
        .collect();
    let root = Path::new(root);
    let tiers = [
        ("shared", root.join("shared")),
        ("org", root.join(&slug).join("org")),
        ("project", root.join(&slug)),
    ];
    // (name, tier, path) in first-seen order; a later tier replaces the value in place.
    let mut winner: Vec<(String, &str, PathBuf, String)> = Vec::new();
    let mut by_path = Vec::new();
    let mut out = Import::default();
    for (tier, dir) in &tiers {
        for (name, path) in tier_files(dir) {
            if !is_env_name(&name) {
                if is_path_secret(&name) {
                    by_path.push(name);
                } else {
                    out.notes.push(Note::new(
                        format!("  secret '{name}' skipped: not a valid environment variable name"),
                        Fg::DarkYellow,
                    ));
                }
                continue;
            }
            if RESERVED.contains(&name.to_ascii_uppercase().as_str()) {
                out.notes.push(Note::new(
                    format!("  secret '{name}' skipped: that name is reserved by the system"),
                    Fg::DarkYellow,
                ));
                continue;
            }
            let Some(value) = read_value(&path) else { continue };
            match winner.iter_mut().find(|w| w.0.eq_ignore_ascii_case(&name)) {
                Some(w) => (w.1, w.2, w.3) = (*tier, path, value),
                None => winner.push((name, *tier, path, value)),
            }
        }
    }
    for (name, _, _, value) in &winner {
        out.vars.push((name.clone(), value.clone()));
    }
    // <NAME>_FILE = the path of the tier that won; a real secret file of that name outranks it.
    for (name, _, path, _) in &winner {
        let derived = format!("{name}_FILE");
        if !winner.iter().any(|w| w.0.eq_ignore_ascii_case(&derived)) {
            out.vars.push((derived, path.to_string_lossy().into_owned()));
        }
    }
    out.loaded = winner.iter().map(|(n, t, _, _)| format!("{n}({t})")).collect();
    if !out.loaded.is_empty() {
        out.notes
            .push(Note::new(format!("  secrets loaded: {}", out.loaded.join(", ")), Fg::DarkGray));
    }
    if !by_path.is_empty() {
        out.notes.push(Note::new(
            format!("  read by path, not exported: {}", by_path.join(", ")),
            Fg::DarkGray,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Store(PathBuf);
    impl Store {
        fn new(tag: &str) -> Self {
            Self(std::env::temp_dir().join(format!("claude-auto-rs-secrets-{tag}-{}", crate::log::new_run_id())))
        }
        fn put(&self, rel: &str, value: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, value).unwrap();
        }
        fn root(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }
    impl Drop for Store {
        fn drop(&mut self) {
            if std::fs::remove_dir_all(&self.0).is_err() {
                // A leftover temp directory is litter, not a test failure.
            }
        }
    }

    fn var<'a>(i: &'a Import, n: &str) -> Option<&'a str> {
        i.vars.iter().rev().find(|(k, _)| k == n).map(|(_, v)| v.as_str())
    }

    fn file_of(i: &Import, n: &str) -> String {
        std::fs::read_to_string(var(i, n).unwrap()).unwrap()
    }

    fn tiered_store() -> Store {
        let s = Store::new("tiers");
        let slug = "C--test--proj";
        s.put(r"shared\SHARED_ONLY", "from-shared");
        s.put(r"shared\OVERRIDDEN", "from-shared");
        s.put(r"shared\ORG_BEATS_THIS", "from-shared");
        s.put(r"org\acme\ORG_ONLY", "from-org");
        s.put(r"org\acme\OVERRIDDEN", "from-org");
        s.put(r"org\acme\ORG_BEATS_THIS", "from-org");
        s.put(&format!(r"{slug}\PROJ_ONLY"), "from-proj\r\n");
        s.put(&format!(r"{slug}\OVERRIDDEN"), "from-proj");
        s.put(&format!(r"{slug}\EMPTY"), "");
        s.put(&format!(r"{slug}\PAIR"), "pair-value");
        s.put(&format!(r"{slug}\PAIR_FILE"), "a-real-secret-not-a-path");
        s.put(&format!(r"{slug}\NOTES.md"), "not a secret");
        super::super::win_fs::create_junction(&s.0.join(slug).join("org"), &s.0.join(r"org\acme")).unwrap();
        s
    }

    #[test]
    fn tiers_cascade_shared_org_project() {
        let s = tiered_store();
        let i = import(r"C:\test\.proj", &s.root());
        assert_eq!(var(&i, "SHARED_ONLY"), Some("from-shared"), "shared tier is loaded");
        assert_eq!(var(&i, "ORG_ONLY"), Some("from-org"), "org tier is loaded through the junction");
        assert_eq!(
            var(&i, "PROJ_ONLY"),
            Some("from-proj"),
            "project tier is loaded, trailing newline trimmed"
        );
        assert_eq!(var(&i, "OVERRIDDEN"), Some("from-proj"), "project beats org and shared");
        assert_eq!(var(&i, "ORG_BEATS_THIS"), Some("from-org"), "org beats shared");
        assert_eq!(var(&i, "NOTES.md"), None, "markdown is not loaded as a variable");
        assert_eq!(var(&i, "EMPTY"), None, "an empty file sets nothing");
        let joined = i.loaded.join(",");
        assert!(
            joined.contains("shared") && joined.contains("org") && joined.contains("project"),
            "the return value names each tier"
        );
        std::fs::remove_dir(s.0.join(r"C--test--proj\org")).unwrap();
    }

    #[test]
    fn every_loaded_secret_gets_a_file_companion_from_the_winning_tier() {
        let s = tiered_store();
        s.put(r"shared\TIERED_TOKEN", "shared-token");
        s.put(r"C--test--proj\TIERED_TOKEN", "proj-token");
        let i = import(r"C:\test\.proj", &s.root());
        assert_eq!(file_of(&i, "SHARED_ONLY_FILE"), "from-shared");
        assert_eq!(file_of(&i, "OVERRIDDEN_FILE"), "from-proj", "_FILE points at the tier that won");
        assert_eq!(file_of(&i, "ORG_BEATS_THIS_FILE"), "from-org");
        assert_eq!(var(&i, "EMPTY_FILE"), None, "a skipped empty file gets no _FILE");
        assert_eq!(var(&i, "NOTES.md_FILE"), None);
        let joined = i.loaded.join(",");
        assert!(!joined.contains("SHARED_ONLY_FILE"), "a derived name stays out of the loaded list");
        assert!(joined.contains("PAIR_FILE(project)"), "while a real secret named _FILE is in it");
        assert_eq!(
            var(&i, "PAIR_FILE"),
            Some("a-real-secret-not-a-path"),
            "a real _FILE secret is not overwritten"
        );
        assert_eq!(file_of(&i, "PAIR_FILE_FILE"), "a-real-secret-not-a-path");
        assert_eq!(var(&i, "SONAR_TOKEN_FILE"), None);
        assert_eq!(file_of(&i, "TIERED_TOKEN_FILE"), "proj-token");
        assert_eq!(var(&i, "TIERED_TOKEN"), Some("proj-token"));
        std::fs::remove_dir(s.0.join(r"C--test--proj\org")).unwrap();
    }

    #[test]
    fn an_unknown_project_gets_shared_only_and_a_missing_root_nothing() {
        let s = tiered_store();
        let i = import(r"C:\test\unknown", &s.root());
        assert_eq!(var(&i, "SHARED_ONLY"), Some("from-shared"));
        assert_eq!(var(&i, "PROJ_ONLY"), None);
        assert!(!i.loaded.is_empty() && !i.loaded.join(",").contains("project"));
        let none = import(r"C:\test\.proj", &s.0.join("does-not-exist").to_string_lossy());
        assert!(none.loaded.is_empty() && none.vars.is_empty() && none.notes.is_empty());
        std::fs::remove_dir(s.0.join(r"C--test--proj\org")).unwrap();
    }

    #[test]
    fn a_file_name_that_is_no_variable_name_costs_one_line() {
        let s = Store::new("names");
        for n in ["PATH", "A=B", "1DIGIT", "HAS SPACE"] {
            s.put(&format!(r"shared\{n}"), "x");
        }
        s.put(r"shared\GOOD_NAME", "good");
        s.put(r"shared\by-path-key.json", "k");
        let i = import(r"C:\test\.names", &s.root());
        let text: Vec<&str> = i.notes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(var(&i, "PATH"), None, "a secret file named PATH does not replace PATH");
        assert_eq!(
            text.iter().filter(|t| t.trim_start().starts_with("secret '")).count(),
            4,
            "each rejected name costs exactly one line"
        );
        assert_eq!(text.iter().filter(|t| t.contains("secret 'A=B'")).count(), 1);
        assert!(text.iter().any(|t| t.contains("secret 'PATH'") && t.contains("reserved")));
        assert_eq!(var(&i, "GOOD_NAME"), Some("good"));
        assert!(
            !text.iter().any(|t| t.contains("secret 'by-path-key.json'")),
            "a path-read secret costs no warning line"
        );
        assert!(text.iter().any(|t| t.contains("read by path") && t.contains("by-path-key.json")));
        assert_eq!(var(&i, "by-path-key.json"), None);
        assert_eq!(i.loaded, vec!["GOOD_NAME(shared)".to_owned()]);
        assert!(!text.iter().any(|t| t.contains("good")), "a value is never printed");
    }

    #[test]
    fn a_store_under_a_bracket_path_loads() {
        let s = Store::new("br");
        let root = s.0.join("br[x]dir");
        std::fs::create_dir_all(root.join("shared")).unwrap();
        std::fs::create_dir_all(root.join("C--test--brk")).unwrap();
        std::fs::write(root.join(r"shared\BRACKET_SHARED"), "br-shared").unwrap();
        std::fs::write(root.join(r"C--test--brk\BRACKET_PROJ"), "br-proj").unwrap();
        let i = import(r"C:\test\.brk", &root.to_string_lossy());
        assert_eq!(var(&i, "BRACKET_SHARED"), Some("br-shared"));
        assert_eq!(var(&i, "BRACKET_PROJ"), Some("br-proj"));
        assert_eq!(file_of(&i, "BRACKET_PROJ_FILE"), "br-proj");
    }
}
