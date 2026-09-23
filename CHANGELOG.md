# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions: [SemVer](https://semver.org/).

## [Unreleased]

## [0.4.2] — 2026-09-23

### Added

- A warning before `continue` or `resume` attaches to a session another `claude` process holds:
  Claude Code takes no lock, so two processes on one session fork its transcript. The warning
  names the session, the pid, its status and start time; Enter continues anyway, Esc goes back to
  the launch screen. A process counts as holding a session only while its pid runs with the same
  start time, the `sessions\` folder of every profile root is read, `continue` checks the session
  `claude -c` would pick, and a fork never warns. It wraps rather than cuts at phone width.

### Fixed

- An account's `settings.json` that differs from the newest copy only in `model` / `effortLevel` /
  `advisorModel` is no longer relinked: each account keeps its own `/model` and advisor choice. Any
  other difference is still relinked.
- The session and project readers (`Get-ClaudeSessions`, `Get-ClaudeSessionFile`,
  `Get-ProjectRegistry`) reject a misspelled parameter instead of running against the real
  projects root and rewriting its cache.

## [0.4.1] — 2026-09-23

### Added

- A phone layout below 100 columns: a one-line header, the three limit bars as one line drawn with
  box-drawing glyphs, every option row in its compact form, a detail line in place of the project
  list's path column, a two-line picker preview under 30 rows, and `claude-auto` as the terminal
  title. The minimum height drops from 20 to 16 rows. At 100 columns and wider every frame is
  byte-identical to 0.4.0.

### Fixed

- A pick that landed on a **folded default cell was not remembered**. Since 0.4.0's resolved
  defaults, the option that reads the same as the `settings.json` default (for example
  `Opus 5.5[1M]`, `high`, `auto`) is drawn as the default cell, and stepping or clicking onto it
  stored `default` - which the saved preferences read as "no choice", so the next launch came back
  with the previous value (for example `fable` / `ultracode` / `bypass`). The cell now stores the
  option it stands for: remembered, and passed as an explicit `--model` / `--effort` / `--advisor`
  / `--permission-mode`, a click on the cell after
  ctrl+r or on a fresh tab included. The screen is unchanged, an untouched default still adds no
  flag, and ctrl+r alone is still not saved.
- A tab whose remembered profile carries no timestamp of its own read `* restored ( ago)` after a
  switch; it now takes the file's timestamp, as the opening tab already did.
- Every preview run left `%TEMP%\claude-auto-projects-preview-<pid>.json` behind. A run now removes
  its own, and a preview sweeps the ones older than a day that killed runs left.

## [0.4.0] — 2026-09-22

### Added

- The model row reads its labels out of the installed `claude.exe`: the catalog embedded in the
  binary (`{id, family, display_name}` records plus `latest_per_family`, the map every alias
  resolves through) names what `fable` / `opus[1m]` start TODAY, so the screen can no longer say
  `Opus 5` on the day the CLI starts Opus 5.5. Cached per binary (path, size, mtime) in
  `~/.claude/claude-auto-models.json`; the hard-coded table is only the fallback for an npm shim or
  a preview with a cold cache. A preview run serves the cache and never scans or writes.
- Before the launch screen the launcher asks the release channel (`autoUpdatesChannel` in
  `settings.json`, `latest` by default) for its current version, cached 30 minutes in
  `~/.claude/claude-auto-update.json`, and when it is newer than the installed build runs the
  updater first - the same `u` path, rename swap included - so the catalog above describes the
  build that will actually run. A failed check is remembered five minutes, so an offline launch
  costs one timeout, not one per launch. `CLAUDE_AUTO_NO_UPDATE=1` skips the step; preview never
  touches the network.
- The launch rows show what `default` RESOLVES to for the current account, as the bare value,
  and fold the option that reads the same into that cell: with `effortLevel: high` the effort row
  is `low medium [high] xhigh max ultracode`, with `defaultMode: auto` the permission row is
  `plan [auto] acceptEdits bypass`, with `advisorModel: opus` the advisor row is
  `fable [opus] off`, and the model row's `default (Fable 5.1[1M])` becomes `Fable 5.1[1M]`. A key
  the account's `settings.json` does not set still reads `default`. A remembered value pinned to
  the folded twin highlights the default cell; arrows and clicks walk the same folded list. Read
  per account (`Get-DefaultLabels`), since the roots differ.

### Changed

- Account keys no longer need distinct first letters. The no-UI fallback prompt advertises each
  key's shortest unique prefix (`[mai]n / [mam]oru`) and resolves the key typed in full, the
  longest advertised prefix, or the default when the answer is ambiguous (`m` beside both).
- The header's update arrow names the release channel's version when nothing newer has been
  downloaded yet, and `claude --version` is reduced to its bare version token before the compare.

## [0.3.1] — 2026-09-22

### Changed

- The launch screen names the Opus family by its current release: `Opus 5.5[1M]` on the model row
  and `default (Opus 5.5)` for a settings default of `opus`. The `opus` / `opus[1m]` arguments are
  unchanged — the CLI resolves the alias, and 2.1.280 points it at Claude Opus 5.5.
- The full-form model row is now 96 characters, the capped inner width exactly, so it shows in full
  from a 102-column terminal instead of 100; at 96-101 columns the row uses its compact form, as it
  always did below that.

## [0.3.0] — 2026-09-22

### Fixed

- The sharing repair kept its backup **one deep**: `Copy-Item -Force` overwrote the previous
  `<file>.pre-relink`, so a second drift threw away the first losing copy — the one worth having,
  since by then the winning copy has replaced that root twice. The existing backup is rotated to
  `<file>.pre-relink.<yyyyMMdd-HHmmss>` and the newest three are kept.
- A **preview run writes nothing**. `Get-CachedFileHash` wrote `claude-auto-hash-cache.json` back
  on every cold entry, so `check-preview` was not the side-effect-free seam it claimed to be, and a
  first run on a new machine hashed 305 MB of `claude.exe` for a screen nobody looks at. A warm
  entry is still served, so a recorded preview reference does not move.
- `claude-auto --launcher-version` reported `0.1.0` on a 0.2.0 clone; the constant is part of the
  release now.
- README: the Tests section claimed 17 checks and 13 unit suites and omitted `Tools` (the
  checkpoint runs 18 and 14), and the sharing section said the losing copy "survives" as
  `.pre-relink`, which held only for the most recent event.

### Changed

- The launch screen hides Fable where the account has no Fable, by the account's own weekly
  model bucket in claude-usage-widget's export (`availableModels`): the field missing means
  nothing is hidden, a list without `fable` hides it on the model and advisor rows. Nothing else
  is ever hidden. This replaces 0.2.0's plan-based rule, which could not tell two accounts on one
  Team plan apart and hid Opus everywhere.
- A secret that is read by **path** rather than exported (a kebab-case name, a `.json` credential)
  no longer costs a warning line every launch: those names are reported as one summary line beside
  the loaded ones. A malformed name — a `=`, a space, a leading digit — still gets its own warning.
- The launcher regression check ignores the `rider MCP` preamble line. With `riderMcp: auto` its
  presence follows whether Rider happens to be running, so a reference recorded either way made the
  gate red on the other; what it covered is asserted in `Test-Remote` and `Test-Env`.

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
[0.3.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/releases/tag/v0.3.0
[0.3.1]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/releases/tag/v0.3.1
