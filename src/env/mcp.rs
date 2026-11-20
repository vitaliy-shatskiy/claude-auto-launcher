//! The `--mcp-config` files for a launch. Configured extras that exist, plus
//! the Rider MCP endpoint file in a build with the `rider-mcp` feature.

use std::path::Path;

use super::Note;
use crate::config::Config;

#[cfg(feature = "rider-mcp")]
mod rider;

/// The configured extras that exist; a missing one is skipped silently.
fn existing_extras(extras: &[String]) -> Vec<String> {
    extras.iter().filter(|e| !e.is_empty() && Path::new(e).exists()).cloned().collect()
}

/// The extras, then the Rider file per `riderMcp` (named, never written, in preview).
#[cfg(feature = "rider-mcp")]
pub(crate) fn config_paths(cfg: &Config, preview: bool) -> (Vec<String>, Vec<Note>) {
    rider::add_live(existing_extras(&cfg.extra_mcp_configs), cfg.rider_mcp, preview)
}

/// A build without the Rider scan: the extras alone.
#[cfg(not(feature = "rider-mcp"))]
pub(crate) fn config_paths(cfg: &Config, _preview: bool) -> (Vec<String>, Vec<Note>) {
    (existing_extras(&cfg.extra_mcp_configs), Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_existing_extra_is_passed_and_a_missing_one_skipped() {
        let dir = std::env::temp_dir().join(format!("claude-auto-rs-mcp-extras-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let extra = dir.join("extra.json");
        std::fs::write(&extra, "{}").unwrap();
        let extras = vec![
            extra.to_string_lossy().into_owned(),
            r"C:\nope\absent.json".to_owned(),
            String::new(),
        ];
        assert_eq!(existing_extras(&extras), vec![extras[0].clone()]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
