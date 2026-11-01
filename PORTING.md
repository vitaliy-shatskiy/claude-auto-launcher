# PORTING - claude-auto PowerShell -> Rust

The porting record of the Rust launcher (`claude-auto-rs`): where each part of the PowerShell
launcher went, where the two still differ, and what changes at cut-over. The PowerShell code was
the spec: each module ported its `tests/Test-<X>.ps1` cases as Rust unit tests, under the same case
names. Both launchers run side by side until the Rust one has passed its test.

## Build, lint, test

- Toolchain pinned in `rust-toolchain.toml` (1.99.1), edition 2024, one package (`lib.rs` +
  `main.rs`).
- Lints in `Cargo.toml` `[lints]`: clippy pedantic, `unwrap`/`expect` denied, `unsafe_code` denied.
  A local override is `#[expect(lint, reason = "...")]` on the smallest item, never `#[allow]`. An
  FFI module opens with `#![expect(unsafe_code, reason = "...")]` and keeps one unsafe operation per
  block under a `// SAFETY:` comment.
- Gates: `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`,
  `cargo test --locked`. CI runs all three on Windows.
- Preview end to end: `CLAUDE_AUTO_PREVIEW=1`, `CLAUDE_AUTO_PREVIEW_KEYS=<keys>`,
  `CLAUDE_AUTO_PREVIEW_SIZE=WxH`, `CLAUDE_AUTO_PREFS=<temp file>`,
  `CLAUDE_AUTO_CONFIG=tests\fixtures\config-four.json`, `CLAUDE_AUTO_PROJECTS_ROOT=<temp dir>`, and
  `USERPROFILE` pointed at a temp directory. Never run a non-preview launch against a real
  `~/.claude`.

## Dependencies

| Crate | Why |
|---|---|
| `windows-sys` | The Win32 crate (chosen over `windows`: raw FFI, no COM, smaller): console, file system, junctions, ACLs, processes, WinHTTP for the release channel. |
| `serde`, `serde_json` (`preserve_order`) | Config, prefs, transcripts, logs. `preserve_order` keeps log records in `ts, stage, run, pid, ...` order like the PowerShell logger, and the MCP mirror keeps the key order of `.claude.json`. |
| `regex` | The painters (`Add-LaunchColor`, `Add-PickerColor`, ...) are regex-defined in PowerShell, and hand parsers would drift. No lookbehind: anchors are checked by hand (`launch/frame.rs`, `list.rs`). |
| `unicode-general-category` | Zero-width cells = Mn, Me, Cf, exactly `[CharUnicodeInfo]::GetUnicodeCategory` in `Get-CodePointWidth`. |

No crossterm: input needs raw console records (motion, VT text in key records).

## Module map

Unit tests sit in `<module>/tests.rs` beside a module, or inline in a small one. `tests/it/main.rs`
drives the built binary end to end.

| Module | PowerShell source |
|---|---|
| `main.rs` | `claude-auto.cmd` entry |
| `app.rs`, `app/prompts.rs`, `app/screens.rs` | `claude-auto.ps1`: the launch order, the preview summary, the no-UI fallback prompts (`Get-AccountKeyPrefixes`, `Get-AccountPrompt`, `Resolve-AccountAnswer`), the screen sequence, `Confirm-HeldSession` |
| `config.rs`, `config/roster.rs` | `Config.ps1` |
| `prefs.rs` | `Prefs.ps1` |
| `layout.rs` | `Layout.ps1` |
| `theme.rs` | `Theme.ps1` |
| `input.rs`, `input/console.rs` | `Input.ps1` (all but `Read-ClaudeFreePath`'s UI), `ConsoleInput.cs` |
| `sys.rs` | the Win32 calls of the core (FFI) |
| `ui.rs` | `Ui.ps1`: alt buffer, `Write-Frame`, `Wait-KeyOrResize`, `New-ScriptedKeyReader` (the preview terminal), `Get-HitAt`, `Read-ClaudeFreePath` |
| `ui/screen_loop.rs` | `Ui.ps1` `Invoke-ScreenLoop`, `Write-UiLog` |
| `frame.rs` | `Screens.ps1`: footers, radio rows, frame lines, `Complete-PickerFrame`, `Get-TooSmallFrame`, `Get-HeldSessionFrame`, the size constants |
| `list.rs` | `Screens.ps1`: `New-ListRow`, `Get-RowBandLength`, `Add-HoverSpanAt`, `Get-PickerPatterns`, `Add-PickerColor` |
| `launch.rs`, `launch/options.rs`, `launch/args.rs` | `Screens.ps1` rows, `Set-LaunchRoster`, `New-LaunchState`, the availability filter, `Step-*`, `Get-RowOptionText`, `Get-LaunchArgs`, `LaunchModes.ps1` |
| `launch/frame.rs` | `Get-LaunchFrame`, `Add-LaunchColor`, `Get-LimitItems`, `New-LimitLine`, `Get-AccountTint` |
| `launch/screen.rs` | `Invoke-LaunchScreen`, `Switch-LaunchTab` |
| `invocation.rs` | `Env.ps1`: `Test-ClaudeSubcommand`, `Get-ClaudeInvocation`, `Resolve-ClaudeExecutable`, the launch marker (child environment only) |
| `log.rs` | `Env.ps1`: `Write-LauncherLog`, `Remove-OldLauncherLogs`, `Get-LauncherParentChain` |
| `out.rs` | `Write-Host` with colours |
| `types.rs` | shared data: `LimitRecord`, `Limits`, `DefaultLabels`, `VersionInfo`, `ChildEnv` |
| `sessions.rs`, `sessions/*` | `Sessions.ps1`, `Ui.ps1` `Expand-SessionPage` and `Invoke-SessionPicker`, the picker half of `Screens.ps1` (`Get-PickerFrame`, `Get-MemoFrame`, `Get-SessionExchange`, ...). Submodules: `listing` (paged snapshot, summary cache), `transcript`, `frame` (with `frame/memo` and `frame/text`), `picker`, `holder` (held-session check), `win` |
| `projects.rs`, `projects/*` | `Projects.ps1`, `Ui.ps1` `Invoke-ProjectScreen`, `Screens.ps1` `Get-ProjectViewport`, `Get-ProjectFrame`, `Step-ProjectAction`, `Get-ProjectActions`. Submodules: `registry`, `paths`, `select`, `view`, `screen`, `win` |
| `env.rs`, `env/*` | `Env.ps1`: profile, project directory, default labels, sharing repair, secrets, launch hooks, MCP config, model catalog, rate limits. `sharing/claude-mirror-mcp.mjs`. Submodules: `sharing`, `mirror`, `secrets`, `hooks`, `mcp`, `models`, `limits`, `win_fs` |
| `maintenance.rs`, `maintenance/*` | `Maintenance.ps1`, `Ui.ps1` `Invoke-MaintenanceScreen`, `ConvertTo-StatusText`, `Invoke-ClaudeCommandText`, `Screens.ps1` `Get-MaintenanceFrame`, the update-check block of `claude-auto.ps1`. Submodules: `install`, `release`, `screen`, `win` |
| `remote.rs`, `remote/*` | `Remote.ps1`, the crc block of `claude-auto.ps1` |

Ported under another name or not needed:

- `Get-LauncherDefaults` -> `Config::defaults`. `Get-LaunchModes` -> `LAUNCH_MODES`. `Get-Ellipsis`
  -> `layout::ellipsis`. `Get-LaunchDefaultAccount`, `Get-LaunchRows` -> `Roster`.
- `Measure-ClaudePrompts`, `Read-BoundedFileLines` -> the transcript reader's 4 MiB cap with an
  `N+` count.
- Not needed: `ConvertTo-PrefsTable` and `Test-MapHas` (typed data),
  `Clear-PhysicalDirectoryPathCache` (no memo), `Initialize-ClaudeConsoleInput` (no C# shim).

## Invariants every module keeps

- Preview is side-effect-free: no file write, no network, no process start, no log record.
- The launch never fails because of an optional step: every optional step degrades (empty, `None`,
  a warning line) and never panics.
- Widths in display cells (`layout::display_width`), never `len()`. Colour after layout. Every data
  field goes through `theme::strip_ctl` before markers are added.
- Times enter as parameters (`now_ms`), never read inside pure code.
- Everything for the session goes into `ChildEnv`, never into the launcher's own environment.

## Deviations from the PowerShell launcher

- Preview size: `CLAUDE_AUTO_PREVIEW_SIZE` (new, default `78x24`). An exhausted key script prints
  `claude-auto: scripted keys exhausted` and exits 1 (PowerShell: an unhandled exception, exit 1).
- Prefs values are stored in the row's own casing after a case-insensitive match (PowerShell kept
  the file's casing, then looked it up case-insensitively). Same launch arguments either way.
- Keyboard-only (`CLAUDE_AUTO_NO_MOUSE=1`) still reads `CONIN$` records, armed without mouse input
  and with QuickEdit kept, instead of `[Console]::ReadKey`.
- The log's parent chain records pid and name. The command line (WMI in PowerShell) is not read.
- The `start` log record carries `psVersion: "rust <version>"` and `moduleMs: 0`.
- Column arithmetic of footers and cells counts Unicode scalars, PowerShell UTF-16 code units. They
  differ only for characters outside the BMP.
- VT text mouse: inside psmux/tmux (`TMUX` set, stdout a console, mouse on) `?1003h ?1006h` is
  written right after arming, `ENABLE_MOUSE_INPUT` kept, motion accepted as `MOUSE_MOVED` records
  and as SGR text (bit 32). Never outside `TMUX`. Elsewhere the switch on a text report requests
  `?1003h ?1006h` (any motion, SGR) instead of `?1003l ?1002l ?1000h ?1006h`. Exit resets 1006,
  1003, 1002 and 1000.
- Ctrl+C while `claude` runs is swallowed by a console handler in the launcher (the child keeps it).
  `CLAUDE_AUTO_LAUNCHED=1` and the profile variables are set on the child only.
- Colour of the status lines is ANSI and only when stdout is a terminal.
- The MCP mirror runs in-process (`env/mirror.rs`), not as `node sharing/claude-mirror-mcp.mjs`. It
  writes a temp file and renames it, with a change check before and after. `serde_json` refuses a
  lone-surrogate `\u` escape that Node accepted: the mirror then fails safe (a warning line, no
  write).
- Launch hooks get the console stdin as before, but their stdout and stderr are captured and printed
  as two blocks (stdout lines, then stderr lines), where PowerShell interleaved `2>&1`.
- Arguments are read as UTF-8 with lossy conversion, so an unpaired surrogate in an argument becomes
  U+FFFD.
- The pre-UI update check stays silent when the release channel cannot be reached at all (WinHTTP
  timeout, name not resolved, cannot connect, connection reset). Any other failed request prints
  `update check skipped: <reason>` with the reason in words (TLS, proxy, HTTP status). PowerShell
  stayed silent on every failed request. A remembered failure is silent for five minutes in both.
- A console whose input cannot be re-armed after a typed line (`read_line` in a prompt) ends the
  run with `claude-auto: the console input could not be re-armed after a typed line`, exit 1, logged
  as `ConsoleLost`. PowerShell had no such path.
- The sessions cache is a separate file, `claude-auto-rs-sessions.json`, until cut-over. The prefs,
  projects, models and hash caches are shared with the PowerShell launcher in compatible formats.

## Cut-over

When the Rust launcher has passed its test:

- The binary is renamed from `claude-auto-rs` to `claude-auto` (the `[[bin]]` name and the package
  name in `Cargo.toml`).
- The sessions cache `claude-auto-rs-sessions.json` is renamed to `claude-auto-sessions.json`.
- `sharing/claude-mirror-mcp.mjs` and `tests/Test-Mirror.*` go once nothing calls them: the Rust
  launcher has the mirror built in.
- The PowerShell launcher (`claude-auto.ps1`, `claude-auto/`, `claude-auto.cmd`, `install.ps1`) and
  its tests (`tests/*.ps1`) are retired, with the PowerShell job in CI.
