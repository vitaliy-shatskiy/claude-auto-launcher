# PORTING - claude-auto PowerShell -> Rust (phase B contract)

This file is the contract between phase A (core) and the four phase B workers. It is deleted at
cut-over, when `claude-auto-rs` is renamed to `claude-auto` and the PowerShell files go.

The PowerShell code is the spec. Read every function you port in full, port its suite
(`tests/Test-<X>.ps1`) as unit tests in the same Rust file, and keep every PowerShell file
untouched: the old launcher keeps running until the owner tests the new one.

## Build, lint, test

- Toolchain pinned in `rust-toolchain.toml` (1.99.1), edition 2024, one package (`lib.rs` + `main.rs`).
- Lints in `Cargo.toml` `[lints]` (pedantic, unwrap/expect deny, `unsafe_code` deny). Local overrides
  are `#[expect(lint, reason = "...")]` on the smallest item, never `#[allow]`.
- Every cargo command except `cargo fmt` runs as
  `pwsh -NoProfile -File ~/.claude-code-tools/shared/load-gate.ps1 -MaxPct 60 <cargo path> <args>`
  inside `with-timeout.ps1`.
- Done = `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` green.
- Preview end to end: `CLAUDE_AUTO_PREVIEW=1`, `CLAUDE_AUTO_PREVIEW_KEYS=<keys>`,
  `CLAUDE_AUTO_PREVIEW_SIZE=WxH` (new; default 78x24), `CLAUDE_AUTO_PREFS=<temp file>`,
  `CLAUDE_AUTO_CONFIG=tests\fixtures\config-four.json`. Never run a non-preview launch against the
  real `~/.claude`: point `USERPROFILE` at a temp directory for any real exec.

## Dependencies (fewest; nothing else without the integrator)

| Crate | Why |
|---|---|
| `windows-sys` 0.61 | The Win32 crate (chosen over `windows`: raw FFI, no COM, smaller). Features for phase B are pre-enabled: `Win32_System_IO`, `Win32_System_Ioctl` (junctions), `Win32_Security_Authorization` (ACL), `Win32_Networking_WinHttp` (release channel). |
| `serde`, `serde_json` (`preserve_order`) | Config, prefs, transcripts, logs. `preserve_order` keeps log records in `ts, stage, run, pid, ...` order like the PowerShell logger. |
| `anyhow` | Reserved for the app glue; no library signature uses it. |
| `regex` | The painters (`Add-LaunchColor`, `Add-PickerColor`, ...) are regex-defined in PowerShell; hand parsers would drift. No lookbehind: anchors are checked by hand (`launch/frame.rs`, `list.rs`). |
| `unicode-general-category` | Zero-width cells = Mn, Me, Cf, exactly `[CharUnicodeInfo]::GetUnicodeCategory` in `Get-CodePointWidth`. |

No crossterm: input needs raw console records (motion, VT text in key records).

## Module map

Owner `A` = phase A, done and shared: no phase B worker edits these files. A needed change to a
shared file goes into the worker's report, and the integrator makes it.

| Module | PowerShell source | Owner |
|---|---|---|
| `main.rs` | `claude-auto.cmd` entry | A |
| `app.rs` | `claude-auto.ps1` (order of the launch, preview summary, fallback prompts, `Confirm-HeldSession`), `Get-AccountKeyPrefixes`, `Get-AccountPrompt`, `Resolve-AccountAnswer` | A |
| `config.rs` | `Config.ps1` (all) | A |
| `prefs.rs` | `Prefs.ps1` (all) | A |
| `layout.rs` | `Layout.ps1` (all) | A |
| `theme.rs` | `Theme.ps1` (all) | A |
| `input.rs`, `input/console.rs` | `Input.ps1` (all but `Read-ClaudeFreePath`'s UI), `ConsoleInput.cs` | A |
| `sys.rs` | every Win32 call of phase A (the one FFI module) | A |
| `ui.rs` | `Ui.ps1`: `Test-AltBufferSupported`, `Enter/Exit-AltBuffer`, `Write-Frame`, `Wait-KeyOrResize`, `New-ScriptedKeyReader`, `Get-HitAt`, `Read-ClaudeFreePath` (as `Term::read_line`) | A |
| `ui/screen_loop.rs` | `Ui.ps1` `Invoke-ScreenLoop`, `Write-UiLog` (key/screen records) | A |
| `frame.rs` | `Screens.ps1`: `Get-FrameWidth`, `New-HintFooter`, `Add-HintColor`, `Get-FooterHover`, `Measure-CellSpans`, `New-RadioRow`, `Format-FrameLine`, `Complete-PickerFrame` (no memo), `Get-TooSmallFrame`, `Get-HeldSessionFrame`, the size constants | A |
| `list.rs` | `Screens.ps1`: `New-ListRow`, `Get-RowBandLength`, `Add-HoverSpanAt`, `Get-PickerPatterns`, `Add-PickerColor` | A |
| `launch.rs` | `Screens.ps1` rows, `Set-LaunchRoster`, `New-LaunchState`, availability filter, `Step-*`, `Get-RowOptionText`, `Get-LaunchArgs`; `LaunchModes.ps1` (all) | A |
| `launch/frame.rs` | `Get-LaunchFrame`, `Add-LaunchColor`, `Get-LimitItems`, `New-LimitLine`, `Get-AccountTint` | A |
| `launch/screen.rs` | `Invoke-LaunchScreen`, `Switch-LaunchTab` | A |
| `invocation.rs` | `Env.ps1`: `Test-ClaudeSubcommand`, `Get-ClaudeInvocation`, `Resolve-ClaudeExecutable`, `Enter/Exit-LaunchMarker` (child env only) | A |
| `log.rs` | `Env.ps1`: `Write-LauncherLog`, `Remove-OldLauncherLogs`, `Get-LauncherParentChain` | A |
| `out.rs` | `Write-Host` with colours | A |
| `types.rs` | shared data: `LimitRecord`, `Limits`, `DefaultLabels`, `VersionInfo`, `ChildEnv` | A |
| `sessions.rs` + `sessions/*` | `Sessions.ps1` (all), `Ui.ps1` `Expand-SessionPage`, `Invoke-SessionPicker`; `Screens.ps1` `Get-ListSignature`, `Get-SessionListSignature`, `Get-MemoFrame` + the memo half of `Complete-PickerFrame`, `Get-SessionExchange`, `Get-ExchangeLines`, `Select-ResumableSessions`, `Get-PromptCountValue`, `Format-PromptCount`, `Select-SessionMatch`, `Get-PickerFrame` | **W1** |
| `projects.rs` + `projects/*` | `Projects.ps1` (all), `Ui.ps1` `Invoke-ProjectScreen`; `Screens.ps1` `Get-ProjectViewport`, `Get-ProjectFrame`, `Step-ProjectAction`, `Get-ProjectActions` | **W2** |
| `env.rs` + `env/*` | `Env.ps1`: `Set-ClaudeProfile` mirror half, `Import-ProjectSecrets`, `Get-SharedFileId`, `ConvertTo-SortedJson`, `Test-VolatileOnlyDifference`, `Repair-SharedLink`, `Repair-SharedProfiles`, `Write-SharingNote`, `Repair-SharedSymlink`, `Invoke-LaunchHooks`, `Protect-SharedJunction`, `Repair-SharedJunction`, `New-ClaudeProfileRoot`, `Test-McpPort`, `Get-RiderMcpUrl`, `Get-McpConfigPaths`, the model catalog (`Get-ModelCatalog*`, `Read-ModelCatalogFrom*`, `Get-AnchorWindows`, `Get-ModelFamilyLabels`, `Initialize-ModelCatalog`, `Get-FriendlyModelName`, `Get-Model1mBracket`), `Get-FreshestRateLimitRecord`, `Get-RateLimitSummary`; `sharing/claude-mirror-mcp.mjs` | **W3** |
| `maintenance.rs` + `maintenance/*`, `remote.rs` + `remote/*` | `Maintenance.ps1` (all), `Remote.ps1` (all), `Ui.ps1` `Invoke-MaintenanceScreen`, `ConvertTo-StatusText`, `Invoke-ClaudeCommandText`; `Screens.ps1` `Get-MaintenanceFrame`; the update-check and crc blocks of `claude-auto.ps1` | **W4** |

Phase A already implements, inside the W3 file `env.rs`, the parts the launch path needs:
`set_profile` (the `CLAUDE_CONFIG_DIR` half), `set_project_directory`, `default_model_label`,
`default_advisor_label`, `default_labels`. W3 owns them from now on and keeps their signatures.

## Disjoint file ownership (four parallel worktrees)

| Worker | Writes only | Golden frames to match |
|---|---|---|
| W1 sessions + picker | `src/sessions.rs`, `src/sessions/**` | `tests/fixtures/wide-frames/picker-*.txt` |
| W2 projects + project screen | `src/projects.rs`, `src/projects/**` | `tests/fixtures/wide-frames/project-*.txt` |
| W3 environment | `src/env.rs`, `src/env/**` | - |
| W4 maintenance + remote | `src/maintenance.rs`, `src/maintenance/**`, `src/remote.rs`, `src/remote/**` | - |

Rules: new files only under your own paths (`sessions/memo.rs`, `env/win_fs.rs`, ...). A second
FFI module under your path opens with `#![expect(unsafe_code, reason = "...")]` and keeps one
unsafe operation per block under a `// SAFETY:` comment. Test fixtures: read the existing ones
under `tests/fixtures/`; a new fixture goes under `tests/fixtures/rs-<worker>/`.
`Cargo.toml`, `Cargo.lock` and every `A` file are the integrator's.

## Shared types and signatures (final)

All in the crate root namespace `claude_auto::`.

```rust
// types.rs
pub struct LimitRecord { five_hour: Option<i64>, seven_day: Option<i64>, age_text: String,
    model: Option<i64>, model_label: Option<String>, available_models: Option<Vec<String>> }
pub type Limits = BTreeMap<String, LimitRecord>;          // account key -> record
pub type DefaultLabels = BTreeMap<String, String>;        // "Model"|"Effort"|"Advisor"|"Permission" -> value
pub struct VersionInfo { installed: Option<String>, newest: Option<String> }
pub struct ChildEnv { vars: Vec<(String, Option<String>)> }  // set / remove for the CHILD only
impl ChildEnv { fn set(&mut self, &str, &str); fn remove(&mut self, &str); fn effective(&self, &str) -> Option<String> }

// ui.rs - every screen talks to the terminal through this
pub trait Term {
    fn size(&mut self) -> (usize, usize);
    fn paint(&mut self, lines: &[String]);
    fn wait(&mut self, size: (usize, usize)) -> Result<Event, Exhausted>;   // Wait-KeyOrResize
    fn window_top(&mut self) -> i32;
    fn input_pending(&mut self) -> bool;
    fn record_time(&self) -> Option<u64>;
    fn last_activation(&mut self) -> &mut Option<Activation>;               // P14 cross-screen twin
    fn drain(&mut self);                                                    // Clear-ClaudeInputQueue
    fn logger(&self) -> &Logger;
    fn read_line(&mut self, prompt: &str) -> String;                        // Read-ClaudeFreePath
}
pub enum Hit { None, Footer{span, index}, Action{value}, Cell{row, name, value}, LaunchRow{row, name}, ListRow(i32) }
pub fn hit_at(map: Option<&RowMap>, x: i32, y: i32, window_top: i32) -> Hit;

// ui/screen_loop.rs - Invoke-ScreenLoop; a screen = an impl of Screen (all handlers optional)
pub enum Flow<R> { Continue, Done(Option<R>) }            // Done(None) = cancel
pub struct Cursor { index: i32, hover: i32, hover_row: i32, hover_value: String, typing: bool }
pub trait Screen { type Output; fn name; fn cursor; fn draw(&mut self, size) -> Frame; fn before;
    fn rows -> Option<i32>; fn up/down -> bool; fn has_left_right; fn left/right -> Flow;
    fn enter/escape/tab -> Option<Flow>; fn hotkeys -> &[char]; fn hotkey(c) -> Flow;
    fn on_key(&KeyPress, &mut dyn Term) -> bool; fn type_key(&KeyPress) -> bool; fn wheel(i32) -> bool;
    fn click(&Hit) -> Flow; fn activate(&Hit) -> Option<Flow>; fn log_fields -> Map; fn silent -> bool }
pub fn run<S: Screen>(screen: &mut S, term: &mut dyn Term) -> Result<Option<S::Output>, Exhausted>;

// frame.rs
pub struct Style { color: bool, ascii: bool }
pub struct Frame { lines: Vec<String>, map: RowMap }
pub struct RowMap { rows: Vec<RowHit>, list: Option<ListMap>, action: Option<ActionHit>,
    footer_y: Option<i32>, footer: Vec<FooterSpan>, footer_lines: usize }
pub fn new_hint_footer(hints: &[Hint], g: &Glyphs, width: usize, plain: bool) -> Footer;
pub fn complete_frame(lines: &[String], footer: &Footer, width: usize, style: Style,
    painter: &dyn Fn(&str, bool) -> String, light: &FooterLight, map: &mut RowMap) -> Vec<String>;
pub fn new_radio_row(spec: &RadioSpec, g: &Glyphs) -> RadioRow;   // the project screen's action field
pub fn format_body_line(line, width, style, painter) -> String;   // the memo's repaint uses this chain

// list.rs
pub fn new_list_row(r: &ListRow) -> String;
pub fn add_picker_color(line: &str, enabled: bool, g: &Glyphs) -> String;   // body painter, picker AND project
pub fn row_band_length(row: &str, pane_width: usize) -> usize;
pub fn add_hover_span_at(line: &str, start: usize, length: usize) -> Option<String>;

// launch.rs
pub struct LaunchState { account, model, effort, advisor, permission, remote, action, mode: String,
    row: usize, hover: i32, hover_row: i32, hover_value: String, profiles: BTreeMap<String, Stash>,
    restored: Vec<String>, restored_age: String, project: String, project_slug: String, project_slugs: Vec<String> }
pub const LAUNCH_MODES: [LaunchMode; 4];  pub fn launch_mode(name: &str) -> Option<&'static LaunchMode>;
pub const PROJECT_SCREEN_OWN_KEYS: [char; 5];

// Phase B entry points, called by app.rs today (stubs) - keep these signatures:
sessions::sessions_root(cfg: &Config, account: &str) -> PathBuf
sessions::run_picker(term: &mut dyn Term, req: &PickerRequest) -> Result<Option<PickedSession>, Exhausted>
sessions::continue_session_holder(sessions_dirs: &[PathBuf], slugs: &[String], projects_root: &Path) -> Option<String>
sessions::session_holder(sessions_dirs: &[PathBuf], session_id: &str) -> Option<String>
sessions::held_warning(holder: &str) -> String
projects::project_registry(cache_path: Option<&Path>, profile_roots: &[String]) -> Vec<Project>
projects::set_launch_start_project(state: &mut LaunchState, cwd: &Path, projects: &[Project]) -> String
projects::update_start_selection(state: &mut LaunchState)          // also called by prefs::switch_account
projects::remove_stale_preview_cache(dir: &Path)
projects::run_project_screen(term: &mut dyn Term, projects: &[Project], cwd: &Path, style: Style) -> Result<Option<ProjectChoice>, Exhausted>
env::set_profile(cfg: &Config, account: &str, preview: bool, child: &mut ChildEnv) -> Result<(), String>
env::mirror_project_mcp(root: &str) -> Result<String, String>
env::import_project_secrets(cwd: &Path, secrets_root: &str, child: &mut ChildEnv) -> usize
env::set_project_directory(project: &str) -> bool
env::repair_shared_profiles(cfg: &Config, preview: bool)
env::run_launch_hooks(hooks: &[String])
env::mcp_config_paths(cfg: &Config, preview: bool) -> Vec<String>
env::model_family_labels(preview: bool) -> Option<BTreeMap<String, String>>
env::friendly_model_name(raw: &str) -> String
env::default_model_label / default_advisor_label(settings: &Path) -> String; env::default_labels(settings: &Path) -> DefaultLabels
env::rate_limit_summary(accounts: &[Account], dir: &Path, now_ms: i64) -> Limits
maintenance::read_version() -> VersionInfo
maintenance::update_before_launch(version: &mut VersionInfo)
maintenance::run_screen(term: &mut dyn Term, actions: &[MaintenanceAction], style: Style) -> Result<(), Exhausted>
remote::stop_companion_server() -> Result<String, String>
remote::launch_via_crc(req: &CrcLaunch) -> Option<i32>              // None = crc not on PATH
```

A worker that needs a signature changed (a new parameter `app.rs` must pass) says so in its report
with the exact new signature; the integrator applies it in `app.rs`. Add, never rename.

## Behaviours each module must keep

Every module: port its `tests/Test-<X>.ps1` cases that cover what it owns; the case names become
test names. The PowerShell comments record measurements and defects; keep the rule, drop the story.

**Everyone**
- Preview is side-effect-free: no file write, no network, no process start, no log record.
- The launch never fails because of an optional step: every phase B entry point degrades (empty,
  `None`, a warning line) and never panics; `app.rs` keeps launching.
- Widths in display cells (`layout::display_width`), never `len()`; colour after layout; every data
  field goes through `theme::strip_ctl` before markers are added.
- Times enter as parameters (`now_ms`), never read inside pure code.

**W1 sessions + picker**
- Paged, snapshotted lists (the file list enumerated once per scope; every page skips over THAT
  list); scoped by every slug of the project; Tab widens to the whole account.
- The sessions cache (`Write/Read-SessionsCache`) with its busy fallback; headless runs counted
  apart from empty sessions; resumable filter; `Format-RelativeAge`; `Get-CleanTranscriptText` maps
  C0 to a space.
- Master-detail at >= 100 columns, single column below; preview shrinks below 30 rows.
- The frame memo: repaint only the lines whose hover changed; a band that does not fit rebuilds.
- `continue_session_holder`/`session_holder`: read every profile's `sessions\`, any failure = `None`.
- Matches `picker-100x24`, `picker-100x30`, `picker-120x30-project-color` byte for byte (colour too).

**W2 projects + project screen**
- Registry: scan transcripts, merge one directory's slugs into one row, scratch and temp roots
  excluded, missing-drive projects kept, cache (preview uses its own per-PID file).
- Start selection: cwd when known or a git root, else the remembered project (existence and
  directory checked), else none; `update_start_selection` re-decides after a tab switch.
- The action field steps over `LAUNCH_MODES` (canonicalised, first row = fallback); mode hotkeys
  never on `PROJECT_SCREEN_OWN_KEYS`; `/` filter; the free path through `Term::read_line`
  (`"  path: "`), never logging the typed text, only its length.
- Matches `project-100x30`, `project-120x30-hover-color` byte for byte.

**W3 environment**
- Canonical account = no `CLAUDE_CONFIG_DIR`; everything for the session goes into `ChildEnv`,
  never into the launcher's own environment.
- Secrets: one file per variable, tiers shared -> org -> project, reserved shell names refused,
  never logged.
- Sharing repair: hardlinked `settings.json`/`statusline.js` (a copy differing only in
  `model`/`effortLevel`/`advisorModel` stays separate), file symlink for `CLAUDE.md`, junctions for
  the shared dirs with their ACL guard; preview touches nothing.
- MCP: `--mcp-config` paths as a list (never characters), Rider endpoint per `riderMcp`.
- Model catalog read out of the installed `claude.exe`, cached per binary; labels feed
  `Roster::set_model_labels`.
- Rate limits: freshest of `<name>.json` / `<name>.widget.json` by `atMs`; `available_models` read
  from the widget record only, three-state, `None` when older than 30 minutes.
- Open question for the owner (W3 does not decide): port `sharing/claude-mirror-mcp.mjs` to Rust
  (recommended: no node at launch) or keep calling node with the `.mjs`.

**W4 maintenance + remote**
- Maintenance screen opened by `u` from the launch screen and returning to it; after it,
  `Term::drain` (already called by `app.rs`).
- Update check before the UI unless preview or `CLAUDE_AUTO_NO_UPDATE=1`; release channel cached
  30 minutes; rename fallback when a running session holds the old binary.
- Confirm-twice actions gate on the RECORD time (`Term::record_time`), not the loop's clock.
- crc branch: only for a plain interactive launch (UI, or no args), never with `CLAUDE_NO_ROAM=1`;
  the `.cmd` shim so `--` survives; companion server, tailnet line, QR; exit code of crc.

## Phase A deviations from the PowerShell (documented)

- Preview size: `CLAUDE_AUTO_PREVIEW_SIZE` (new). An exhausted key script prints
  `claude-auto: scripted keys exhausted` and exits 1 (PowerShell: an unhandled exception, exit 1).
- Prefs values are stored in the row's own casing after a case-insensitive match (PowerShell kept
  the file's casing, then looked it up case-insensitively). Same launch arguments either way.
- Keyboard-only (`CLAUDE_AUTO_NO_MOUSE=1`) still reads `CONIN$` records, armed without mouse input
  and with QuickEdit kept, instead of `[Console]::ReadKey`.
- `Get-LauncherParentChain` records pid and name; the command line (WMI in PowerShell) is not read.
- The `start` record carries `psVersion: "rust <version>"` and `moduleMs: 0`.
- Column arithmetic of footers and cells counts Unicode scalars, PowerShell UTF-16 code units: they
  differ only for characters outside the BMP.
- VT text mouse (CS-1653): the switch requests `?1003h ?1006h` (any motion, SGR) instead of
  `?1003l ?1002l ?1000h ?1006h`; exit resets 1006, 1003, 1002 and 1000.
- Ctrl+C while `claude` runs is swallowed by a console handler in the launcher (the child keeps it);
  `CLAUDE_AUTO_LAUNCHED=1` and the profile variables are set on the child only.
- Colour of the `Write-Host` lines is ANSI and only when stdout is a terminal.
