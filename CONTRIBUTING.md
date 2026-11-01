# Contributing

One maintainer, small surface. Issues and PRs welcome. Expect a slow but real reply.

## Before a PR

```
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

All three exit `0`. CI runs the same three on Windows. Try a screen change in preview mode first
(README, [Preview mode](README.md#preview-mode)): it needs no real Claude and writes nothing.

`cargo run --example privacy_scan` is the privacy scan (`examples/privacy_scan.rs`, not in the
release exe). It reads an optional pattern list from `CLAUDE_AUTO_CLEAN_PATTERNS` or `--patterns`. Without one it only checks the current Windows username. It scans the
git-tracked worktree, never commit history or authorship. Exit `0` clean · `1` a hit · `2` could not
run, which is not a pass.

## What a PR needs

- A test that fails without the change. A test that cannot fail is worse than no test.
- The gates green, quoted in the PR.
- No new dependency without a reason in the PR. The crate keeps to the few below.
- No `unwrap`/`expect` outside tests, no `#[allow]` (use `#[expect(lint, reason = "...")]`),
  `unsafe` only in an FFI module, one operation per block under a `// SAFETY:` comment.
- Windows only. Console behaviour differs enough between Windows Terminal, conhost and Rider's
  terminal that a change touching input or rendering should say which of them it was tried in.

## Dependencies

| Crate | Why |
|---|---|
| `windows-sys` | Raw Win32 FFI (no COM, smaller than `windows`): console, file system, junctions, ACLs, processes, WinHTTP for the release channel. |
| `serde`, `serde_json` (`preserve_order`) | Config, prefs, transcripts, logs. `preserve_order` keeps log records in a stable key order and the MCP mirror keeps the key order of `.claude.json`. |
| `regex` | The colour painters. No lookbehind: anchors are checked by hand (`launch/frame.rs`, `list.rs`). |
| `unicode-general-category` | Zero-width cells = Mn, Me, Cf. |

No crossterm: input needs raw console records (motion, VT text in key records).

## Invariants every module keeps

- Preview is side-effect-free: no file write, no network, no process start, no log record.
- The launch never fails because of an optional step: every optional step degrades (empty, `None`,
  a warning line) and never panics.
- Widths in display cells (`layout::display_width`), never `len()`. Colour after layout.
- Times enter as parameters (`now_ms`), never read inside pure code.
- Everything for the session goes into `ChildEnv`, never into the launcher's own environment.

## Style

Match the surrounding code. `rustfmt.toml` and `clippy.toml` set the style. Comments explain
*why*, and several in this repo record a bug that was actually hit: do not delete one without
understanding what it is guarding.
