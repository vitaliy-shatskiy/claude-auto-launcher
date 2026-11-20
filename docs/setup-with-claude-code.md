# Set up claude-auto with Claude Code

Hand this file to your own Claude Code and say "set up claude-auto for me". It is written for
that agent: check, install, ask, write the config, validate, hand over. Every key and rule it
names is in the [README](../README.md).

## Rules for the agent

- Run only `claude-auto --launcher-version` and preview runs (below). Never a bare `claude-auto`
  from your shell: it is not a console, so it would start `claude` itself.
- Never press `p` (prune) on the maintenance screen and never script it: it deletes old Claude Code
  builds.
- Never touch `~/.claude.json`, `~/.claude/settings.json` or any account's credentials.
- Never overwrite an existing `~/.claude/claude-auto.json`: back it up first and edit a copy.
- Ask the user every question in "Questions" before writing the config. Do not guess an answer.

## 1. Pre-checks

Stop and tell the user if any check fails.

```powershell
[Environment]::Is64BitOperatingSystem                    # True
[Environment]::OSVersion.Version                         # 10.0.x (Windows 10 or 11)
Get-Command claude -All | Select-Object Source           # Claude Code is on PATH
Test-Path "$HOME\.local\bin\claude.exe"                  # True = the native install
Get-Command claude-auto -All -ErrorAction SilentlyContinue | Select-Object Source
Test-Path "$HOME\.claude\claude-auto.json"               # an existing config
```

- No `claude` on PATH: install Claude Code first.
- No native install (an npm one): the launcher runs, but the Claude Code update, rename swap,
  prune and model names need the native install. Tell the user.
- An older `claude-auto` on PATH (a `.ps1`/`.cmd` in a clone folder from 0.5.0 or older): ask
  whether to remove that folder from the user PATH, or to put the new exe in that folder.
- An existing config: copy it to `claude-auto.json.bak-<date>` beside it and say so.

## 2. Install

Run the script in the README's [Install](../README.md#install) section, then check the exe:

```powershell
& "$env:LOCALAPPDATA\Programs\claude-auto\claude-auto.exe" --launcher-version
```

A hash mismatch is a stop: delete the file and tell the user. With `gh` installed, also check the
build attestation: `gh attestation verify "$env:LOCALAPPDATA\Programs\claude-auto\claude-auto.exe" -R vitaliy-shatskiy/claude-auto-launcher`.

## 3. Questions for the user

Ask these, in one round, with the defaults named:

1. **Accounts.** One per Claude login. For each: a key (1-8 characters, unique ignoring case), a
   label, a tint (Green, Magenta, Cyan, Blue, Yellow, Red), and a root folder. Exactly one account
   has root `~/.claude` (the main one); every other root is its own folder, e.g.
   `~/.claude-second`. One account only = no config needed at all.
2. **Sharing** (default off). On, every secondary account links settings, skills, agents, hooks,
   projects and more to the main one, and MCP servers are mirrored, secrets included. Turn it on
   only after the user has read the README's sharing warning and confirmed. It needs Developer Mode
   on (or one elevated run) and every root on the same drive as `~/.claude`.
3. **Launch rows.** Which rows show, in what order (`rows`: `account`, `model`, `effort`,
   `advisor`, `permission`, `mode`); for a hidden row, the value it launches with (`hiddenValues`,
   values in the README's row table); which model families show and in what order (`modelOrder`).
4. **Project screen.** `projectScreen`: `always` (default) or `onDemand` (Enter launches in the
   current folder). `hideHeadlessProjects` (default true). `mouse` (default true).
5. **Launch scripts** (`launchHooks`, default none). A `.ps1` needs PowerShell 7 (`pwsh`), a
   `.js`/`.mjs` needs Node.js.
6. **Claude Code update.** Before each launch screen the launcher checks Claude Code's release
   channel and runs `claude update` when it is newer. OK, or off (`CLAUDE_AUTO_NO_UPDATE=1` as a
   user environment variable)?
7. **claude-auto self-update** (`selfUpdate`). Once a day the launcher checks its own GitHub
   releases: `notify` (default: a line on the launch screen, maintenance `c` updates), `auto`
   (downloads and swaps in the background, active next start) or `off`?

Keep `remote` false: its companion is not published.

## 4. Write the config

`~/.claude/claude-auto.json`, UTF-8 without BOM. Paths as `~/...` or with forward slashes; a single
backslash breaks the whole file. Example:

```json
{
  "accounts": [
    { "key": "main", "root": "~/.claude", "label": "main account", "tint": "Green" },
    { "key": "second", "root": "~/.claude-second", "label": "second account", "tint": "Magenta" }
  ],
  "sharing": false,
  "rows": ["account", "model", "effort", "permission"],
  "hiddenValues": { "advisor": "off" }
}
```

## 5. Validate with preview

Preview drives the screens from a key list and prints the command it would run. It launches
nothing, runs no update, hook, plugin or sharing repair, and saves nothing but its own projects
cache in `%TEMP%`.

```powershell
$env:CLAUDE_AUTO_PREVIEW = '1'
$env:CLAUDE_AUTO_PREVIEW_KEYS = 'Enter,Enter'
$env:CLAUDE_AUTO_PREVIEW_SIZE = '100x30'
claude-auto
Remove-Item Env:CLAUDE_AUTO_PREVIEW, Env:CLAUDE_AUTO_PREVIEW_KEYS, Env:CLAUDE_AUTO_PREVIEW_SIZE
```

Pass = the output has no line starting with `  config:` and ends in a summary after
`--- preview, nothing launched ---`. Each `  config:` line names a key the launcher dropped: fix it
and run again. `accounts: ... using the default roster` means the whole account list was discarded.

## 6. Hand-off

Tell the user:

- Open a new terminal (the PATH change reaches only new ones) and run `claude-auto`.
- For each secondary account: pick its tab, start a session, type `/login` once.
- Keys: `w`/`s` or arrows move, `a`/`d` or arrows change a value, `enter` next, `u` maintenance,
  `esc` back or quit. Project screen: `enter` run, `c` continue, `r` resume, `t` worktree,
  `/` filter, `p` pin on top, `h` hide, `v` show the hidden ones. Mouse: click selects, a
  second click runs.
- The usage bars stay empty unless a script of theirs writes `~/.claude/rate-limits/<key>.json`.

## 7. Rollback

- Restore the backup: copy `claude-auto.json.bak-<date>` back over `claude-auto.json`, or delete
  `claude-auto.json` if there was none.
- Uninstall: delete `$env:LOCALAPPDATA\Programs\claude-auto` and remove it from the user PATH.
  Optional: the `claude-auto-*.json` files, `launcher-logs\` and `claude-auto-logs\` under
  `~/.claude`.
- Only if sharing was ever on: per junction `icacls <junction> /L /remove:d "<DOMAIN\user>"`, per
  secondary root `icacls <root> /remove:d "<DOMAIN\user>"` (`whoami` prints `<DOMAIN\user>`), then
  remove the junctions.
