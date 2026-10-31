//! `sharing/claude-mirror-mcp.mjs` in Rust: mirrors MCP servers from the canonical `.claude.json`
//! into a target profile's - the user scope (top-level `mcpServers`) and the project scope
//! (`projects[<dir>].mcpServers`, `disabledMcpServers` unioned). Work's definition wins; a server
//! only the target has is kept (and named in the user scope). Credentials travel unless
//! `copy_secrets` is off. The output names servers, never a value. The write is temp + rename,
//! guarded by an mtime+size check before and after the temp write; one retry, then abort.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Map, Value};

use super::sorted_json;

/// A name that would reach a JS object's prototype: skipped, as the node script did.
const UNSAFE_NAMES: [&str; 3] = ["__proto__", "constructor", "prototype"];
/// The fields that carry a server's credentials: `env` (stdio) and `headers` (http/sse).
const SECRET_FIELDS: [&str; 2] = ["env", "headers"];

#[derive(Debug, Clone, Default)]
pub(crate) struct Options {
    pub(crate) exclude: Vec<String>,
    pub(crate) copy_secrets: bool,
}

impl Options {
    /// `CLAUDE_AUTO_MIRROR_EXCLUDE` (comma list) and `CLAUDE_AUTO_MIRROR_SECRETS` (only "0" strips).
    pub(crate) fn from_vars(exclude: Option<&str>, secrets: Option<&str>) -> Self {
        Self {
            exclude: exclude
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            copy_secrets: secrets != Some("0"),
        }
    }
}

/// The file operations one run makes; the suite injects concurrent writers through it.
pub(crate) trait MirrorFs {
    fn exists(&mut self, p: &Path) -> bool;
    fn read(&mut self, p: &Path) -> std::io::Result<String>;
    fn stat(&mut self, p: &Path) -> Option<(SystemTime, u64)>;
    fn write(&mut self, p: &Path, data: &str) -> std::io::Result<()>;
    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn remove(&mut self, p: &Path);
}

#[derive(Debug)]
pub(crate) struct RealFs;

impl MirrorFs for RealFs {
    fn exists(&mut self, p: &Path) -> bool {
        p.exists()
    }
    fn read(&mut self, p: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(p)
    }
    fn stat(&mut self, p: &Path) -> Option<(SystemTime, u64)> {
        let m = std::fs::metadata(p).ok()?;
        Some((m.modified().ok()?, m.len()))
    }
    fn write(&mut self, p: &Path, data: &str) -> std::io::Result<()> {
        std::fs::write(p, data)
    }
    fn rename(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }
    fn remove(&mut self, p: &Path) {
        if std::fs::remove_file(p).is_err() {
            // Never written, or already gone.
        }
    }
}

fn read_json(fs: &mut dyn MirrorFs, file: &Path, label: &str) -> Result<Value, String> {
    let raw = fs.read(file).map_err(|e| format!("could not read {label}: {e}"))?;
    // Never the parser's own message near the bad token: this file holds tokens. Position only.
    serde_json::from_str(&raw).map_err(|e| format!("{label} is not valid JSON (at line {}, column {})", e.line(), e.column()))
}

fn without_secrets(cfg: &Value) -> Value {
    match cfg {
        Value::Object(m) => {
            let mut copy = m.clone();
            for f in SECRET_FIELDS {
                copy.shift_remove(f);
            }
            Value::Object(copy)
        }
        other => other.clone(),
    }
}

/// The per-server decision both scopes share: `None` when the target already agrees (key order
/// ignored), else the definition to write. Without secrets it compares and writes without the
/// credential fields and carries the target's own ones over.
fn next_definition(cfg: &Value, current: &Map<String, Value>, name: &str, copy_secrets: bool) -> Option<Value> {
    let have = current.get(name);
    if copy_secrets {
        return match have {
            Some(h) if sorted_json(h) == sorted_json(cfg) => None,
            _ => Some(cfg.clone()),
        };
    }
    let mut next = without_secrets(cfg);
    if let Some(h) = have {
        if sorted_json(&without_secrets(h)) == sorted_json(&next) {
            return None;
        }
        if let (Value::Object(n), Value::Object(hm)) = (&mut next, h) {
            for f in SECRET_FIELDS {
                if let Some(v) = hm.get(f) {
                    n.insert(f.to_owned(), v.clone());
                }
            }
        }
    }
    Some(next)
}

fn is_unsafe(name: &str) -> bool {
    UNSAFE_NAMES.contains(&name)
}

fn excluded(opts: &Options, name: &str) -> bool {
    opts.exclude.iter().any(|e| e == name)
}

/// User scope: returns (changes, notes). Mutates `personal.mcpServers` only when something changes.
fn merge_user(work: &Map<String, Value>, personal: &mut Map<String, Value>, opts: &Options) -> (Vec<String>, Vec<String>) {
    let (mut changed, mut notes) = (Vec::new(), Vec::new());
    let Some(Value::Object(servers)) = work.get("mcpServers") else {
        return (changed, notes);
    };
    let mut current = match personal.get("mcpServers") {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    for (name, cfg) in servers {
        if excluded(opts, name) {
            continue;
        }
        if is_unsafe(name) {
            notes.push(format!("{name} @ user (unsafe name, skipped)"));
            continue;
        }
        let has = current.contains_key(name);
        let Some(next) = next_definition(cfg, &current, name, opts.copy_secrets) else {
            continue;
        };
        current.insert(name.clone(), next);
        changed.push(format!("{name} @ user{}", if has { " (updated)" } else { "" }));
    }
    for name in current.keys() {
        if !servers.contains_key(name) && !excluded(opts, name) && !is_unsafe(name) {
            notes.push(format!("{name} @ user (only in the target, kept)"));
        }
    }
    if !changed.is_empty() {
        personal.insert("mcpServers".into(), Value::Object(current));
    }
    (changed, notes)
}

/// `path.win32.basename`: the last segment, trailing separators ignored.
fn basename(dir: &str) -> &str {
    dir.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or_default()
}

/// The object at `map[key]`, created when absent or null; `Err` when it holds something else.
fn object_at<'a>(map: &'a mut Map<String, Value>, key: &str) -> Result<&'a mut Map<String, Value>, String> {
    let slot = map.entry(key.to_owned()).or_insert(Value::Null);
    if slot.is_null() {
        *slot = Value::Object(Map::new());
    }
    slot.as_object_mut().ok_or_else(|| format!("target file: '{key}' is not an object"))
}

fn merge_project_servers(
    dir: &str,
    servers: &Map<String, Value>,
    entry: &mut Map<String, Value>,
    opts: &Options,
    added: &mut Vec<String>,
) -> Result<(), String> {
    let target = object_at(entry, "mcpServers")?;
    for (name, cfg) in servers {
        if excluded(opts, name) || is_unsafe(name) {
            continue;
        }
        let has = target.contains_key(name);
        let Some(next) = next_definition(cfg, target, name, opts.copy_secrets) else {
            continue;
        };
        // A new server names the credential fields it was copied without.
        let dropped: Vec<&str> = match cfg {
            Value::Object(m) if !has && !opts.copy_secrets => SECRET_FIELDS.into_iter().filter(|f| m.contains_key(*f)).collect(),
            _ => Vec::new(),
        };
        target.insert(name.clone(), next);
        let left_out = if dropped.is_empty() {
            String::new()
        } else {
            format!(" ({} not copied)", dropped.join(", "))
        };
        added.push(format!(
            "{name} @ {}{}{left_out}",
            basename(dir),
            if has { " (updated)" } else { "" }
        ));
    }
    Ok(())
}

/// Project scope: returns the changes. A disabled list is a union by value, written only when
/// something is genuinely missing.
fn merge_projects(work: &Map<String, Value>, personal: &mut Map<String, Value>, opts: &Options) -> Result<Vec<String>, String> {
    let mut added = Vec::new();
    let Some(Value::Object(projects)) = work.get("projects") else {
        return Ok(added);
    };
    for (dir, wp) in projects {
        if is_unsafe(dir) {
            continue;
        }
        if let Some(Value::Object(servers)) = wp.get("mcpServers")
            && !servers.is_empty()
        {
            let entry = object_at(object_at(personal, "projects")?, dir)?;
            merge_project_servers(dir, servers, entry, opts, &mut added)?;
        }
        if let Some(Value::Array(disabled)) = wp.get("disabledMcpServers")
            && !disabled.is_empty()
        {
            let entry = object_at(object_at(personal, "projects")?, dir)?;
            let current: Vec<Value> = match entry.get("disabledMcpServers") {
                Some(Value::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            let missing: Vec<Value> = disabled.iter().filter(|n| !current.contains(n)).cloned().collect();
            if !missing.is_empty() {
                let names: Vec<String> = missing
                    .iter()
                    .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
                    .collect();
                entry.insert(
                    "disabledMcpServers".into(),
                    Value::Array(current.into_iter().chain(missing).collect()),
                );
                added.push(format!("-{} @ {}", names.join(" -"), basename(dir)));
            }
        }
    }
    Ok(added)
}

struct Attempt {
    baseline: Option<(SystemTime, u64)>,
    personal: Value,
    added: Vec<String>,
    notes: Vec<String>,
}

fn load_and_merge(fs: &mut dyn MirrorFs, work: &Map<String, Value>, file: &Path, opts: &Options) -> Result<Attempt, String> {
    let baseline = fs.stat(file);
    let mut personal = read_json(fs, file, "target file")?;
    let map = personal.as_object_mut().ok_or("target file is not a JSON object")?;
    let (mut added, notes) = merge_user(work, map, opts);
    added.extend(merge_projects(work, map, opts)?);
    Ok(Attempt {
        baseline,
        personal,
        added,
        notes,
    })
}

/// Writes only when the target still has the stat it had when read, checked again right before
/// the rename. `Ok(false)` = another writer moved it.
fn write_if_unchanged(fs: &mut dyn MirrorFs, a: &Attempt, file: &Path, tmp: &Path) -> std::io::Result<bool> {
    let same = |fs: &mut dyn MirrorFs| a.baseline.is_some() && a.baseline == fs.stat(file);
    if !same(fs) {
        return Ok(false);
    }
    let text = serde_json::to_string_pretty(&a.personal).map_err(std::io::Error::other)?;
    fs.write(tmp, &text)?;
    if !same(fs) {
        fs.remove(tmp);
        return Ok(false);
    }
    fs.rename(tmp, file)?;
    Ok(true)
}

fn report(a: &Attempt) -> String {
    a.added.iter().chain(&a.notes).cloned().collect::<Vec<_>>().join(", ")
}

fn fail(reason: &str) -> String {
    format!("claude-mirror-mcp: {reason}")
}

/// `runMirror`: `Ok(line)` (empty = nothing to say) or `Err(one-line reason)`. Never panics.
pub(crate) fn run(work_file: &Path, personal_file: &Path, fs: &mut dyn MirrorFs, opts: &Options) -> Result<String, String> {
    if !fs.exists(work_file) || !fs.exists(personal_file) {
        return Ok(String::new());
    }
    let work = match read_json(fs, work_file, "work file").map_err(|e| fail(&e))? {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let mut attempt = load_and_merge(fs, &work, personal_file, opts).map_err(|e| fail(&e))?;
    if attempt.added.is_empty() {
        return Ok(report(&attempt));
    }
    let tmp = PathBuf::from(format!("{}.tmp-{}", personal_file.display(), std::process::id()));
    for round in 0..2 {
        match write_if_unchanged(fs, &attempt, personal_file, &tmp) {
            Ok(true) => return Ok(report(&attempt)),
            Ok(false) if round == 0 => {
                attempt = load_and_merge(fs, &work, personal_file, opts).map_err(|e| fail(&e))?;
                if attempt.added.is_empty() {
                    return Ok(report(&attempt));
                }
            }
            Ok(false) => {}
            Err(e) => {
                fs.remove(&tmp);
                return Err(fail(&format!("could not write target file: {e}")));
            }
        }
    }
    Err(fail(
        "target file changed twice during merge - aborting rather than lose a concurrent write",
    ))
}

#[cfg(test)]
#[path = "mirror_tests.rs"]
mod tests;
