# Contributing

One maintainer, small surface. Issues and PRs welcome; expect a slow but real reply.

## Before a PR

```
pwsh -File tests\checkpoint.ps1
```

15 checks, `0` green. Three of them need a machine-local reference recorded once — on a fresh clone
they correctly report `2 DID NOT RUN`, which is not a pass:

```
pwsh -File tests\check-regression.ps1 -Record   # also fixes the droplist row
pwsh -File tests\check-preview.ps1 -Record
```

A single suite runs on its own: `pwsh -File tests\Test-Ui.ps1` (exit `0` pass · `1` fail ·
`2` could not run). `Test-Input.ps1 -Live` adds the console-mode and mouse assertions, which need a
real console — the checkpoint always passes `-Live`.

`check-clean.ps1` is the privacy scan. It reads an optional pattern list from
`CLAUDE_AUTO_CLEAN_PATTERNS`; without one it only checks the current Windows username. It scans the
git-tracked worktree, never commit history or authorship.

## What a PR needs

- A test that fails without the change. A test that cannot fail is worse than no test.
- The checkpoint green, quoted in the PR.
- No new dependency. This is PowerShell 7 plus one small C# input shim compiled at first run.
- Windows only. Console behaviour differs enough between Windows Terminal, conhost and Rider's
  terminal that a change touching input or rendering should say which of them it was tried in.

## Style

Match the surrounding code. Comments explain *why*, and several in this repo record a bug that was
actually hit — do not delete one without understanding what it is guarding.
