//! The model catalog read out of the installed `claude.exe` (`Get-ModelCatalog*`,
//! `Read-ModelCatalogFrom*`, `Get-AnchorWindows`, `Get-ModelFamilyLabels`,
//! `Get-FriendlyModelName`, `Get-Model1mBracket`). The binary embeds one
//! `{id:"...",family:"...",display_name:"..."}` record per model and a `latest_per_family:{...}`
//! map, exactly what each alias starts. The scan is cached per binary (path, size, write time);
//! every failure degrades to the static labels.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

use regex::bytes::Regex;
use serde_json::{Value, json};

/// What the row says when the binary cannot be read. Same four families, same order, always.
pub(crate) const STATIC_FAMILY_LABELS: [(&str, &str); 4] = [
    ("fable", "Fable 5.2"),
    ("opus", "Opus 5.5"),
    ("sonnet", "Sonnet 5.5"),
    ("haiku", "Haiku 5.5"),
];

const CHUNK_BYTES: usize = 8 << 20;
const OVERLAP_BYTES: usize = 64 << 10;

#[expect(clippy::expect_used, reason = "constant patterns, proven by every test run")]
static RECORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?-u)\{id:"(claude-[a-z0-9.-]+)",family:"([a-z]+)",display_name:"([^"\\]+)""#).expect("record pattern"));
#[expect(clippy::expect_used, reason = "constant patterns, proven by every test run")]
static LATEST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?-u)latest_per_family:\{((?:[a-z]+:"claude-[a-z0-9.-]+",?)+)\}"#).expect("latest pattern"));
#[expect(clippy::expect_used, reason = "constant patterns, proven by every test run")]
static PAIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?-u)([a-z]+):"([^"]+)""#).expect("pair pattern"));

/// family -> latest id, id -> display name. Empty maps, never absent, when the text has nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Catalog {
    pub(crate) latest: BTreeMap<String, String>,
    pub(crate) names: BTreeMap<String, String>,
}

/// Where a catalog came from: the launcher mentions only the (slow) rescan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Cache,
    Binary,
}

/// Latin-1: one byte = one char, so a record never shifts on invalid UTF-8.
fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

/// `Get-AnchorWindows`: `[hit - before, hit + anchor + after]` around each ordinal occurrence,
/// clipped. Keeps the regexes off the bulk of a 200 MB scan.
pub(crate) fn anchor_windows<'a>(text: &'a [u8], anchor: &str, before: usize, after: usize) -> Vec<&'a [u8]> {
    let Ok(finder) = Regex::new(&format!("(?-u){}", regex::escape(anchor))) else {
        return Vec::new();
    };
    finder
        .find_iter(text)
        .filter_map(|m| text.get(m.start().saturating_sub(before)..(m.end() + after).min(text.len())))
        .collect()
}

/// `Read-ModelCatalogFromText`, over bytes.
pub(crate) fn from_text(text: &[u8], into: &mut Catalog) {
    for w in anchor_windows(text, "display_name:\"", 160, 120) {
        for c in RECORD.captures_iter(w) {
            into.names.insert(latin1(&c[1]), latin1(&c[3]));
        }
    }
    for w in anchor_windows(text, "latest_per_family:{", 0, 600) {
        for c in LATEST.captures_iter(w) {
            for p in PAIR.captures_iter(&c[1]) {
                into.latest.insert(latin1(&p[1]), latin1(&p[2]));
            }
        }
    }
}

/// `Read-ModelCatalogFromBinary`: chunked, the tail of each chunk carried into the next so a
/// record straddling a boundary is seen whole.
pub(crate) fn from_binary(path: &Path, chunk: usize, overlap: usize) -> std::io::Result<Catalog> {
    let mut f = std::fs::File::open(path)?;
    let mut cat = Catalog::default();
    let mut buf = vec![0u8; chunk];
    let mut text: Vec<u8> = Vec::new();
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(cat);
        }
        text.extend_from_slice(buf.get(..n).unwrap_or_default());
        from_text(&text, &mut cat);
        let keep = text.len().min(overlap);
        text.drain(..text.len() - keep);
    }
}

fn ticks(t: SystemTime) -> i128 {
    const UNIX_EPOCH_TICKS: i128 = 621_355_968_000_000_000;
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => UNIX_EPOCH_TICKS + i128::try_from(d.as_nanos() / 100).unwrap_or(0),
        Err(e) => UNIX_EPOCH_TICKS - i128::try_from(e.duration().as_nanos() / 100).unwrap_or(0),
    }
}

/// `'{FullName}|{Length}|{LastWriteTimeUtc.Ticks}'`: the PowerShell launcher's key, so both
/// launchers share one cache file.
fn cache_key(bin: &Path) -> Option<String> {
    let m = std::fs::metadata(bin).ok()?;
    let full = std::path::absolute(bin).ok()?.to_string_lossy().into_owned();
    Some(format!(
        "{}|{}|{}",
        full.strip_prefix(r"\\?\").unwrap_or(&full),
        m.len(),
        ticks(m.modified().ok()?)
    ))
}

fn string_map(v: Option<&Value>) -> BTreeMap<String, String> {
    v.and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

fn read_cache(cache: &Path, key: &str) -> Option<Catalog> {
    let text = std::fs::read_to_string(cache).ok()?;
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let latest = string_map(v.get("Latest"));
    (v.get("Key").and_then(Value::as_str) == Some(key) && !latest.is_empty()).then(|| Catalog {
        latest,
        names: string_map(v.get("Names")),
    })
}

/// `Get-ModelCatalog`: from the cache when the binary is unchanged, else a fresh scan (written
/// back). Preview answers from the cache and stops there: the scan writes. `None` = static labels.
pub(crate) fn load(bin: &Path, cache: &Path, preview: bool) -> Option<(Catalog, Source)> {
    if !bin.exists() {
        return None;
    }
    let key = cache_key(bin)?;
    if let Some(c) = read_cache(cache, &key) {
        return Some((c, Source::Cache));
    }
    if preview {
        return None;
    }
    let cat = from_binary(bin, CHUNK_BYTES, OVERLAP_BYTES).ok()?;
    if cat.latest.is_empty() {
        return None;
    }
    let body = json!({ "Key": key, "Latest": cat.latest, "Names": cat.names });
    if serde_json::to_string_pretty(&body)
        .map_err(std::io::Error::other)
        .and_then(|t| std::fs::write(cache, t))
        .is_err()
    {
        // An unwritable cache costs the next launch a rescan, never this one its labels.
    }
    Some((cat, Source::Binary))
}

/// `Get-ModelCatalogBinPath`: wherever `claude` resolves, else the native install path.
pub(crate) fn bin_path(home: &Path) -> PathBuf {
    let path = std::env::var("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    crate::invocation::find_on_path("claude", &path, &pathext).unwrap_or_else(|| home.join(r".local\bin\claude.exe"))
}

/// `Get-ModelFamilyLabels`: family -> what its alias starts today; the static table fills the rest.
pub(crate) fn family_labels(cat: Option<&Catalog>) -> Vec<(String, String)> {
    STATIC_FAMILY_LABELS
        .iter()
        .map(|(fam, fallback)| {
            let live = cat
                .and_then(|c| c.latest.get(*fam))
                .and_then(|id| cat.and_then(|c| c.names.get(id)));
            ((*fam).to_owned(), live.map_or_else(|| (*fallback).to_owned(), Clone::clone))
        })
        .collect()
}

/// `Get-Model1mBracket`: Sonnet's 1M context is native, the bracket would say nothing.
pub(crate) fn bracket_1m(family: &str) -> &'static str {
    if family.eq_ignore_ascii_case("sonnet") { "" } else { "[1M]" }
}

fn strip_1m(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let mut out = String::new();
    let mut rest = (raw, lower.as_str());
    while let Some(i) = rest.1.find("[1m]") {
        out.push_str(rest.0.get(..i).unwrap_or_default());
        rest = (rest.0.get(i + 4..).unwrap_or_default(), rest.1.get(i + 4..).unwrap_or_default());
    }
    out.push_str(rest.0);
    out.trim().to_owned()
}

/// `Get-FriendlyModelName`: an exact catalog id inside the value wins (longest first, so
/// `claude-fable-5` cannot shadow `claude-fable-5-1`); a bare alias falls to its family's label;
/// anything else renders as itself, cut to 24 characters.
pub(crate) fn friendly(raw: &str, cat: Option<&Catalog>) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let has_1m = raw.to_lowercase().contains("[1m]");
    let base = strip_1m(raw);
    let lower = base.to_lowercase();
    let by_id = cat.and_then(|c| {
        c.names
            .iter()
            .filter(|(id, _)| lower.contains(&id.to_lowercase()))
            .max_by_key(|(id, _)| id.len())
            .map(|(_, n)| n.clone())
    });
    let found = by_id.or_else(|| {
        family_labels(cat)
            .into_iter()
            .find(|(fam, _)| lower.contains(fam.as_str()))
            .map(|(_, l)| l)
    });
    let Some(mut name) = found else {
        if raw.chars().count() > 24 {
            return raw.chars().take(23).chain(['\u{2026}']).collect();
        }
        return raw.to_owned();
    };
    if has_1m {
        name.push_str(bracket_1m(if lower.contains("sonnet") { "sonnet" } else { "" }));
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(
        r#"junk{a:1}{id:"claude-fable-5",family:"fable",display_name:"Fable 5",knowledge_cutoff:"x"},"#,
        r#"{id:"claude-fable-5-1",family:"fable",display_name:"Fable 5.1",knowledge_cutoff:"June 2026"},"#,
        r#"{id:"claude-opus-4-8",family:"opus",display_name:"Opus 4.8"},{id:"claude-opus-5",family:"opus",display_name:"Opus 5"},"#,
        r#"{id:"claude-sonnet-5",family:"sonnet",display_name:"Sonnet 5"},{id:"claude-haiku-4-5",family:"haiku",display_name:"Haiku 4.5"}"#,
        r#"more junk latest_per_family:{fable:"claude-fable-5-1",opus:"claude-opus-5",sonnet:"claude-sonnet-5",haiku:"claude-haiku-4-5"},alias_migration"#
    );

    fn cat() -> Catalog {
        let mut c = Catalog::default();
        from_text(FIXTURE.as_bytes(), &mut c);
        c
    }

    fn label(cat: Option<&Catalog>, fam: &str) -> String {
        family_labels(cat).into_iter().find(|(f, _)| f == fam).map(|(_, l)| l).unwrap()
    }

    #[test]
    fn friendly_names_without_a_catalog() {
        assert_eq!(friendly("claude-fable-5-1[1m]", None), "Fable 5.2[1M]");
        assert_eq!(friendly("fable", None), "Fable 5.2");
        assert_eq!(friendly("opus[1m]", None), "Opus 5.5[1M]");
        assert_eq!(friendly("claude-sonnet-4-6", None), "Sonnet 5.5");
        assert_eq!(
            friendly("sonnet[1m]", None),
            "Sonnet 5.5",
            "a saved sonnet[1m] default draws no [1M] bracket"
        );
        assert_eq!(friendly("haiku", None), "Haiku 5.5");
        let long = "some-unknown-model-id-xyz-12345678";
        let cut = friendly(long, None);
        assert!(
            cut.chars().count() <= 24 && long.starts_with(cut.trim_end_matches('\u{2026}')),
            "{cut}"
        );
    }

    #[test]
    fn the_catalog_parser_reads_records_and_the_latest_map() {
        let c = cat();
        assert_eq!(c.latest.get("fable").map(String::as_str), Some("claude-fable-5-1"));
        assert_eq!(c.names.len(), 6);
        assert_eq!(c.names.get("claude-fable-5-1").map(String::as_str), Some("Fable 5.1"));
        let mut empty = Catalog::default();
        from_text(b"nothing here", &mut empty);
        assert_eq!(empty, Catalog::default());
    }

    #[test]
    fn family_labels_follow_the_catalog_and_fall_back() {
        let c = cat();
        assert_eq!(label(Some(&c), "fable"), "Fable 5.1");
        assert_eq!(label(Some(&c), "opus"), "Opus 5");
        assert_eq!(label(None, "fable"), "Fable 5.2");
        let partial = Catalog {
            latest: [("fable".to_owned(), "claude-fable-9".to_owned())].into(),
            names: BTreeMap::new(),
        };
        assert_eq!(
            label(Some(&partial), "fable"),
            "Fable 5.2",
            "a latest id with no display name keeps the static label"
        );
        assert_eq!(
            family_labels(None).iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(),
            ["fable", "opus", "sonnet", "haiku"]
        );
    }

    #[test]
    fn friendly_names_with_a_catalog() {
        let c = cat();
        assert_eq!(friendly("fable[1m]", Some(&c)), "Fable 5.1[1M]");
        assert_eq!(
            friendly("claude-fable-5[1m]", Some(&c)),
            "Fable 5[1M]",
            "a pinned older id is NOT shadowed"
        );
        assert_eq!(friendly("us.anthropic.claude-opus-4-8", Some(&c)), "Opus 4.8");
        assert_eq!(friendly("sonnet", Some(&c)), "Sonnet 5");
        assert_eq!(friendly("sonnet[1m]", Some(&c)), "Sonnet 5");
    }

    #[test]
    fn chunked_read_and_cache_against_a_fake_binary() {
        let dir = std::env::temp_dir().join(format!("claude-auto-rs-catalog-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (bin, cache) = (dir.join("claude.exe"), dir.join("models.json"));
        let padded = format!("{}{FIXTURE}{}", "x".repeat(4000), "y".repeat(9000));
        std::fs::write(&bin, &padded).unwrap();
        let chunked = from_binary(&bin, 4096, 1024).unwrap();
        assert_eq!(chunked.names.len(), 6, "records straddling a chunk boundary are still found");
        assert_eq!(chunked.latest.get("opus").map(String::as_str), Some("claude-opus-5"));

        assert_eq!(
            load(&bin, &cache, false).map(|c| c.1),
            Some(Source::Binary),
            "the first read scans the binary"
        );
        assert!(cache.exists(), "the scan result is written");
        let (second, src) = load(&bin, &cache, false).unwrap();
        assert_eq!(src, Source::Cache, "an unchanged binary is served from the cache");
        assert_eq!(second, chunked, "names and latest survive the JSON round trip");
        std::fs::write(&bin, padded.replace("Fable 5.1", "Fable 5.2")).unwrap();
        let (third, src) = load(&bin, &cache, false).unwrap();
        assert_eq!(
            (src, third.names.get("claude-fable-5-1").map(String::as_str)),
            (Source::Binary, Some("Fable 5.2"))
        );
        std::fs::write(&cache, "not json {{{").unwrap();
        assert_eq!(
            load(&bin, &cache, false).map(|c| c.1),
            Some(Source::Binary),
            "a corrupt cache is rescanned"
        );
        assert_eq!(load(&dir.join("missing.exe"), &cache, false), None);
        std::fs::write(&bin, "no catalog in this build").unwrap();
        assert_eq!(load(&bin, &cache, false), None, "a build without latest_per_family yields None");

        let (pbin, pcache) = (dir.join("p.exe"), dir.join("p.json"));
        std::fs::write(&pbin, FIXTURE).unwrap();
        assert_eq!(load(&pbin, &pcache, true), None, "preview: a cold cache is not scanned");
        assert!(!pcache.exists(), "preview: nothing is written");
        load(&pbin, &pcache, false).unwrap();
        assert_eq!(
            load(&pbin, &pcache, true).map(|c| c.1),
            Some(Source::Cache),
            "preview: a warm cache is served"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
