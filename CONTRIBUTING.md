# Contributing

One maintainer, small surface. Issues and PRs welcome. Expect a slow but real reply.

Two launchers live here: the PowerShell one (current default) and the Rust one (`claude-auto-rs`,
under test, replaces it at cut-over). A change to behaviour lands in both while both ship, or says
why not. [PORTING.md](PORTING.md) maps each PowerShell function to its Rust module.

## Before a PR

### Rust launcher

```
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

All three exit `0`. CI runs the same three on Windows. Try a screen change in preview mode first
(README, [Preview mode](README.md#preview-mode)): it needs no real Claude and writes nothing.

### PowerShell launcher

```
pwsh -File tests\checkpoint.ps1
```

19 checks, `0` green. Three of them need a machine-local reference recorded once. On a fresh clone
they correctly report `2 DID NOT RUN`, which is not a pass:

```
pwsh -File tests\check-regression.ps1 -Record   # also fixes the droplist row
pwsh -File tests\check-preview.ps1 -Record
```

A single suite runs on its own: `pwsh -File tests\Test-Ui.ps1` (exit `0` pass · `1` fail ·
`2` could not run). `Test-Input.ps1 -Live` adds the console-mode and mouse assertions, which need a
real console. The checkpoint always passes `-Live`.

`check-clean.ps1` is the privacy scan. It reads an optional pattern list from
`CLAUDE_AUTO_CLEAN_PATTERNS`. Without one it only checks the current Windows username. It scans the
git-tracked worktree, never commit history or authorship.

## What a PR needs

- A test that fails without the change. A test that cannot fail is worse than no test.
- The gates green, quoted in the PR.
- No new dependency without a reason in the PR. The Rust crate keeps to the few in `Cargo.toml`
  (why each: [PORTING.md](PORTING.md#dependencies)). The PowerShell launcher is PowerShell 7 plus
  one small C# input shim compiled at first run.
- Rust: no `unwrap`/`expect` outside tests, no `#[allow]` (use `#[expect(lint, reason = "...")]`),
  `unsafe` only in an FFI module, one operation per block under a `// SAFETY:` comment.
- Windows only. Console behaviour differs enough between Windows Terminal, conhost and Rider's
  terminal that a change touching input or rendering should say which of them it was tried in.

## Style

Match the surrounding code. `rustfmt.toml` and `clippy.toml` set the Rust style. Comments explain
*why*, and several in this repo record a bug that was actually hit: do not delete one without
understanding what it is guarding.
