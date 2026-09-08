# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions: [SemVer](https://semver.org/).

## [0.1.1] — 2026-09-09

### Fixed

- Caps Lock made every maintenance hotkey dead: `u` reaches the launcher as `U` with no Shift, and
  the uppercase guard rejected the real keypress. The guard stays — a genuine press carries
  `CAPSLOCK_ON` and a mouse report's coordinate byte does not.
- `Exit-AltBuffer` put Ctrl+C back as a hardcoded `false` instead of what it found.
- `CLAUDE_AUTO_PREVIEW` still wrote and swept `%TEMP%` files; the preview path now has no writes.

### Changed

- The regression check's drop list no longer carries one machine's launch-hook patterns. They come
  from `CLAUDE_AUTO_VOLATILE_PATTERNS`, one regex per line; a file that is named but unreadable
  exits `2` rather than comparing against a shorter list.
- `Get-LauncherEditDistance` (a Levenshtein matrix used only to suggest a mistyped config key)
  replaced by a prefix match; anything it cannot catch gets the full key list.
- CI moved to `actions/checkout@v5`. The timing-sensitive live console half of `Test-Input` gets one
  retry, announced in the output.

### Tests

- Eight assertions across three suites compared path SPELLING and passed only on a machine whose
  account name is short — CI on a GitHub runner (`C:\Users\RUNNER~1\...`) caught them on its first
  run. They now assert on what the path points at.

## [0.1.0] — 2026-09-08

First public release.

- Launch screen: account tab strip with five-hour usage bars, action (new / continue / resume /
  worktree), model, effort, advisor, permission and safe mode; every choice remembered per account.
- Session picker with a last-exchange preview, filter and fork.
- Maintenance screen: update, rename swap, doctor, mcp list, prune, plus configured actions.
- Config-driven accounts, MCP configs, secrets root, launch hooks and maintenance actions —
  nothing hardcoded to one machine.
- Optional multi-account sharing (hardlinks and junctions between account roots).
- Mouse and keyboard input including a Cyrillic-layout hotkey fallback and VT mouse reports
  decoded as text, for terminals that send them that way.
- 15-check verification gate (`tests\checkpoint.ps1`).

[0.1.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/releases/tag/v0.1.0
