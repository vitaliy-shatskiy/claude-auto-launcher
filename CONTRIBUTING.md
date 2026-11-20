# Contributing

One maintainer, small surface. Issues and PRs welcome. Expect a slow but real reply.

## Before a PR

```
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo clippy --no-default-features --all-targets --locked -- -D warnings
cargo test --no-default-features --locked
```

All five exit `0`; CI runs the same five on Windows. The last two build without the four optional
modules (cargo features `mouse`, `sharing`, `remote`, `rider-mcp`, all on by default). A change to
`Cargo.toml` or `Cargo.lock` also passes `cargo deny check` (`deny.toml`: advisories, bans,
licences, sources). `.config/nextest.toml` kills a hung test after 30 s when you run the suite
through `cargo nextest run`.

Try a screen change in preview mode first (README, [Preview mode](README.md#preview-mode)): it runs
no Claude and writes only its own projects cache in `%TEMP%`, but a `claude` must resolve on PATH
(a stub will do).

`cargo run --example privacy_scan` is the privacy scan (`examples/privacy_scan.rs`, not in the
release exe). It reads an optional pattern list from `CLAUDE_AUTO_CLEAN_PATTERNS` or `--patterns`.
Without one it only checks the current Windows username. It scans the git-tracked worktree, never
commit history or authorship. Exit `0` clean · `1` a hit · `2` could not run, which is not a pass.

## What a PR needs

- A test that fails without the change. A test that cannot fail is worse than no test.
- The gates green, quoted in the PR.
- No new dependency without a reason in the PR. The crate keeps to the few below.
- No `unwrap`/`expect` outside tests, no `#[allow]` (use `#[expect(lint, reason = "...")]`),
  `unsafe` only in an FFI module, one operation per block under a `// SAFETY:` comment.
- A screen change that alters a golden frame: rerun the tests with `CLAUDE_AUTO_BLESS=1` to
  rewrite the goldens in `tests/fixtures/wide-frames/`, then review their diff.
- A change on a hot path stays inside the speed budget ([perf/README.md](perf/README.md)).
- Windows only. Console behaviour differs enough between Windows Terminal, conhost and Rider's
  terminal that a change touching input or rendering should say which of them it was tried in.

## Module map

One package, `claude-auto-rs`: the library `src/lib.rs` and the binary `claude-auto` (`src/main.rs`).
Each module's `//!` header says more.

| Module | What |
|---|---|
| `app` | The launch in order: argument pass-through, config, log, the screens, account, secrets, sharing, hooks, MCP, exec; the headless entries and the no-UI prompts. |
| `atomic_file` | Whole-file writes no reader sees half of, reads and renames that ride out a moment's lock. |
| `background` | One job on its own thread while a screen is up, read without blocking. |
| `config` | `claude-auto.json`: every key, its validation and the warning a bad value gets; the roster; the schema test. |
| `env` | The environment around a launch: account, secrets, sharing repair, MCP mirror, launch hooks, MCP discovery, the model catalog, usage limits. |
| `frame` | Frame pieces every screen shares: footer, radio rows, click spans, the row map. |
| `input` | Input events and their decoding: keys, console mouse records, VT mouse reports, hotkeys on any layout. |
| `invocation` | The `claude` argument vector and how the child runs. |
| `launch` | The launch rows, the launch state and the claude arguments it produces; the launch screen. |
| `launcher_api` | `--launcher-options` and `--launcher-run`: the launch screen as JSON. |
| `layout` | Geometry in display cells: width, truncation, wrapping, boxes, panes. |
| `list` | List-screen pieces the picker and the project screen share. |
| `log` | The run log, one JSON record per line, 14 days kept. |
| `maintenance` | The maintenance screen, Claude Code builds on disk, the Claude Code update, the rows editor. |
| `out` | Console output: coloured lines and raw frame writes. |
| `perf` | Work counts (OS I/O counters) for the speed budget. |
| `plugins` | Plugin protocol 1: the verbs, the runner, the describe cache. |
| `prefs` | Remembered choices per account, saved under a per-file lock. |
| `projects` | The project registry, the project screen, the filter and the path navigator. |
| `remote` | The optional companion path (`remote` feature, author setup). |
| `self_update` | claude-auto self-update: the daily check, download, swap and roll back. |
| `sessions` | Transcripts, session summaries and their cache, the session picker and its chat preview. |
| `sys` | Every Win32 call, behind safe wrappers. |
| `theme` | Palette, glyphs and the pure painters; colour after layout. |
| `timing` | Debug timings: spans, the F12 screen, `--launcher-timings`. |
| `types` | Data several modules share. |
| `ui` | The terminal shell of every screen: alternate buffer, line-diff frame writes, the event wait, hit-testing, the preview terminal. |

## Dependencies

| Crate | Why |
|---|---|
| `windows-sys` | Raw Win32 FFI (no COM, smaller than `windows`): console, file system, junctions, ACLs, processes, WinHTTP for the release checks. |
| `serde`, `serde_json` (`preserve_order`) | Config, prefs, transcripts, logs. `preserve_order` keeps log records in a stable key order and the MCP mirror keeps the key order of `.claude.json`. |
| `regex` | The colour painters. No lookbehind: anchors are checked by hand (`launch/frame.rs`, `list.rs`). |
| `unicode-general-category` | Zero-width cells = Mn, Me, Cf. |

No crossterm: input needs raw console records (motion, VT text in key records).

## Examples

None of these ship in the release exe.

- `examples/plugin_sample.rs`: the reference plugin for protocol 1. `examples/plugin_git_branch.rs`:
  a useful one.
- `examples/bench.rs`: the speed budget's table with wall times ([perf/README.md](perf/README.md)).
- `examples/registry_bench.rs`, `examples/sessions_bench.rs`: time the project registry and the
  picker's first page against a real projects root, read-only.
- `examples/privacy_scan.rs`: the privacy scan above.

## Invariants every module keeps

- Preview has no side effect but its own projects cache in `%TEMP%`: no other file write, no
  network, no process start, no log record.
- The launch never fails because of an optional step: every optional step degrades (empty, `None`,
  a warning line) and never panics.
- Widths in display cells (`layout::display_width`), never `len()`. Colour after layout.
- Times enter as parameters (`now_ms`), never read inside pure code.
- Everything for the session goes into `ChildEnv`, never into the launcher's own environment.

## Style

Match the surrounding code. `rustfmt.toml` and `clippy.toml` set the style. Comments explain
*why*, and several in this repo record a bug that was actually hit: do not delete one without
understanding what it is guarding.
