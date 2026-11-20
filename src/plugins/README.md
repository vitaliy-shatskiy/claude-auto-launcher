# src/plugins

Protocol 1 of the plugin processes (README, Plugins). One file per verb; `VERBS` in `plugins.rs`
is the registry `describe` checks hook and source names against. A new verb = a new file with its
`VERB` constant plus one line in `VERBS`, and its call site in `app`.

Invariants:

- Only `config::PluginSpec` entries run, by full path, exactly the file listed: config load and
  `run.rs` (before each spawn) refuse a missing file and one with an `<path>.exe` twin beside it,
  which Windows would start instead. `plugins::load` with `may_run` false (preview,
  `--launcher-options`) starts nothing.
- Every call goes through `run::Runner`; tests pass a fake. `Process` runs the child in a job
  object: a timeout or an oversized answer terminates the tree; a plugin that ends on its own
  releases the job, so what it started (an app an action opened) keeps running.
- Plugin output is untrusted: each verb file checks its answer before anything uses it. A failure
  is a warning string; only `pre_launch::Outcome::Cancel` stops a launch.
- Plugin rows are `RowName::Plugin(i)`, `i` counting every plugin's rows in config order
  (`Plugins::rows`); `Roster::add_plugin_row` must be called in the same order.

Callers: `app.rs` (load, preLaunch, postLaunch), `app/screens.rs` (actions, project and session
sources), `sessions::run_picker_with` (the session merge point).
