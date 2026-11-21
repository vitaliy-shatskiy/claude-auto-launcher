//! claude-auto: picks the Claude account and launch options, then starts `claude` with the
//! caller's arguments passed through untouched.

pub mod app;
pub(crate) mod atomic_file;
pub mod background;
pub mod cockpit;
pub mod config;
pub mod env;
pub mod frame;
pub mod input;
pub mod invocation;
pub mod launch;
pub mod launcher_api;
pub mod layout;
pub mod list;
pub mod log;
pub mod maintenance;
pub mod out;
pub mod perf;
pub mod plugins;
pub mod prefs;
pub mod projects;
#[cfg(feature = "remote")]
pub mod remote;
pub mod self_update;
pub mod sessions;
pub(crate) mod sys;
pub mod theme;
pub mod timing;
pub mod types;
pub mod ui;
