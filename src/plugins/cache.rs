//! The `describe` cache: one answer per plugin, valid while the plugin's file keeps its size and
//! write time and the entry keeps its args. A changed exe is described again. Exports `Cache`.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::{Map, Value, json};

use crate::config::PluginSpec;

/// The file's identity: size and write time (100 ns ticks).
fn stamp(path: &Path) -> Option<(u64, u128)> {
    let m = std::fs::metadata(path).ok()?;
    let t = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos() / 100;
    Some((m.len(), t))
}

fn key(spec: &PluginSpec) -> String {
    spec.path.to_lowercase()
}

/// The cache file, read once; `store` adds answers, `save` writes it when anything changed.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    file: Option<PathBuf>,
    entries: Map<String, Value>,
    dirty: bool,
}

impl Cache {
    /// `None` = no file: nothing is cached (tests, a missing config directory).
    pub(crate) fn open(file: Option<&Path>) -> Self {
        let entries = file
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        Self {
            file: file.map(Path::to_path_buf),
            entries,
            dirty: false,
        }
    }

    /// The cached answer while the file and the args are the ones it was given for.
    pub(crate) fn get(&self, spec: &PluginSpec) -> Option<&Value> {
        let (size, mtime) = stamp(Path::new(&spec.path))?;
        let e = self.entries.get(&key(spec))?;
        let same = e.get("size").and_then(Value::as_u64) == Some(size)
            && e.get("mtime").and_then(Value::as_str) == Some(mtime.to_string().as_str())
            && e.get("args") == Some(&json!(spec.args));
        same.then(|| e.get("describe")).flatten()
    }

    pub(crate) fn store(&mut self, spec: &PluginSpec, answer: &Value) {
        let Some((size, mtime)) = stamp(Path::new(&spec.path)) else { return };
        self.entries.insert(
            key(spec),
            json!({ "size": size, "mtime": mtime.to_string(), "args": spec.args, "describe": answer }),
        );
        self.dirty = true;
    }

    /// Writes the file when an answer was stored; a failed write only costs the next start a call.
    pub(crate) fn save(&self) {
        let (Some(file), true) = (&self.file, self.dirty) else { return };
        if let Ok(bytes) = serde_json::to_vec(&self.entries)
            && crate::atomic_file::write_atomic(file, &bytes).is_err()
        {
            // Described again next time.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(path: &Path) -> PluginSpec {
        PluginSpec {
            name: "p".into(),
            path: path.to_string_lossy().into_owned(),
            args: Vec::new(),
            timeout_ms: None,
        }
    }

    #[test]
    fn a_changed_exe_or_args_misses_the_cache() {
        let dir = std::env::temp_dir().join(format!("cars-plugin-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("p.exe");
        std::fs::write(&exe, b"one").unwrap();
        let file = dir.join("cache.json");
        let mut c = Cache::open(Some(&file));
        c.store(&spec(&exe), &json!({"protocol":1}));
        c.save();
        let c = Cache::open(Some(&file));
        assert_eq!(c.get(&spec(&exe)), Some(&json!({"protocol":1})), "read back");
        let mut other_args = spec(&exe);
        other_args.args.push("--x".into());
        assert_eq!(c.get(&other_args), None, "other args");
        std::fs::write(&exe, b"two-longer").unwrap();
        assert_eq!(c.get(&spec(&exe)), None, "a rebuilt exe is described again");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
