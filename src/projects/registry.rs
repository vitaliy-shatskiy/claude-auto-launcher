//! `Get-ProjectRegistry`: every slug directory under the projects root, resolved from its newest
//! transcript's cwd, merged one row per real directory, ordered by last interactive activity. The
//! cache holds only derived data (path, git-root answer, headless verdict), so a lost or corrupt
//! write costs one recompute; written atomically, last writer wins.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{Map, Value, json};

use super::Project;
use super::paths::{DriveCache, is_scratch_key, on_network, path_present, project_key, scratch_key, scratch_keys};

/// What one build reads, all injected so a suite owns its tree.
#[derive(Debug, Clone)]
pub(crate) struct RegistryInput<'a> {
    pub projects_root: &'a Path,
    pub cache_path: &'a Path,
    pub temp_roots: &'a [String],
    pub profile_roots: &'a [String],
}

/// One transcript file of a slug folder.
#[derive(Debug, Clone)]
struct Transcript {
    path: PathBuf,
    name: String,
    modified: SystemTime,
}

/// .NET ticks (100 ns since 0001-01-01 UTC): the PowerShell launcher keys the shared cache on them.
fn ticks(t: SystemTime) -> i64 {
    const EPOCH_TICKS: i64 = 621_355_968_000_000_000;
    let hundreds = |d: Duration| i64::try_from(d.as_nanos() / 100).unwrap_or(i64::MAX);
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => EPOCH_TICKS.saturating_add(hundreds(d)),
        Err(e) => EPOCH_TICKS.saturating_sub(hundreds(e.duration())),
    }
}

pub(crate) fn unix_ms(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

/// The transcripts of one folder, newest first.
fn transcripts(dir: &Path) -> Vec<Transcript> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Transcript> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let is_jsonl = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("jsonl"));
            let meta = fs::metadata(&path).ok().filter(|m| m.is_file() && is_jsonl)?;
            Some(Transcript {
                path,
                name,
                modified: meta.modified().ok()?,
            })
        })
        .collect();
    out.sort_by_key(|t| std::cmp::Reverse(t.modified));
    out
}

/// `Get-ProjectPathFromTranscript`: the cwd of the first of the newest transcript's first 40
/// records that carries one. A slug is not reversible; the transcript is the only authority.
pub(crate) fn path_from_transcript(file: &Path) -> Option<String> {
    let mut r = BufReader::new(File::open(file).ok()?);
    let mut buf = Vec::new();
    for _ in 0..40 {
        buf.clear();
        if r.read_until(b'\n', &mut buf).ok()? == 0 {
            return None;
        }
        let Ok(rec) = serde_json::from_slice::<Value>(trim_line(&buf)) else {
            continue;
        };
        if let Some(cwd) = rec.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty()) {
            return Some(cwd.to_owned());
        }
    }
    None
}

fn trim_line(b: &[u8]) -> &[u8] {
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    let end = b.iter().rposition(|c| !c.is_ascii_whitespace()).map_or(0, |i| i + 1);
    b.get(..end).unwrap_or_default()
}

static ENTRYPOINT: LazyLock<Regex> = LazyLock::new(|| crate::theme::re(r#""entrypoint":\s?"([^"]*)""#));

/// `Test-HeadlessEntrypoint`: an entrypoint starting with `sdk-` is a headless run.
fn is_headless_entrypoint(e: &str) -> bool {
    e.starts_with("sdk-")
}

/// `Get-TranscriptHeadlessVerdict`: `Some(true)` when every entrypoint seen is headless,
/// `Some(false)` when any is not, `None` when none is seen or the file cannot be read. Seen = the
/// head up to its first entrypoint plus the last `window` bytes; a file up to two windows is read
/// whole.
pub(crate) fn headless_verdict(path: &Path, window: u64) -> Option<bool> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let head_max = if len <= 2 * window { len } else { window };
    let mut head = Vec::new();
    let mut chunk = [0u8; 8192];
    while (head.len() as u64) < head_max {
        let want = usize::try_from((head_max - head.len() as u64).min(8192)).unwrap_or(8192);
        let n = f.read(chunk.get_mut(..want)?).ok()?;
        if n == 0 {
            break;
        }
        head.extend_from_slice(chunk.get(..n)?);
        if head_max < len && ENTRYPOINT.is_match(&String::from_utf8_lossy(&head)) {
            break;
        }
    }
    let mut seen = String::from_utf8_lossy(&head).into_owned();
    if let Some(c) = ENTRYPOINT.captures(&seen)
        && !is_headless_entrypoint(c.get(1).map_or("", |m| m.as_str()))
    {
        return Some(false);
    }
    if head_max < len {
        f.seek(SeekFrom::Start(len - window)).ok()?;
        let mut tail = Vec::new();
        f.take(window).read_to_end(&mut tail).ok()?;
        seen.push('\n');
        seen.push_str(&String::from_utf8_lossy(&tail));
    }
    let mut any = false;
    for c in ENTRYPOINT.captures_iter(&seen) {
        any = true;
        if !is_headless_entrypoint(c.get(1).map_or("", |m| m.as_str())) {
            return Some(false);
        }
    }
    any.then_some(true)
}

/// `Test-HeadlessTranscript`: no entrypoint, or an unreadable file, is NOT headless.
#[cfg(test)]
pub(crate) fn is_headless_transcript(path: &Path) -> bool {
    headless_verdict(path, 65_536) == Some(true)
}

/// `ConvertFrom-ClaudeProjectSlug`'s worktree half: the name after `--claude-worktrees-`.
fn slug_worktree(slug: &str) -> Option<String> {
    slug.split_once("--claude-worktrees-").map(|(_, w)| w.to_owned())
}

/// `Find-ProjectInteractiveTranscript`: the newest transcript that is not a headless run. Only a
/// FOUND entrypoint is cached; a transcript read before its first records were written is asked
/// again next build.
fn interactive_transcript<'a>(
    slug: &str,
    ts: &'a [Transcript],
    cache: &Map<String, Value>,
    fresh: &mut Map<String, Value>,
) -> Option<&'a Transcript> {
    for t in ts {
        let key = format!("entrypoint|{slug}|{}|{}", t.name, ticks(t.modified));
        let headless = match cache.get(&key).and_then(|v| v.get("Headless")).and_then(Value::as_bool) {
            Some(h) => h,
            None => match headless_verdict(&t.path, 65_536) {
                None => return Some(t),
                Some(h) => h,
            },
        };
        fresh.insert(key, json!({ "Headless": headless }));
        if !headless {
            return Some(t);
        }
    }
    None
}

/// `Test-ProjectGitRoot`: `.git` is a DIRECTORY (a linked worktree's is a file).
fn is_git_root(path: &str) -> bool {
    !path.is_empty() && Path::new(path).join(".git").is_dir()
}

/// `Split-Path -Leaf`, with a drive root named as `C:`, not as the whole path again.
fn leaf_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let leaf = trimmed.rsplit(['\\', '/']).next().unwrap_or_default();
    if leaf.is_empty() { path.to_owned() } else { leaf.to_owned() }
}

fn read_cache(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// Atomic temp + rename; a failed write is a recompute next time, never an error.
fn write_cache(path: &Path, fresh: &Map<String, Value>) {
    let Ok(text) = serde_json::to_string_pretty(&Value::Object(fresh.clone())) else {
        return;
    };
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let tmp = PathBuf::from(format!("{}.{}-{:06x}.tmp", path.display(), std::process::id(), nonce & 0x00FF_FFFF));
    if fs::write(&tmp, text).is_err() || fs::rename(&tmp, path).is_err() {
        // Best effort: a temp file that cannot be removed is swept by nobody and harms nothing.
        fs::remove_file(&tmp).unwrap_or_default();
    }
}

/// One slug folder's row before the merge.
struct SlugRow {
    project: Project,
    interactive: bool,
}

/// Builds the registry. Never fails: a missing root is an empty list.
pub(crate) fn build_registry(inp: &RegistryInput<'_>) -> Vec<Project> {
    let Ok(dirs) = fs::read_dir(inp.projects_root) else {
        return Vec::new();
    };
    let cache = read_cache(inp.cache_path);
    let mut fresh = Map::new();
    let mut drives = DriveCache::new();
    let (temp_keys, profile_keys) = scratch_keys(inp.temp_roots, inp.profile_roots);
    let mut rows = Vec::new();
    for d in dirs.flatten() {
        let dir = d.path();
        if !dir.is_dir() {
            continue;
        }
        let slug = d.file_name().to_string_lossy().into_owned();
        let ts = transcripts(&dir);
        let Some(newest) = ts.first() else { continue };
        let interactive = interactive_transcript(&slug, &ts, &cache, &mut fresh);
        let activity = interactive.unwrap_or(newest).modified;
        let key = format!("{slug}|{}", ticks(newest.modified));
        let cached_path = cache
            .get(&key)
            .and_then(|v| v.get("Path"))
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
            .map(str::to_owned);
        let Some(path) = cached_path.or_else(|| path_from_transcript(&newest.path)) else {
            continue;
        };
        if path.contains('\0') {
            continue;
        }
        let mut entry = Map::new();
        entry.insert("Path".into(), json!(path));
        let present = path_present(&path, &mut drives);
        let network = on_network(&path, &drives);
        let mut headless_hidden = false;
        if present && interactive.is_none() && !network {
            let git = cache
                .get(&key)
                .and_then(|v| v.get("GitRoot"))
                .and_then(Value::as_bool)
                .unwrap_or_else(|| is_git_root(&path));
            entry.insert("GitRoot".into(), json!(git));
            headless_hidden = !git;
        }
        fresh.insert(key, Value::Object(entry));
        if !present {
            continue;
        }
        let scratch = !network && is_scratch_key(&scratch_key(&path), &temp_keys, &profile_keys);
        rows.push(SlugRow {
            project: Project {
                name: leaf_name(&path),
                path,
                slug: slug.clone(),
                slugs: vec![slug.clone()],
                worktree: slug_worktree(&slug),
                last_activity_ms: Some(unix_ms(activity)),
                hidden: headless_hidden || scratch,
            },
            interactive: interactive.is_some(),
        });
    }
    write_cache(inp.cache_path, &fresh);
    merge(rows)
}

/// One real directory is one row: every slug kept, hidden only when every slug is, dated by its
/// newest interactive slug.
fn merge(rows: Vec<SlugRow>) -> Vec<Project> {
    let mut groups: Vec<(String, Vec<SlugRow>)> = Vec::new();
    for r in rows {
        let k = project_key(&r.project.path);
        match groups.iter_mut().find(|(g, _)| *g == k) {
            Some((_, v)) => v.push(r),
            None => groups.push((k, vec![r])),
        }
    }
    let mut merged: Vec<Project> = groups
        .into_iter()
        .filter_map(|(_, mut g)| {
            g.sort_by_key(|r| std::cmp::Reverse(r.project.last_activity_ms));
            let dated = g.iter().find(|r| r.interactive).map(|r| r.project.last_activity_ms);
            let hidden = g.iter().all(|r| r.project.hidden);
            let slugs = g.iter().map(|r| r.project.slug.clone()).collect();
            let top = g.into_iter().next()?.project;
            Some(Project {
                slugs,
                last_activity_ms: dated.unwrap_or(top.last_activity_ms),
                hidden,
                ..top
            })
        })
        .collect();
    merged.sort_by_key(|p| std::cmp::Reverse(p.last_activity_ms));
    merged
}

/// `Remove-StalePreviewProjectsCache`: only the exact name a preview writes, older than a day.
pub(crate) fn remove_stale_preview_cache_at(dir: &Path, now: SystemTime) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let cutoff = now.checked_sub(Duration::from_hours(24)).unwrap_or(UNIX_EPOCH);
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let ours = name
            .strip_prefix("claude-auto-projects-preview-")
            .and_then(|r| r.strip_suffix(".json"))
            .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()));
        let old = e
            .metadata()
            .ok()
            .filter(fs::Metadata::is_file)
            .and_then(|m| m.modified().ok())
            .is_some_and(|m| m < cutoff);
        if ours && old {
            // Best effort: a file another process holds is pruned by a later run.
            fs::remove_file(e.path()).unwrap_or_default();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs::FileTimes;

    pub(crate) struct Tree(pub PathBuf);

    impl Tree {
        pub(crate) fn new(tag: &str) -> Self {
            let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let d = std::env::temp_dir().join(format!("cap-rs-{tag}-{}-{n}", std::process::id()));
            fs::create_dir_all(&d).unwrap();
            Self(d)
        }
        pub(crate) fn dir(&self, rel: &str) -> String {
            let p = self.0.join(rel);
            fs::create_dir_all(&p).unwrap();
            p.to_string_lossy().into_owned()
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap_or_default();
        }
    }

    pub(crate) fn ago(mins: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(mins * 60)
    }

    pub(crate) fn write_at(file: &Path, text: &str, when: SystemTime) {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        File::options()
            .write(true)
            .open(file)
            .unwrap()
            .set_times(FileTimes::new().set_modified(when))
            .unwrap();
    }

    fn user(cwd: &str, entry: Option<&str>) -> String {
        let mut v = json!({ "type": "user", "cwd": cwd });
        if let Some(e) = entry {
            v["entrypoint"] = json!(e);
        }
        format!("{{\"type\":\"queue-operation\"}}\n{v}\n")
    }

    fn build(root: &Path, cache: &Path, temp: &[String], profiles: &[String]) -> Vec<Project> {
        build_registry(&RegistryInput {
            projects_root: root,
            cache_path: cache,
            temp_roots: temp,
            profile_roots: profiles,
        })
    }

    fn slugs(r: &[Project]) -> String {
        r.iter().map(|p| p.slug.as_str()).collect::<Vec<_>>().join(",")
    }

    #[test]
    fn cwd_comes_from_the_transcript_not_the_slug() {
        let t = Tree::new("cwd");
        let real = r"C:\Users\x\Projects\Acme.Dashboard.Web-team-reports";
        let f = t.0.join("C--Users-x-Projects-Acme-Dashboard-Web-team-reports").join("s1.jsonl");
        write_at(&f, &json!({ "type": "user", "cwd": real }).to_string(), ago(1));
        assert_eq!(path_from_transcript(&f).as_deref(), Some(real));
    }

    #[test]
    fn only_resolvable_existing_projects_newest_first_and_cached() {
        let t = Tree::new("reg");
        let a = t.dir("real-a");
        let b = t.dir("real-b");
        let root = t.0.join("projects");
        write_at(&root.join("A").join("s.jsonl"), &user(&a, None), ago(5));
        write_at(&root.join("B").join("s.jsonl"), &user(&b, None), ago(2 * 1440));
        write_at(&root.join("C").join("s.jsonl"), &user(r"C:\no\such\dir\at\all", None), ago(0));
        fs::create_dir_all(root.join("D")).unwrap();
        let cache = t.0.join("cache.json");
        let reg = build(&root, &cache, &[], &[]);
        assert_eq!(slugs(&reg), "A,B");
        assert_eq!(reg[0].path, a);
        assert!(cache.exists(), "cache file written");
        fs::remove_file(root.join("B").join("s.jsonl")).unwrap();
        assert_eq!(slugs(&build(&root, &cache, &[], &[])), "A", "no transcript left drops out");
    }

    #[test]
    fn a_cache_hit_answers_without_rereading() {
        let t = Tree::new("hit");
        let p1 = t.dir("hit1");
        let p2 = t.dir("hit2");
        let root = t.0.join("projects");
        let f = root.join("Hit").join("s.jsonl");
        let when = ago(1);
        write_at(&f, &user(&p1, None), when);
        let cache = t.0.join("c.json");
        assert_eq!(build(&root, &cache, &[], &[])[0].path, p1);
        write_at(&f, &user(&p2, None), when);
        assert_eq!(build(&root, &cache, &[], &[])[0].path, p1, "same mtime: the cache answers");
    }

    #[test]
    fn a_corrupt_cache_rebuilds() {
        let t = Tree::new("corrupt");
        let x = t.dir("x");
        let root = t.0.join("projects");
        write_at(&root.join("X").join("s.jsonl"), &user(&x, None), ago(1));
        let cache = t.0.join("corrupt.json");
        fs::write(&cache, "{").unwrap();
        assert_eq!(build(&root, &cache, &[], &[]).len(), 1);
    }

    #[test]
    fn one_directory_is_one_row_with_every_slug() {
        let t = Tree::new("merge");
        let real = t.dir("Shared");
        let root = t.0.join("projects");
        write_at(&root.join("C--tmp-Shared").join("a.jsonl"), &user(&real, None), ago(60));
        write_at(
            &root.join("C--tmp-Shared-alt").join("b.jsonl"),
            &user(&real.replace('\\', "/"), None),
            ago(120),
        );
        let reg = build(&root, &t.0.join("c.json"), &[], &[]);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg[0].slug, "C--tmp-Shared");
        let mut s = reg[0].slugs.clone();
        s.sort();
        assert_eq!(s, ["C--tmp-Shared", "C--tmp-Shared-alt"]);
        let at = reg[0].last_activity_ms.unwrap();
        assert!(at <= unix_ms(ago(59)) && at >= unix_ms(ago(61)), "the newest activity of the two");
    }

    #[test]
    fn a_drive_root_is_named_and_a_nul_cwd_dropped() {
        let t = Tree::new("drive");
        let root = t.0.join("projects");
        let drive = format!("{}\\", t.0.to_string_lossy().get(..2).unwrap());
        write_at(&root.join("C--").join("d.jsonl"), &user(&drive, None), ago(1));
        write_at(
            &root.join("C--src-nul").join("n.jsonl"),
            r#"{"type":"user","cwd":"C:\\src\\\u0000bad"}"#,
            ago(1),
        );
        let reg = build(&root, &t.0.join("c.json"), &[], &[]);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg[0].path, drive);
        assert_eq!(reg[0].name, drive.trim_end_matches('\\'));
    }

    #[test]
    fn stale_preview_caches_pruned_by_exact_name() {
        let t = Tree::new("prune");
        let now = SystemTime::now();
        let old = now - Duration::from_hours(48);
        let names = [
            ("claude-auto-projects-preview-111.json", old, false),
            ("claude-auto-projects-preview-222.json", now, true),
            ("claude-auto-projects.json", old, true),
            ("claude-auto-projects-preview-333.json.bak", old, true),
            ("claude-auto-projects-preview-notes.json", old, true),
        ];
        for (n, when, _) in names {
            write_at(&t.0.join(n), "{}", when);
        }
        remove_stale_preview_cache_at(&t.0, now);
        for (n, _, kept) in names {
            assert_eq!(t.0.join(n).exists(), kept, "{n}");
        }
    }

    #[test]
    fn scratch_rows_stay_known_but_hidden() {
        let t = Tree::new("scratch");
        let fake_temp = t.dir("faketemp");
        let fake_profile = t.0.join(".claude-x").to_string_lossy().into_owned();
        let real = t.dir(r"work\real");
        let in_temp = t.dir(r"faketemp\claude\slug\session\rules\child");
        let job = t.dir(r".claude-x\jobs\76c3aa80\tmp\w585");
        let root = t.0.join("projects");
        write_at(&root.join("real").join("r.jsonl"), &user(&real, None), ago(1));
        write_at(
            &root.join("intemp").join("i.jsonl"),
            &user(&in_temp.to_uppercase().replace('\\', "/"), None),
            ago(2),
        );
        write_at(&root.join("job").join("j.jsonl"), &user(&job, None), ago(3));
        write_at(
            &root.join("gone").join("g.jsonl"),
            &user(&format!(r"{fake_temp}\gone"), None),
            ago(5),
        );
        let cache = t.0.join("c.json");
        let temp = [fake_temp];
        let prof = [fake_profile];
        for _ in 0..2 {
            let reg = build(&root, &cache, &temp, &prof);
            assert_eq!(slugs(&reg), "real,intemp,job");
            assert_eq!(reg.iter().map(|p| p.hidden).collect::<Vec<_>>(), [false, true, true]);
        }
    }

    #[test]
    fn headless_only_folders_hidden_unless_a_repository_root() {
        let t = Tree::new("hl");
        let root = t.0.join("projects");
        let dirs: Vec<String> = ["hl", "mx", "old", "vs", "gr", "wt"]
            .iter()
            .map(|n| t.dir(&format!("real-{n}")))
            .collect();
        fs::create_dir_all(Path::new(&dirs[4]).join(".git")).unwrap();
        fs::write(Path::new(&dirs[5]).join(".git"), "gitdir: C:/somewhere/.git/worktrees/wt\n").unwrap();
        let w = |slug: &str, name: &str, cwd: &str, e: Option<&str>, m: u64| {
            let f = root.join(slug).join(format!("{name}.jsonl"));
            write_at(&f, &user(cwd, e), ago(m));
            f
        };
        w("Hl", "h1", &dirs[0], Some("sdk-cli"), 1);
        w("Hl", "h2", &dirs[0], Some("sdk-ts"), 2);
        w("Mx", "m1", &dirs[1], Some("cli"), 120);
        let mx_headless = w("Mx", "m2", &dirs[1], Some("sdk-cli"), 4);
        w("Old", "o1", &dirs[2], None, 180);
        w("Vs", "v1", &dirs[3], Some("claude-vscode"), 240);
        w("Gr", "g1", &dirs[4], Some("sdk-cli"), 5);
        w("Wt", "w1", &dirs[5], Some("sdk-cli"), 6);
        let cache = t.0.join("c.json");
        let reg = build(&root, &cache, &[], &[]);
        let hidden: Vec<&str> = reg.iter().filter(|p| p.hidden).map(|p| p.slug.as_str()).collect();
        let listed: Vec<&str> = reg.iter().filter(|p| !p.hidden).map(|p| p.slug.as_str()).collect();
        assert!(hidden.contains(&"Hl") && hidden.contains(&"Wt"));
        assert_eq!(listed, ["Gr", "Mx", "Old", "Vs"], "listed by activity");
        let mx = reg.iter().find(|p| p.slug == "Mx").unwrap().last_activity_ms;
        let ms = |m: u64| unix_ms(ago(m));
        assert!(
            mx.unwrap() <= ms(119),
            "dated by the interactive session, not the newer headless run"
        );
        fs::remove_dir_all(Path::new(&dirs[4]).join(".git")).unwrap();
        let again = build(&root, &cache, &[], &[]);
        assert!(
            !again.iter().find(|p| p.slug == "Gr").unwrap().hidden,
            "git-root answer from the cache"
        );
        assert!(fs::read_to_string(&cache).unwrap().contains("entrypoint|Mx|m2.jsonl|"));
        let when = fs::metadata(&mx_headless).unwrap().modified().unwrap();
        write_at(&mx_headless, &user(&dirs[1], Some("cli")), when);
        let third = build(&root, &cache, &[], &[]);
        assert_eq!(
            third.iter().find(|p| p.slug == "Mx").unwrap().last_activity_ms,
            mx,
            "verdict from the cache"
        );
    }

    #[test]
    fn headless_verdict_reads_head_and_tail() {
        let t = Tree::new("verdict");
        let sdk = "{\"type\":\"user\",\"entrypoint\":\"sdk-cli\"}\n";
        let cli = "{\"type\":\"user\",\"entrypoint\":\"cli\"}\n";
        let pad = format!("{{\"type\":\"assistant\",\"text\":\"{}\"}}\n", "x".repeat(1000)).repeat(200);
        let f = |n: &str, s: String| {
            let p = t.0.join(n);
            fs::write(&p, s).unwrap();
            p
        };
        assert!(!is_headless_transcript(&f("small.jsonl", format!("{sdk}{cli}"))));
        assert!(!is_headless_transcript(&f("big-cli.jsonl", format!("{sdk}{pad}{cli}"))));
        assert!(is_headless_transcript(&f("big-sdk.jsonl", format!("{sdk}{pad}{sdk}"))));
        assert!(is_headless_transcript(&f("py.jsonl", "{\"entrypoint\":\"sdk-py\"}\n".into())));
        assert!(!is_headless_transcript(&f("old.jsonl", "{\"type\":\"user\"}\n".into())));
        assert!(!is_headless_transcript(&t.0.join("no-such.jsonl")));
    }

    #[test]
    fn interactive_slug_dates_and_shows_a_merged_folder() {
        let t = Tree::new("hlmerge");
        let root = t.0.join("projects");
        let plain = t.dir("merge-plain");
        let repo = t.dir("merge-repo");
        fs::create_dir_all(Path::new(&repo).join(".git")).unwrap();
        write_at(&root.join("Pa").join("a1.jsonl"), &user(&plain, Some("cli")), ago(120));
        write_at(
            &root.join("Pb").join("b1.jsonl"),
            &user(&plain.to_uppercase(), Some("sdk-cli")),
            ago(1),
        );
        write_at(&root.join("Ra").join("a1.jsonl"), &user(&repo, Some("cli")), ago(120));
        write_at(
            &root.join("Rb").join("b1.jsonl"),
            &user(&repo.to_uppercase(), Some("sdk-cli")),
            ago(1),
        );
        let reg = build(&root, &t.0.join("c.json"), &[], &[]);
        assert_eq!(reg.len(), 2);
        for p in &reg {
            assert!(!p.hidden, "{}", p.path);
            assert!(p.last_activity_ms.unwrap() <= unix_ms(ago(119)), "dated by the interactive slug");
        }
    }

    #[test]
    fn a_headless_share_cwd_is_never_probed_and_not_hidden() {
        let t = Tree::new("net");
        let root = t.0.join("projects");
        write_at(
            &root.join("Net").join("n1.jsonl"),
            &user("//claude-auto-no-such-host-hl/share/repo", Some("sdk-cli")),
            ago(1),
        );
        let reg = build(&root, &t.0.join("c.json"), &[], &[]);
        assert_eq!(reg.len(), 1);
        assert!(!reg[0].hidden);
    }
}
