//! AC22: the default build carries all four optional modules; leaving one out is an opt-in build
//! (`--no-default-features`), never what `cargo build` gives.

const MANIFEST: &str = include_str!("../../Cargo.toml");

/// The names in `[features]`'s `default = [...]` line.
fn default_features() -> Vec<&'static str> {
    let line = MANIFEST
        .lines()
        .skip_while(|l| l.trim() != "[features]")
        .find(|l| l.split_once('=').is_some_and(|(k, _)| k.trim() == "default"))
        .unwrap();
    line.split('"').skip(1).step_by(2).collect()
}

#[test]
fn the_default_build_keeps_all_four_modules() {
    let names = default_features();
    for module in ["mouse", "sharing", "remote", "rider-mcp"] {
        assert!(names.contains(&module), "{module} is not in the default build: {names:?}");
    }
}
