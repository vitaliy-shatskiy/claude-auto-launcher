//! Privacy scan: no owner identifier may be in the tree. Patterns = the current Windows user name
//! plus an optional private list (one regex per line, `#` comments; `--patterns <file>` or env
//! `CLAUDE_AUTO_CLEAN_PATTERNS`). The private list never enters the repository - it IS the list of
//! things that must not. Each file's path and its text (UTF-8, or UTF-16 with a BOM) are matched,
//! case-insensitively. A hit names the pattern by its line number,
//! never its text, so the list does not leak into a log.
//!
//! A dev tool, not part of the release exe: `cargo run --example privacy_scan -- [--root <dir>]
//! [--patterns <file>]`. `cargo test` runs its unit tests on throwaway git repositories.
//! Exit: 0 clean, 1 a hit, 2 could not run (never a pass).
//!
//! SCOPE: what git would ship (tracked plus untracked-not-ignored files). It never reads commit
//! history or authorship: check those separately before publishing (`git log --all --format='%an <%ae>'`).

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use regex::{Regex, RegexBuilder};

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Clean { files: usize, patterns: usize },
    Hit(Vec<String>),
    CouldNotRun(String),
}

#[derive(Debug)]
struct Pattern {
    label: String,
    re: Regex,
}

fn compile(source: &str) -> Option<Regex> {
    RegexBuilder::new(source).case_insensitive(true).build().ok()
}

/// The user name (word-bounded, so a prefix of another word does not flag) plus the private list.
/// An empty user name is skipped: `\b\b` would match every line. Nothing to look for is an error.
fn load_patterns(user: Option<&str>, list: Option<&Path>) -> Result<Vec<Pattern>, String> {
    let mut out = Vec::new();
    if let Some(user) = user.filter(|u| !u.trim().is_empty()) {
        let re = compile(&format!(r"\b{}\b", regex::escape(user))).ok_or("the user name does not compile")?;
        out.push(Pattern {
            label: "the user name".into(),
            re,
        });
    }
    if let Some(path) = list {
        let text = std::fs::read_to_string(path).map_err(|e| format!("the pattern file cannot be read: {e}"))?;
        // A BOM left on the first pattern would make it match nothing: a silent pass.
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            // The regex error text quotes the pattern: report the line number only.
            let re = compile(line).ok_or_else(|| format!("pattern line {} is not a valid regex", i + 1))?;
            out.push(Pattern {
                label: format!("pattern line {}", i + 1),
                re,
            });
        }
    }
    if out.is_empty() {
        return Err("no patterns: the user name is empty and no pattern file was given".into());
    }
    Ok(out)
}

/// Paths git would ship, NUL-separated with quoting off so a non-ASCII or odd name is not dropped.
fn list_files(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let out = Command::new("git")
        .args(["-c", "core.quotePath=false", "-C"])
        .arg(root)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .output()
        .map_err(|e| format!("git did not start: {e}"))?;
    if !out.status.success() {
        return Err(format!("git ls-files failed ({}): is the root a git repository?", out.status));
    }
    let text = String::from_utf8(out.stdout).map_err(|e| format!("git ls-files printed non-UTF-8: {e}"))?;
    Ok(text
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(|s| (s.to_owned(), root.join(s)))
        .collect())
}

fn skipped(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dll") || e.eq_ignore_ascii_case("pdb"))
}

/// UTF-16 with a BOM is decoded as such: read as UTF-8 its NUL bytes split every word, a silent pass.
fn decode(bytes: &[u8]) -> String {
    fn utf16(rest: &[u8], unit: fn([u8; 2]) -> u16) -> String {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| unit(*c)).collect();
        String::from_utf16_lossy(&units)
    }
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn scan(root: &Path, patterns: &[Pattern]) -> Verdict {
    let files = match list_files(root) {
        Ok(f) => f,
        Err(e) => return Verdict::CouldNotRun(e),
    };
    if files.is_empty() {
        return Verdict::CouldNotRun("git listed no files".into());
    }
    let mut hits = Vec::new();
    let mut scanned = 0;
    for (rel, path) in &files {
        if let Some(p) = patterns.iter().find(|p| p.re.is_match(rel)) {
            hits.push(format!("{rel} (path)  {}", p.label));
        }
        if skipped(path) {
            continue;
        }
        // A listed path that is not a readable file (deleted but staged, a submodule, a broken
        // link) fails closed: dropping it is how a real file goes unscanned.
        let Ok(bytes) = std::fs::read(path) else {
            return Verdict::CouldNotRun(format!("a listed path cannot be read: {rel}"));
        };
        scanned += 1;
        for (n, line) in decode(&bytes).lines().enumerate() {
            if let Some(p) = patterns.iter().find(|p| p.re.is_match(line)) {
                hits.push(format!("{rel}:{}  {}", n + 1, p.label));
            }
        }
    }
    if hits.is_empty() {
        Verdict::Clean {
            files: scanned,
            patterns: patterns.len(),
        }
    } else {
        Verdict::Hit(hits)
    }
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<(PathBuf, Option<PathBuf>), String> {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut list = std::env::var_os("CLAUDE_AUTO_CLEAN_PATTERNS")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    while let Some(a) = args.next() {
        let value = args.next().ok_or_else(|| format!("{a} needs a value"))?;
        match a.as_str() {
            "--root" => root = PathBuf::from(value),
            "--patterns" => list = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown argument {a} (known: --root, --patterns)")),
        }
    }
    Ok((root, list))
}

fn run() -> Verdict {
    let (root, list) = match parse_args(std::env::args().skip(1)) {
        Ok(v) => v,
        Err(e) => return Verdict::CouldNotRun(e),
    };
    let user = std::env::var("USERNAME").ok();
    match load_patterns(user.as_deref(), list.as_deref()) {
        Ok(patterns) => scan(&root, &patterns),
        Err(e) => Verdict::CouldNotRun(e),
    }
}

#[expect(clippy::print_stdout, reason = "the scan's CLI output")]
fn main() -> ExitCode {
    let (text, code) = match run() {
        Verdict::Clean { files, patterns } => (format!("privacy scan: clean ({files} files, {patterns} pattern(s))"), 0),
        Verdict::Hit(hits) => {
            for h in &hits {
                println!("{h}");
            }
            (format!("privacy scan: {} hit(s)", hits.len()), 1)
        }
        Verdict::CouldNotRun(e) => (format!("privacy scan could not run: {e}"), 2),
    };
    println!("{text}");
    ExitCode::from(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway git repository holding `files`, plus a pattern file outside it.
    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(name: &str, files: &[(&str, &str)], patterns: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("privacy-scan-{name}-{}", std::process::id()));
            if dir.exists() {
                std::fs::remove_dir_all(&dir).unwrap();
            }
            let repo = dir.join("repo");
            std::fs::create_dir_all(&repo).unwrap();
            let ok = Command::new("git").arg("init").arg("-q").arg(&repo).status().unwrap();
            assert!(ok.success(), "git init");
            for (rel, body) in files {
                std::fs::write(repo.join(rel), body).unwrap();
            }
            std::fs::write(dir.join("patterns.txt"), patterns).unwrap();
            Self { dir }
        }
        fn repo(&self) -> PathBuf {
            self.dir.join("repo")
        }
        fn list(&self) -> PathBuf {
            self.dir.join("patterns.txt")
        }
        fn scan(&self, user: Option<&str>) -> Verdict {
            match load_patterns(user, Some(&self.list())) {
                Ok(p) => scan(&self.repo(), &p),
                Err(e) => Verdict::CouldNotRun(e),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Err(e) = std::fs::remove_dir_all(&self.dir) {
                eprintln!("leftover fixture {}: {e}", self.dir.display());
            }
        }
    }

    #[test]
    fn planted_hit_is_found_case_insensitive() {
        let f = Fixture::new("hit", &[("a.txt", "one\nPlantedSecret here\n")], "# c\nplantedsecret\n");
        assert_eq!(f.scan(None), Verdict::Hit(vec!["a.txt:2  pattern line 2".into()]));
    }

    #[test]
    fn hit_in_non_ascii_name_is_found() {
        let f = Fixture::new("utf", &[("\u{444}.txt", "plantedsecret")], "plantedsecret");
        assert!(matches!(f.scan(None), Verdict::Hit(_)));
    }

    #[test]
    fn utf16_files_are_decoded() {
        let f = Fixture::new("u16", &[("a.txt", "x")], "plantedsecret");
        for (bom, be) in [([0xFF, 0xFE], false), ([0xFE, 0xFF], true)] {
            let mut bytes = bom.to_vec();
            for u in "one\r\nplantedsecret\r\n".encode_utf16() {
                bytes.extend(if be { u.to_be_bytes() } else { u.to_le_bytes() });
            }
            std::fs::write(f.repo().join("a.txt"), bytes).unwrap();
            assert_eq!(f.scan(None), Verdict::Hit(vec!["a.txt:2  pattern line 1".into()]), "be={be}");
        }
    }

    #[test]
    fn hit_in_file_path_is_found() {
        let f = Fixture::new("path", &[("plantedsecret-notes.txt", "nothing")], "plantedsecret");
        assert_eq!(
            f.scan(None),
            Verdict::Hit(vec!["plantedsecret-notes.txt (path)  pattern line 1".into()])
        );
    }

    #[test]
    fn user_name_is_word_bounded() {
        let f = Fixture::new("user", &[("a.txt", "plantuserx\nby plantuser.\n")], "");
        assert_eq!(f.scan(Some("plantuser")), Verdict::Hit(vec!["a.txt:2  the user name".into()]));
    }

    #[test]
    fn clean_tree_passes() {
        let f = Fixture::new("clean", &[("a.txt", "nothing\n")], "plantedsecret\n");
        assert_eq!(f.scan(Some("plantuser")), Verdict::Clean { files: 1, patterns: 2 });
    }

    #[test]
    fn bom_on_pattern_file_still_matches() {
        let f = Fixture::new("bom", &[("a.txt", "plantedsecret")], "\u{feff}plantedsecret\n");
        assert!(matches!(f.scan(None), Verdict::Hit(_)));
    }

    #[test]
    fn unreadable_pattern_file_could_not_run() {
        let f = Fixture::new("nolist", &[("a.txt", "x")], "");
        let missing = f.dir.join("missing.txt");
        assert!(load_patterns(Some("plantuser"), Some(&missing)).is_err());
    }

    #[test]
    fn bad_regex_could_not_run_without_quoting_it() {
        let f = Fixture::new("badre", &[("a.txt", "x")], "ok\nsecret(\n");
        let Verdict::CouldNotRun(msg) = f.scan(None) else {
            panic!("expected could-not-run")
        };
        assert!(!msg.contains("secret"), "{msg}");
    }

    #[test]
    fn no_patterns_could_not_run() {
        let f = Fixture::new("empty", &[("a.txt", "x")], "# only a comment\n\n");
        assert!(matches!(f.scan(Some("")), Verdict::CouldNotRun(_)));
    }

    #[test]
    fn not_a_git_repo_could_not_run() {
        let f = Fixture::new("nogit", &[("a.txt", "x")], "plantedsecret");
        let p = load_patterns(None, Some(&f.list())).unwrap();
        assert!(matches!(scan(&f.dir.join("nowhere"), &p), Verdict::CouldNotRun(_)));
    }

    #[test]
    fn staged_but_deleted_file_could_not_run() {
        let f = Fixture::new("gone", &[("a.txt", "x"), ("b.txt", "y")], "plantedsecret");
        let repo = f.repo();
        let ok = Command::new("git").arg("-C").arg(&repo).args(["add", "b.txt"]).status().unwrap();
        assert!(ok.success());
        std::fs::remove_file(repo.join("b.txt")).unwrap();
        assert!(matches!(f.scan(None), Verdict::CouldNotRun(_)));
    }
}
