//! The release-channel check: what the channel serves now, cached so a burst of launches costs
//! one request and an offline launch does not pay a timeout every time.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

use super::install::{parse_version, write_atomic};

pub(crate) const RELEASE_ENDPOINT: &str = "https://downloads.claude.ai/claude-code-releases";
const TTL_MS: i64 = 30 * 60_000;
const FAILURE_TTL_MS: i64 = 5 * 60_000;

static CHANNEL_BODY: LazyLock<Regex> = LazyLock::new(|| crate::theme::re(r"^\s*(\d+\.\d+\.\d+\S*)\s*$"));
static VERSION_TOKEN: LazyLock<Regex> = LazyLock::new(|| crate::theme::re(r"(\d+\.\d+\.\d+\S*)"));

/// `Get-AutoUpdatesChannel`: `latest` or `stable` from settings.json, `latest` otherwise.
pub(crate) fn auto_updates_channel(settings: &Path) -> String {
    std::fs::read_to_string(settings)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
        .and_then(|v| v.get("autoUpdatesChannel").and_then(Value::as_str).map(str::to_owned))
        .filter(|c| c.eq_ignore_ascii_case("latest") || c.eq_ignore_ascii_case("stable"))
        .unwrap_or_else(|| "latest".into())
}

/// `Get-RemoteClaudeVersion`: the channel's version or `None`. A hit younger than 30 minutes
/// answers without a request; a failure younger than 5 minutes answers `None` without one.
pub(crate) fn remote_version(
    channel: &str,
    cache: &Path,
    now_ms: i64,
    fetch: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Option<String> {
    let cached = std::fs::read_to_string(cache)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok());
    if let Some(c) = cached {
        let same = c
            .get("Channel")
            .and_then(Value::as_str)
            .is_some_and(|ch| ch.eq_ignore_ascii_case(channel));
        let at = c.get("CheckedAtMs").and_then(Value::as_i64).filter(|v| *v != 0);
        if let (true, Some(at)) = (same, at) {
            let age = now_ms - at;
            let version = c.get("Version").and_then(Value::as_str).filter(|v| !v.is_empty());
            match version {
                Some(v) if (0..TTL_MS).contains(&age) => return Some(v.to_owned()),
                None if (0..FAILURE_TTL_MS).contains(&age) => return None,
                _ => {}
            }
        }
    }
    let version = fetch(&format!("{RELEASE_ENDPOINT}/{channel}")).ok().and_then(|body| {
        CHANNEL_BODY
            .captures(body.trim())
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_owned())
    });
    let record = json!({ "Channel": channel, "Version": version, "CheckedAtMs": now_ms });
    write_atomic(cache, &record.to_string());
    version
}

/// `Test-ClaudeUpdateAvailable`: both parse as versions and `remote` is strictly newer. An
/// unparseable side never starts an update.
pub(crate) fn update_available(installed: Option<&str>, remote: Option<&str>) -> bool {
    match (installed.and_then(parse_version), remote.and_then(parse_version)) {
        (Some(i), Some(r)) => r > i,
        _ => false,
    }
}

/// `Get-InstalledClaudeVersion`'s parse: the bare version out of `claude --version`, else the
/// raw line.
pub(crate) fn version_token(raw: &str) -> String {
    VERSION_TOKEN
        .captures(raw)
        .and_then(|c| c.get(1))
        .map_or_else(|| raw.to_owned(), |m| m.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(std::path::PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }
    fn tmp() -> Tmp {
        let d = std::env::temp_dir().join(format!("claude-auto-rs-w4-rel-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }

    #[test]
    fn remote_version_caches_by_channel_and_ttl() {
        let t = tmp();
        let upd = t.0.join("update-cache.json");
        let mut calls: Vec<String> = Vec::new();
        let t0 = 1_000_000_000_000;
        let mut fetch = |u: &str| -> Result<String, String> {
            calls.push(u.to_owned());
            Ok("2.1.280\n".into())
        };
        assert_eq!(
            remote_version("latest", &upd, t0, &mut fetch).as_deref(),
            Some("2.1.280"),
            "trimmed"
        );
        assert_eq!(
            remote_version("latest", &upd, t0 + 10 * 60_000, &mut fetch).as_deref(),
            Some("2.1.280")
        );
        remote_version("stable", &upd, t0 + 10 * 60_000, &mut fetch);
        remote_version("latest", &upd, t0 + 60 * 60_000, &mut fetch);
        assert_eq!(
            calls,
            vec![
                format!("{RELEASE_ENDPOINT}/latest"),
                format!("{RELEASE_ENDPOINT}/stable"),
                format!("{RELEASE_ENDPOINT}/latest"),
            ],
            "a hit makes no request, another channel and an expired entry do"
        );
    }

    #[test]
    fn remote_version_remembers_failures_briefly_and_rejects_garbage() {
        let t = tmp();
        let upd = t.0.join("update-cache.json");
        let t0 = 1_000_000_000_000;
        let mut failing = |_: &str| -> Result<String, String> { Err("no network".into()) };
        assert_eq!(remote_version("latest", &upd, t0, &mut failing), None, "a failed fetch yields None");
        let mut boom = |_: &str| -> Result<String, String> { panic!("must not be called") };
        assert_eq!(
            remote_version("latest", &upd, t0 + 2 * 60_000, &mut boom),
            None,
            "a failure is remembered"
        );
        let mut ok = |_: &str| -> Result<String, String> { Ok("2.1.280".into()) };
        assert_eq!(
            remote_version("latest", &upd, t0 + 10 * 60_000, &mut ok).as_deref(),
            Some("2.1.280")
        );
        let mut garbage = |_: &str| -> Result<String, String> { Ok("<html>maintenance</html>".into()) };
        assert_eq!(remote_version("latest", &upd, t0 + 100 * 60_000, &mut garbage), None);
        std::fs::write(&upd, "{not json").unwrap();
        assert_eq!(
            remote_version("latest", &upd, t0 + 200 * 60_000, &mut ok).as_deref(),
            Some("2.1.280"),
            "a corrupt cache is rewritten"
        );
    }

    #[test]
    fn update_available_is_strict_and_numeric() {
        assert!(update_available(Some("2.1.278"), Some("2.1.280")));
        assert!(!update_available(Some("2.1.280"), Some("2.1.280")));
        assert!(
            !update_available(Some("2.1.281"), Some("2.1.280")),
            "an older remote (stable) is not"
        );
        assert!(update_available(Some("2.1.9"), Some("2.1.10")));
        assert!(!update_available(Some("2.1.280 (Claude Code)"), Some("2.1.281")));
        assert!(!update_available(Some("2.1.280"), None));
    }

    #[test]
    fn channel_falls_back_to_latest() {
        let t = tmp();
        let f = t.0.join("settings.json");
        std::fs::write(&f, r#"{ "autoUpdatesChannel": "stable" }"#).unwrap();
        assert_eq!(auto_updates_channel(&f), "stable");
        std::fs::write(&f, r#"{ "autoUpdatesChannel": "nightly" }"#).unwrap();
        assert_eq!(auto_updates_channel(&f), "latest");
        assert_eq!(auto_updates_channel(&t.0.join("missing.json")), "latest");
    }

    #[test]
    fn version_token_takes_the_bare_version() {
        assert_eq!(version_token("2.1.280 (Claude Code)"), "2.1.280");
        assert_eq!(version_token("weird"), "weird");
    }
}
