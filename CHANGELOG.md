# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions: [SemVer](https://semver.org/).

## [Unreleased]

### Changed

- The launch screen hides Fable where the account has no Fable, by the account's own weekly
  model bucket in claude-usage-widget's export (`availableModels`): the field missing means
  nothing is hidden, a list without `fable` hides it on the model and advisor rows. Nothing else
  is ever hidden. This replaces 0.2.0's plan-based rule, which could not tell two accounts on one
  Team plan apart and hid Opus everywhere.

## [0.2.0] — 2026-09-17

### Added

- A **project screen** between the launch screen and the session picker: the current directory
  first, then every known project (resolved from the transcripts, one row per real directory, paths
  middle-truncated and dimmed), then a free path. `/` filters; a filter parks the cursor on its first
  match. The last launched project is remembered per account.
- An **action field** on that screen (`new` / `continue` / `resume` / `worktree`), a radio row driven
  with the arrows or a click; hotkeys `c` `r` `t` set it and run at once. It is an indicator, never
  remembered.
- The session picker is **scoped to the chosen project**; `tab` widens it to the whole account.
  Sessions are paged, so the first frame costs one page rather than every transcript.
- **WASD** navigates every cursor screen beside the arrows. Footer hotkeys render as buttons: dim
  when idle, accent when hovered or when they name the current action.
- The launch screen **hides models the account's plan lacks**: on a Team plan Fable disappears from
  the model and advisor rows, a remembered Fable snaps to `default`, and a settings default that
  names Fable reads `default (plan default)`. The plan comes from claude-usage-widget's export
  (`plan`, re-fetched hourly); no record or an unknown plan hides nothing.
- One UI log: every screen and key writes a record through `Write-UiLog`; the UI block and the page
  fetcher log an error record before failing; `tools\Show-LauncherRun.ps1` prints a run as a timeline.
- `output-styles` joins the shared profile directories.

### Changed

- The mouse highlights instead of choosing: the pointer paints the row, value or footer button under
  it and moves nothing. A left click selects (on the session picker it also loads the preview), and a
  click on the row that already carries the cursor runs or opens it. A double-click has no meaning of
  its own - it is a select followed by a run.
- Frames are measured without a function call per character: `Get-DisplayWidth`, `Limit-Cells` and
  `Limit-CellsRight` decide printable ASCII and the zero-width markers inline and reach the width
  table only for the characters that need it. Same widths, a session picker that answers a mouse
  move far sooner.
- Moving the mouse over the project list or the session picker repaints only the lines whose
  highlight changed, instead of rebuilding the whole frame behind them. The picture is the one a
  full rebuild would have drawn; every other input still rebuilds.
- A cold frame measures each distinct piece of text once and remembers the answer, so the rows,
  borders and wrapped words that repeat down a screen and across redraws are no longer re-measured
  character by character.
- Every screen is a handler table over one `Invoke-ScreenLoop`: draw, wait, resize, mouse, arrows,
  Enter/Escape and the log records are written once; a screen contributes only what it alone does.
- Every frame stops one cell short of the last console column and every glyph is one cell wide, so
  nothing wraps on a terminal that fills the last column and no emoji-presentation glyph shifts a row.
- Session summaries: one cache per physical projects directory, a substring pre-filter before the
  JSON parse, a byte-bounded prompt counter (`N+` past the budget) — a cold picker opens in a
  fraction of the time it took.

### Fixed

- A physical double click is two clicks and nothing more: its second half is recognised as the same
  gesture by flag or by time, also across a screen change, so a rejected pick never prompts twice and
  a click that ends one screen never opens something on the next.
- A merged multi-slug project opened its picker on the whole account; a filter that hid one row
  disabled paging; the shared session cache pruned, blocked and lied under two writers; a FILE was
  accepted as a project directory; raw transcript text reached the screen — all found by an
  adversarial pass and closed.
- The launch screen's default label and the launcher's own arguments can never name a model the
  account's plan lacks.

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
