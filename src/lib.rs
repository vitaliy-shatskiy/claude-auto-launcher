//! claude-auto: picks the Claude account and launch options, then starts `claude` with the
//! caller's arguments passed through untouched. Rust port of the PowerShell launcher; the module
//! map and the porting record live in PORTING.md.

pub mod app;
pub(crate) mod atomic_file;
pub mod background;
pub mod config;
pub mod env;
pub mod frame;
pub mod input;
pub mod invocation;
pub mod launch;
pub mod layout;
pub mod list;
pub mod log;
pub mod maintenance;
pub mod out;
pub mod prefs;
pub mod projects;
pub mod remote;
pub mod sessions;
pub(crate) mod sys;
pub mod theme;
pub mod timing;
pub mod types;
pub mod ui;
