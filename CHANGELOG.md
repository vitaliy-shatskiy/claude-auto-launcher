# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions: [SemVer](https://semver.org/).

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
