# Changelog

Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versions: [SemVer](https://semver.org/).

## [Unreleased]

## [0.9.1] — 2026-11-21

### Fixed

- Launch page: the focused sessions panel of a project with no sessions no longer marks
  `no sessions yet` (or `loading…`) as a selected row; a placeholder takes no cursor or fill.

## [0.9.0] — 2026-11-21

### Added

- `docs/media/screens.ps1`: PNGs of every screen (launch, project, all-projects picker,
  maintenance, path navigator, rows editor) at 280x72, 130x45 and 55x20 on the demo home, through
  VHS, never a visible window; `target/screens/`. Exit 0 all written, 1 one failed, 2 did not run.
- Launch keys `1`-`5` (or a click on the line) continue that recent project's newest session with
  the choices on screen, skipping the project screen, as its `c` would.

- A JSON Schema for the config, `schema/claude-auto.schema.json` (draft 2020-12): every key the
  launcher reads, with its type, allowed values, default and description. `config.example.json`
  points at it with `"$schema"`, which the launcher accepts and ignores. A test keeps the schema's
  keys, enums and defaults equal to the parser's.
- Config `version` (optional, default `1`): a file written by a newer claude-auto warns once and
  is read best-effort; a value that is not a whole number of at least 1 warns and reads as `1`.
- Plugin protocol 1 is forward-tolerant: every request carries `"protocol": 1`, unknown fields in
  a plugin's answers are ignored, and a plugin answering `describe` with a higher protocol is used
  for its protocol 1 fields with one warning (it was refused before). Protocol 1 may gain optional
  fields; a plugin must ignore fields it does not know.
- `examples/plugin_git_branch.rs`, an example plugin: `preLaunch` sets `CLAUDE_AUTO_GIT_BRANCH`
  to the launch directory's branch (read from `.git/HEAD`, no git process), launch-screen key `g`
  shows the branch and the number of `git status --porcelain` entries (git runs windowless).
- `deny.toml`: `cargo deny check` (advisories, bans, licences, sources) passes on the dependency
  tree.
- A speed budget in the repo: `perf/budget.json` holds the work counts of the hot paths (start to
  the first frame, the project screen, the picker opened over a cold and a warm summary cache, one
  cursor move at 250x68, the chat load of an 83 MB transcript), and `tests/perf` fails `cargo test`
  when a path reads or writes more than its budget plus 10 %. Counts come from the OS I/O counters,
  so the check does not depend on machine load; `examples/bench.rs` prints the counts with wall
  times for reference (`perf/README.md`).
- The speed budget covers the interactive picker's cold open in two counts: what the UI thread
  reads before the first frame (cache hits and quick rows only, 2.9 MB on the budget's world) and
  what the summary worker then reads to finish those pages (128 MB). A full scan moved back onto
  the UI thread fails the check.

- claude-auto self-update from its GitHub releases (config `selfUpdate`: `notify` by default,
  `auto`, `off`; `updateRepo` for a fork; `CLAUDE_AUTO_NO_SELF_UPDATE=1`): a daily background
  check, a dim line on the launch screen, maintenance `c` to update and `b` to roll back. The
  download is https only and checked against the release's SHA-256 before the running exe is
  renamed to `claude-auto.exe.old`; a failed step puts the old exe back. Keys `c` and `b` are now
  reserved for `maintenanceActions`.
- The session picker's preview is a chat: a title and a `DD.MM HH:MM · N msgs · size` line, a
  rule, then the newest messages filling the pane from the bottom, Claude's on the left under an
  orange `claude · DD.MM HH:MM` label, yours on the right in cyan. Tool calls, tool results,
  notifications and subagent lines are left out, markdown is shown as plain text (`2*3`, `a_b_c`
  and paths stay as written), an older message is cut at 8 lines and the newest one, when taller
  than the pane, shows its end. A worker thread reads only the transcript's tail (1 MB steps, 8 MB
  at most) once the cursor rests, and keeps the last 32 chats; the pane says `loading…` until it
  lands. Debug mode logs `picker.chat.load` with the bytes read.
- The picker has a colour hierarchy: the selected row is a grey fill in bold (the mouse hover a
  darker one), ages and the project name dim, a green dot before a session live now, the
  preview's title bold and its meta line dim but for the message count and size.
- Every screen repaints only the lines that changed since its last frame (a full rewrite on a
  resize, a new screen, after a child or a typed line), and input that arrives in one burst (mouse
  motion, key repeat) costs one frame; background work waits while input is pending. Debug mode
  logs `<screen>.key`, `<screen>.frame.build` and `<screen>.frame.write` (with its `bytes`).
- Pinned projects: `p` on the project screen pins the project under the cursor on top (config
  `pinnedProjects`, in pin order, whatever their recency), `p` again unpins it. Pinning a hidden
  project unhides it and hiding a pinned one unpins it; in a `/` search a pin only breaks ties. A
  pinned folder that no longer exists stays in the config and is listed dimmed, marked `(missing)`.
  Pinned rows carry a dim `↑` before the name.
- With `sharing` off, the session picker lists every account's sessions (even when the launching
  account has no `projects` folder yet) and tags another account's with its name; resuming one
  copies the transcript into the launching account first (an older copy there kept as `.bak`, a
  newer one left alone, a failed copy stops the launch, preview copies nothing). Ported from
  Reva1v's fork.
- `--launcher-options` prints what the launch screen offers as one line of JSON, starting nothing
  and writing nothing; `--launcher-run '<json>'` launches with those choices after checking them
  against the rows (exit `2` before any side effect when they do not fit) and does not save them.
  Ported from Reva1v's fork, with a JSON request instead of its flags.
- Plugins (config `plugins`): programs listed by full path, spoken to in JSON (protocol 1:
  `describe`, `preLaunch`, `postLaunch`, `action`, `projects`, `sessions`). They add rows to the
  launch screen, environment and arguments to the launch (or cancel it), a key on the launch
  screen, and entries to the project list and the resume picker, tagged with the plugin's name. A
  crash, a timeout (the process tree is killed) or bad output is one warning and the launch goes
  on; nothing unlisted ever runs. A plugin may not set `CLAUDE_CONFIG_DIR` or any `ANTHROPIC_*`
  variable. `examples/plugin_sample.rs` is a complete plugin.
- The mouse, multi-account sharing, the Remote row and the Rider MCP scan are cargo features
  (`mouse`, `sharing`, `remote`, `rider-mcp`), all on by default; a default build behaves as
  before. `cargo build --no-default-features` leaves all four out; a left-out module's config key
  (`mouse`, `sharing`, `remote`, `riderMcp`) warns that the build has no such module and the
  module stays off. CI builds and tests both ways.

### Changed

- The cockpit frame on the launch screen, maintenance, the project screen and the session picker:
  rounded panels titled in their top border, the focused one carrying `✻ claude-auto ─ <page>` and
  the versions in the accent colour, tinted interiors (24-bit where the terminal reports it), a
  filled bold selected row with `❯`, one footer line under the panels; under 120 columns the
  panels stack. The launch screen shows the selected account's limits one per line with the data
  age on the panel's last row, recent projects, running sessions and Claude Code's state;
  maintenance shows the last run, the versions and the warnings beside its actions; the project
  screen shows the selected project and the action beside its list; the picker's chat preview is
  its own panel. The rows editor and the path navigator are one focused cockpit panel each.
- Project screen beside the list (from 120 columns): the list panel takes what its rows need (full
  paths, at most 60 % of the width), the action field closes it on one line, and the right column
  shows the selected project's path, git branch, session count and newest session, then its
  sessions newest first with a green dot on a running one, from the summary cache off the UI thread.
- Launch: a two-column grid. The launch panel is as wide as its rows, recent under it at the same
  width; the 46-cell column right of them stacks limits, running and Claude Code with their borders
  in line; past the grid the terminal stays background; the session picker's chat text wraps at 110 cells; recent projects show their newest session's title, never the path; the Claude Code
  panel names the last maintenance MCP check and its age when one is stored.
- Maintenance: a hint too long for its row, and in a short stacked window the last run's state, go
  on their own line under the row; the group titles carry the version facts cut to fit; a short
  window drops the blank rows between groups first; with room for three columns the versions and
  warnings keep 66 cells and the last run's output takes the rest.
- The session picker's chat starts under the rule when it fits the pane; only a longer one ends its
  newest message on the last row.
- A cut panel title keeps a rule cell before the corner.
- Maintenance lists its actions in groups (Claude Code, Checks, claude-auto, Launcher, Custom);
  hints and live states share one right-aligned column. claude-auto's entries read
  `update to <version>` and `roll back`.
- Hovering a row label, a value, an action row, a recent project or a footer key paints it again
  on these screens, without selecting it; one test walks every clickable region of each screen.

- One page shell for every screen: a frame from the top row to the footer, the title on its top
  border (`─ <screen> ─ <facts> ─`), the text three cells in, the key footer under it. The launch
  screen joins it: its header box and the rule above the footer are gone.
- Launch screen: the limits are percentages on the top border's right end
  (`5h 4% · 7d 96% · fable 24% · 5 h ago`), no bar glyphs; the account row is one line. A narrow
  terminal drops the limits before the frame. The `* restored (…), ctrl+r resets` line is gone:
  the `*` marks stay and the footer names `ctrl+r reset` when a row was restored.
- Config and plugin warnings no longer print before the screens: maintenance lists them with
  where to fix them, and the launch footer badges `u maintenance ⚠ N` in yellow. A start without
  screens still prints them.
- Maintenance redesigned: a list of actions (`w`/`s` select, `enter` or the key runs) with short
  names (`update anyway (rename)`, `delete old builds`, `MCP servers`) and a hint on the selected
  row only, beside a facts grid (Claude Code version and whether it is the newest, builds on disk,
  claude-auto and its self-update, warnings, the last action). Hashes are no longer shown. A long
  action shows a live step line (what the command last printed and the seconds so far), then
  `done` or `failed: <reason>`. `m` MCP servers ends with `N of M connected` and the server lines.
- Every screen fills the whole terminal from its top-left corner at any size (0.9.0 centred a
  240x80 page on a larger terminal); clicks count from column 0. A size report of 0 or over
  10000 cells on a side is taken as 80x25 on that side.
- Project screen from 100 columns: compact columns instead of the path flushed right. The name
  column is the longest visible name (at most 40 % of the width), the path follows two cells
  after it, the age sits right-aligned just after the longest visible path.
- No row carries the dim `‹ a/d ←/→ ›` hint any more; the footer says it: `a/d ←/→ switch` on the
  launch screen, `a/d ←/→ action` on the project screen. On a narrow launch screen the footer
  keeps `w/s row` and the switch hint and wraps to a second line.
- The README states what 1.0 keeps stable (config keys, plugin protocol 1, the `--launcher-*`
  flags and their JSON, the `CLAUDE_AUTO_*` switches) and what stays internal.
- Picker chat: messages keep their markdown shape (paragraphs, `•` and numbered items with a
  hanging indent, bold headings, dimmed code blocks and tables, bold and inline code styled, no raw
  markers), bodies sit 2 cells in from their label's side, and each message shows from its start,
  capped at 14 lines plus `… N more lines`. Preview mode renders the same way.
- Picker chat streams the transcript backwards in 64 KB steps, newest message first, until the
  pane plus one screen is filled (8 MB cap): the first messages show after one small read instead
  of an up-to-8 MB tail read. A session seen once is kept while the picker is open (going back
  reads nothing); a grown transcript shows its earlier chat marked `updating…` meanwhile. A
  below-normal prefetch reads one step per idle moment for the sessions beside the cursor and the
  newest five (one pane, 1 MB each). Speed budget: `chat_load` 8 MB -> 64 KB, new `chat_revisit`
  (0 bytes) and `chat_prefetch_open`.

- Every screen is a full page: the launch screen, maintenance, the rows editor and the held-session
  question span the terminal's width (boxes and rules included) with the footer on the last row
  but one, like the project screen and the picker, and redraw on resize. A terminal over 240x80
  gets a 240x80 page centred horizontally at the top, clicks counted from its first column. From
  155 columns the launch screen's usage bars double to 16 cells.
- A launch screen narrower than 100 columns collapses only the rows that do not fit to
  `‹ value ›`; the rows that fit stay radio lists you can read and click (before, one row too wide
  collapsed them all).
- A row that switches with a/d says so: the project screen's action row, and on the launch screen
  the row under the cursor, end in a dim `‹ a/d ←/→ ›` (`< a/d <-/-> >` in ASCII).
- Session picker: a row is the session's title and age (the project only outside one project's
  view). The list pane stays 40 % wide, at most 72 columns; a tall picker loads more pages while
  idle.
- A session is titled by its `/rename` name, else Claude Code's newest AI title, else its first
  prompt. The message count counts every user and assistant message of the whole transcript (an
  assistant message written over several lines once), not only typed prompts.
- The session picker's summary cache is `claude-auto-sessions.json` again (0.6.0 kept the old
  `claude-auto-rs-` prefix); the first cache write removes the old `claude-auto-rs-sessions.json`
  beside it (that exact file, nothing else).

### Fixed

- The picker's green dot marked any session written to in the last minute; it now marks only a
  session a running Claude Code has open (`sessions\<pid>.json` in any account root, the pid alive
  with its recorded start time, so a reused pid does not count).
- Maintenance `m` (and `d`, `u`, the custom actions) no longer hang after the command finished:
  the output was read to the pipe's end, and an MCP server `claude mcp list` started inherited the
  pipe and held it open after `claude` exited. The read now ends a second after the exit.
- The session picker opened late (seconds on a cold disk) after every update: it now shows its
  rows at once, titled by their first prompt, and fills in titles and message counts as a
  background scan finishes them; an update no longer discards the session summary cache.
- Self-update `auto` installed a build again on the next start after `b` rolled it back; a
  rolled-back build now comes back only through `c`, and a newer release updates as before.
- Self-update trusted its record of the installed build after the exe was replaced by hand, and
  hid the update; a start now forgets a record the exe does not match. A download cut off by an
  exit (`claude-auto.exe.<pid>-<id>.new`) is deleted an hour later once its process is gone.
- Self-update `auto` downloaded ~10 MB again on every start after a failed update (a checksum
  mismatch, a locked folder); it now retries that release a day later and repeats the reason.
- Self-update `auto` dropped a download still running when the screens closed, so each start
  downloaded again; the check now lives until claude exits.
- The picker's chat dropped `*` and `_` from paths and globs taken for emphasis
  (`src/**/*.rs`, `__init__.py`, `\__pycache__\`); next to `/`, `\` or `.` they stay.
- The picker showed every time at today's UTC offset, so a message from before the DST change
  was an hour off; each time now takes the offset of its own date.

## [0.8.2] — 2026-11-11

### Fixed

- A power cut during a save can no longer leave `claude-auto.json` or the prefs file empty: both
  are flushed to disk before they replace the old file (caches still skip the flush).
- Two launchers saving the config at once no longer lose each other's keys: the config save now
  runs under the same per-file lock as the prefs save, and a config that another writer is
  replacing at that moment is never mistaken for a missing one and rebuilt from nothing.
- A file held for a moment by an antivirus scan or another reader (access denied, sharing
  violation) is retried three times 25 ms apart before a save or read gives up; a prefs save that
  still fails, or that the lock skipped, now writes an `error` record to the run log.
- Saving through a symlink keeps the link for every file the launcher writes, a hard-linked config
  or prefs file stays linked (rewritten in place), and hidden and system attributes survive a save.
- Two saves of one process in the same clock tick can no longer pick the same temp file name.

## [0.8.1] — 2026-11-11

### Changed

- The launch screen no longer waits for the project list: the registry is built on a background
  thread while the launch screen is up, and Enter waits only for what is left of that one build.
  A warm build reads each transcript's time from the folder listing instead of opening every file
  (about 3x faster on 500 folders). Debug timings split `load:project` into `registry:scan`,
  `registry:cache read`, `registry:derive`, `registry:cache write`, plus `wait:project`.
- The launch screen no longer waits for the Claude Code update check: the check (and `claude
  update` when a newer build is out) runs on a background thread, the screen draws at once with
  the installed version and redraws the header and model row in place when the check changes
  them (the `default (X)` labels too), keeping the selection. Enter waits for a check still
  running, so claude never starts mid-update (`wait:update`); its messages print after the screen
  closes. Esc quits at once unless `claude update` itself is running, and `u` opens maintenance
  only after the check, so two updates never run at once.
- A wait after Enter longer than 150 ms shows `loading projects...` or `checking for a Claude Code
  update...` instead of a frozen frame.
- The run log's `preselect` and `projectSource` now describe the start project of the final tab
  (the rule runs once after Enter).

### Fixed

- Esc, a cancel or an error exit no longer waits for the background registry build: its cache
  write is temp + rename, and day-old cache temps an exit left are swept. Only a preview waits for
  it (its per-run cache file is removed right after). A registry build that panics is logged
  (`error`, `where=registry`) and prints nothing over the screen.
- A stuck update wait can be left: Esc or Ctrl+C while `checking for a Claude Code update...` is
  shown quits without launching (claude never starts mid-update), and during `u` returns to the
  launch screen without opening maintenance. A `claude update` already running finishes on its
  own; the status line says so.
- A project whose newest transcript is held open by its writer no longer shows a last activity
  hours old: that one file's time is re-read by path (the folder listing lags on NTFS).
- The model catalog is read before the update check starts, so the check's re-read after an
  install never writes the catalog cache at the same time.
- Every launcher file write (prefs, config edits, the install and release-channel caches, the
  project registry cache, the model catalog cache) goes through one temp + rename write: a reader
  never sees half a file, two writers in one process never share a temp, and day-old temps an
  exit left are swept. The model catalog cache was a plain write before.

## [0.8.0] — 2026-11-11

### Added

- Project screen: `enter a path...` opens an in-screen path navigator instead of a console line:
  a text field, the typed parent's folders filtered as you type, the drives on an empty field, and
  `use this folder` and `..` rows. Enter or right enters a folder, left goes up, Backspace on an
  empty segment goes up, Ctrl+Backspace drops a segment, Tab completes and a second Tab cycles, Esc
  goes back with no message. Only a full path (`C:\...`, `\\server\share\`) is listed. Network
  folders are listed only on Tab (`no answer in 1000 ms` for a dead share; a late answer is still
  used, one listing per folder). A missing folder asks `create <path>? enter yes / esc no`: one
  level, never on a network path; only Enter answers it, a click or a cursor move drops it. In
  preview mode the yes creates nothing and says `preview - nothing was created`.
- Project search: space-separated words must all match; a pasted path matches as typed. Ranked
  name starts with a word, then name contains it, then path; hidden projects last; matches
  underlined. Enter in the open filter runs the highlighted row and the arrows move while typing;
  with no match Enter on the current directory says `no project matches`.
- Rows editor (maintenance `l`): a click selects a row, a second click on a hidden row's value
  steps it; footer buttons `[^w] up`, `[^s] down`, `[^x] show/hide`, `[enter] save`, `[esc] cancel`;
  the a/d hint is a line in the body.
- Held-session warning: clickable `[enter] go on` and `[esc] back` buttons.
- README: Requirements, install steps with a hash check, row values, and
  `docs/setup-with-claude-code.md`, a guide to hand to your own Claude Code.

### Changed

- A click selects a row and a second click on that row runs it; a row the keyboard or the wheel
  moved to never runs on one click (project screen, session picker, path navigator).
- Project screen Esc steps back one level: open filter, kept filter, hidden view, then the launch
  screen. The footer follows the state: the filter shows `up/down move · left/right action ·
  enter run · esc clear`, the hidden view `v back`, a kept filter `esc clear`, the navigator its
  own keys. The `w/s move` and `a/d action` hints stay at 78 to 120 columns.
- Choosing a project never hangs on an offline network share; a file path is refused.
- Release builds link the C runtime statically: no VC++ redistributable needed.
- Release notes come from the version's CHANGELOG section.

### Fixed

- The config writer (rows editor, `h`) refuses a config whose file attributes cannot be read; it
  could replace such a file with one holding only the saved key.

## [0.7.3] — 2026-11-11

### Added

- Config `hiddenValues`: the value a hidden row launches with (`{"effort": "high"}`), the same
  claude arguments as picking it on a shown row; unset rows keep their default. The rows editor
  (maintenance `l`) sets it with `a`/`d` on a hidden row, which reads `hidden -> <label>`
  (`claude default` when unset), and saves it with `rows`, dropping entries of shown rows. `model`
  takes the row keys (`haiku`, `sonnet1m`, `opus1m`, `fable`), `remote` only `on` or `off`. A bad
  row or value warns and is skipped.

### Fixed

- Rows editor: with config `remote` off the remote entry no longer claims `shown`; it reads
  `off in config (remote: false)`, dimmed, takes no key, and keeps its place in `rows`.
- A row hidden mid-run no longer takes a value back from another account tab's stash on a tab
  switch; it launches with its `hiddenValues` entry or its default.

## [0.7.2] — 2026-11-11

### Changed

- Row editing moved from the launch screen to the maintenance rows editor (`l`): `ctrl+w`/`ctrl+s`
  (or `alt+up`/`alt+down`) move the row, `ctrl+x` (or `space`) hides it, `enter` saves, `esc`
  cancels; a hidden row stays in place while editing, dimmed. Shift+`w`/`s` no longer move (a split mouse report
  arrives as shift+letter). The launch screen drops `ctrl+w`/`ctrl+s`/`ctrl+x` and `o`; its footer
  is back to five hints.

## [0.7.1] — 2026-11-10

### Added

- Launch screen: `ctrl+w`/`ctrl+s` (or `alt+up`/`alt+down`) move the selected row, `ctrl+x` hides it (not `account`), `o`
  opens the rows editor, where hidden rows can be shown again. Each change saves `rows` at once;
  a refused save holds for this run and says why. The footer names the keys.

### Changed

- A `rows` save from the rows editor (`o`, or maintenance `l`) shows on return to the launch
  screen, no longer from the next start.
- `ctrl+s` reaches the launcher on Windows: the armed console drops line and echo input, so
  conhost no longer takes it as pause and swallows it with the next key.
- `ctrl+x` on the action row (`projectScreen: "onDemand"`) no longer writes `rows`.
- A control key psmux sends as a bare character (`ctrl+w` as U+0017) arrives as that ctrl key.

## [0.7.0] — 2026-11-10

### Added

- Config `rows`: which launch rows show and in what order; a hidden row launches with its default,
  and a hidden model row still pins Opus on an account without the default model's family. The
  maintenance screen's `l` edits it in place, so `l` is no longer free for `maintenanceActions`.
- The launcher's own config writes (`rows`, `hiddenProjects`) keep every other key, refuse a file
  that is not UTF-8 JSON, and go through a symlinked config to its target.
- Config `modelOrder`: the model row's families and their order; unlisted families are hidden.
- Config `hiddenProjects` and the project screen's `h` (hide or show the project under the
  cursor, saved at once; only that entry changes, and a failed save is named on screen) and `v`
  (list the hidden projects, marked).
- Config `projectScreen: "onDemand"`: Enter starts in the launch directory; the launch screen's
  new `action` row picks new, project, continue, resume or worktree, and `project` opens the
  project screen for that launch.
- Config `mouse: false` turns mouse input off (`CLAUDE_AUTO_NO_MOUSE=1` still wins).

### Changed

- The model row lists Haiku, Sonnet, Opus, Fable (weakest to strongest); the static Fable label is
  Fable 5.2.
- The usage line keeps the Fable weekly bucket from a widget export older than 30 minutes, with
  that export's age.
- Config `hideHeadlessProjects` (default `true`): a project only headless sessions ran in is hidden
  even when it is a git root.

## [0.6.1] — 2026-11-08

### Fixed

- A `vsec.cmd` shim as the vsec path (`vsecPath`, `CLAUDE_AUTO_VSEC` or PATH) is resolved to the
  `vsec.exe` it names and that exe is spawned directly, so a vsec redeploy no longer breaks session
  secrets and quoted arguments survive. A shim naming anything outside its folder is refused.
- The "secrets loaded" line prints once.

## [0.6.0] — 2026-11-08

The launcher is one Rust binary, `claude-auto.exe`. The PowerShell launcher is gone.

### Added

- The launcher rewritten in Rust (`cargo build --release --locked`, or `claude-auto.exe` from the
  release page). Same screens, config, prefs, project and model caches. The multi-account MCP
  mirror runs inside it, so a launch needs neither PowerShell nor Node.
- Secrets from a `vsec` secrets tool: config `vsecPath`, else env `CLAUDE_AUTO_VSEC`, else
  `vsec.exe` or `vsec.cmd` on PATH. The launcher runs `vsec launch-env --cwd <project>` windowless
  and sets the variables it returns on the session. vsec not found: the `secretsRoot` file walk
  runs as before, with one gray line. vsec found but failing: no secrets and one yellow line, never
  a mix of both sources.
- Debug mode (`--launcher-debug` or `CLAUDE_AUTO_DEBUG=1`): nothing on screen (a timing footer only
  with `CLAUDE_AUTO_DEBUG=footer`), F12 for the timings of the run, `timing` records in the run log,
  and `claude-auto --launcher-timings [--days N]` for count, p50, p95 and max per span.
- CI: a Windows job for the crate (`cargo fmt --check`, `clippy -D warnings`, `cargo test`, all
  `--locked`).

### Changed

- The binary is `claude-auto.exe` (was `claude-auto-rs.exe` while under test). Rename the old file
  on PATH, or delete it after copying the new one. Cache and temp file names keep their old prefix.
- Secrets: Claude Code's own auth variables (`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`,
  `CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_BASE_URL`) and the secrets store's access token
  (`BWS_ACCESS_TOKEN`) never reach the session from either source.
  The vsec path exports exactly vsec's variables, no derived `<NAME>_FILE`.
- Launch hooks run at the same time, not one after another. The session still starts only after
  every hook has ended, and their lines print in config order.
- `launchHooks`: an entry may be an array of paths, run in order as one chain beside the other
  entries (`["a.ps1", ["b.ps1", "c.ps1"]]`). A plain string is one hook, as before.
- Launch hooks get the console's stdin. The companion server, its build and the pairing QR start
  with the chosen account's environment (`CLAUDE_CONFIG_DIR`, project secrets), as the session
  does. An offline launch passes the release-channel check silently; any other failed request
  prints `update check skipped: <reason>` (a remembered failure stays silent for five minutes).
- Project screen: scratch directories are no longer listed as projects. A directory under the temp
  folder or under a profile's `jobs\<id>\tmp` stays known but hidden: the title reads
  `10 known ─ 19 hidden`, and a filter (`/`) that matches a hidden project lists it after the
  visible matches. The current directory is always shown.
- Project screen: headless sessions (`claude -p`, the SDKs) no longer date a project. A folder only
  headless runs used is hidden like a scratch directory, unless it is a repository root. The
  session picker leaves headless sessions out, counts them in its title (`N headless hidden`), and
  loads further pages until it has a page of resumable sessions or none are left.
- Launch screen: on an account whose widget `availableModels` lacks the family of `settings.json`'s
  model, `default` launches with `--model opus[1m]` and the row names that option; the advisor row
  does the same with `--advisor opus`. An account that has the model, or one the widget never
  reported (or reported over 30 minutes ago), still passes no `--model`.
- Session picker: with `sharing` on, one session list for every account. With `sharing` off each
  account still lists its own sessions.

### Removed

- The PowerShell launcher (`claude-auto.ps1`, `claude-auto/`, `claude-auto.cmd`, `install.ps1`),
  its tests, the Node MCP mirror (`sharing/claude-mirror-mcp.mjs`) and the PowerShell CI job. The
  last commit with the PowerShell code is `7348f59`.
- `tests/check-clean.ps1`: the privacy scan is `cargo run --example privacy_scan` now (same
  contract, case-insensitive, a hit names the pattern's line number instead of its text).

### Fixed

- Launch: the session carries `CLAUDE_AUTO_LAUNCHED=1`, on every account, so a SessionStart hook
  can tell a launcher start from one by hand. It is set only around the child start, so a later
  `claude` typed in the same terminal does not carry it.
- Multi-account sharing: every secondary account's root gets `CLAUDE.md` as a file symlink to the
  canonical one, repaired at each launch; a session under `CLAUDE_CONFIG_DIR` outside the home tree
  ran with no user instructions before. A copy equal to the canonical file is saved as
  `CLAUDE.md.pre-symlink.<yyyyMMdd-HHmmss>` and replaced, a diverged one is left and named, and
  without the symlink privilege one warning line is printed and the launch goes on.
- Project screen: no network path (`\\host\share`, `\\?\UNC\...`, a mapped network drive) is probed
  any more, so an unreachable share cannot hold the first screen or a tab switch; such a row is
  listed unchecked and refused at launch if it cannot be entered. A missing drive letter is skipped.

### Known limits

- A transcript read as headless checks only its first entrypoint and its last 64 KB: a run resumed
  interactively and then followed by more than 64 KB of headless records still reads as headless.
- Whether a headless-only folder is a repository root is cached until a new transcript lands in
  it: a `.git` created or removed in between shows on the next session there, not at once.

## [0.5.0] — 2026-10-02

### Added

- Two switches for the MCP mirror that `sharing` runs: `CLAUDE_AUTO_MIRROR_EXCLUDE` names servers
  never to copy, and `CLAUDE_AUTO_MIRROR_SECRETS=0` copies servers without their `env` and
  `headers`. No behaviour change for existing users: a verbatim copy, secrets included, stays the
  default.

### Changed

- Project screen: when the current directory is a known project, the top row names it
  (`<project> (current directory)`, with its age) and the project is no longer listed a second time
  below. The cursor always opens on that top row instead of on the last launched project.
- Model row: Sonnet is labelled `Sonnet 5.5` with no `[1M]` marker (its 1M context is native), and
  a saved `sonnet[1m]` default reads `default (Sonnet 5.5)`.

### Fixed

- Each launcher instance writes the companion server's log to its own
  `%TEMP%\claude-remote-server-<pid>.log`, so two instances no longer share one file.
- The session picker's title shows a typed or pasted filter cleaned of control characters, as the
  project screen already did: an escape sequence in the filter is displayed, never sent to the
  terminal.

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

[Unreleased]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.9.1...HEAD
[0.9.1]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.8.2...v0.9.0
[0.8.2]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.8.1...v0.8.2
[0.8.1]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.7.3...v0.8.0
[0.7.3]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.7.2...v0.7.3
[0.7.2]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.4.2...v0.5.0
[0.4.2]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/vitaliy-shatskiy/claude-auto-launcher/releases/tag/v0.1.0
