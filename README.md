# claude-auto-launcher

[![tests](https://github.com/vitaliy-shatskiy/claude-auto-launcher/actions/workflows/ci.yml/badge.svg)](https://github.com/vitaliy-shatskiy/claude-auto-launcher/actions/workflows/ci.yml)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A Windows PowerShell launcher for Claude Code: an account picker, remembered model/effort/advisor/
permission choices per account, a session-resume picker, and a maintenance screen, all driven by one
config file so nothing here is hardcoded to a particular machine.

Unofficial and unaffiliated with Anthropic. It runs the `claude` CLI you already have; bugs in the
CLI itself belong [upstream](https://github.com/anthropics/claude-code/issues).

```
╭────────────────────────────────────────────────────────────────────────────╮
│ ✻ claude-auto                                                              │
╰────────────────────────────────────────────────────────────────────────────╯

 ❯ account     [work]
   action      ● [new] ○ continue ○ resume ○ worktree
   model       ‹ default (Sonnet 5) ›
   effort      ● [default] ○ low ○ medium ○ high ○ xhigh ○ max ○ ultracode
   advisor     ● [default (none)] ○ fable ○ opus ○ off
   permission  ● [default] ○ plan ○ auto ○ acceptEdits ○ bypass
   mode        ● [normal] ○ safe

  ──────────────────────────────────────────────────────────────────────────
  up/down row  ┊  left/right value  ┊  enter start  ┊  u maintenance
  esc quit
```

The `default (…)` labels above resolve from the reader's own `~/.claude/settings.json` and differ per machine.

## Requirements

- PowerShell 7 (`pwsh`) - without it the launcher re-execs under `pwsh` if it can find one, else warns and continues with the full UI but no MCP configuration (a bare session is a separate, module-load failure path)
- Claude Code (`claude` on PATH) - the launch-screen rows (model/effort/advisor/permission values) were built against `2.1.263`; a much older or newer CLI may accept different flags
- Node.js - only for two things: the MCP mirror used by multi-account `sharing`, and `Test-Mirror.ps1`, which exits `2` (could not run) without it. The launcher itself does not need it

## Install

```
git clone https://github.com/vitaliy-shatskiy/claude-auto-launcher.git
cd claude-auto-launcher
pwsh -File install.ps1
```

`install.ps1` is safe to run again: it never overwrites `~/.claude/claude-auto.json` once it exists,
and only adds this folder to the user PATH if it is missing there. It reads and writes
`HKCU\Environment` raw, so an existing `%USERPROFILE%\...` entry keeps its `%VAR%` form; anything it
cannot read leaves PATH untouched and tells you to add the folder by hand.

On a machine whose execution policy is Restricted — the Windows default on a client SKU with no
policy set in any scope — `pwsh -File install.ps1` refuses to run. Either
`Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` once, or run it as
`pwsh -ExecutionPolicy Bypass -File install.ps1`.

## Uninstall

Nothing is installed outside your own user profile, and there is no uninstaller.

1. Remove this folder from PATH: `sysdm.cpl` → Advanced → Environment Variables → user `Path`.
2. Delete the clone.
3. Optional, if you want the state gone too: `~/.claude/claude-auto.json`,
   `claude-auto-prefs.json`, `claude-auto-hash-cache.json`, `claude-auto-sessions.json`,
   `launcher-logs/` and `claude-auto-logs/` (full list under **Files it writes**).
4. **Only if you ever set `sharing: true`**: the junctions it created under each secondary account
   root carry two `icacls` deny ACEs, so a plain delete fails. Lift them first —
   `icacls <path> /L /remove:d "<user>"` — then remove the junction.

## Configuration

`~/.claude/claude-auto.json` (or `CLAUDE_AUTO_CONFIG`). No file = one account, `work` -> `~/.claude`,
sharing and remote off. Every validation failure falls back to the default for that key and prints a warning at launch; nothing throws. An unrecognized top-level key (a typo, e.g. `account` for `accounts`) also warns, naming it. Windows path values must use `~/...` or forward slashes, or double every backslash (`C:\\Users\\me\\.claude`) - JSON reads a single backslash as an escape character, and an unescaped one fails the whole file with no other hint.

| key | type | default | notes |
|---|---|---|---|
| `accounts[]` | `{key, root, label, tint, hidden}` | one `work` account | key 1-8 chars, unique, and unique first letters; root absolute, exactly one must resolve to `~/.claude` (the canonical account); tint one of Green/Magenta/Cyan/Blue/Yellow/Red; hidden omits it from the tab strip and the no-UI prompt's text, though it stays typeable there. Breaking any one of these rules discards the WHOLE roster, not just the offending account - the warning names the culprit and its value, but every account falls back to the single default `work` account |
| `sharing` | bool | `false` | forced off with fewer than two accounts; see Multi-account sharing below before turning it on |
| `remote` | bool | `false` | **the companion it needs is not published** — `crc.cmd` on PATH plus a `remote-control-claude-code` checkout (`CLAUDE_REMOTE_ROOT`), neither of which you can obtain, so leave this off unless you have written your own. Adds a Remote row to the launch screen and a second prompt in the no-UI fallback |
| `riderMcp` | `auto`\|`on`\|`off` | `auto` | `auto` scans for Rider's MCP port only while Rider is running |
| `extraMcpConfigs[]` | paths | `["~/.claude/mcp-shared.json"]` | must be absolute; a relative entry is skipped with a warning |
| `secretsRoot` | path | `~/.claude-secrets` | must be absolute; see Secrets below |
| `launchHooks[]` | paths | `[]` | `.ps1`/`.js`/`.mjs`/`.cmd`, run after the account choice, each best-effort; must be absolute |
| `maintenanceActions[]` | `{key, label, script, confirmTwice}` | `[]` | key is one character, a-z or 0-9, outside `u r d m p`; `script` is required, must be absolute, and an entry failing either check is skipped with a warning |

See `config.example.json` for a two-account example (sharing off; `launchHooks`/`maintenanceActions`
are empty there so a fresh clone never warns about a script nobody has - example values:
`"launchHooks": ["~/scripts/after-launch.ps1"]`,
`"maintenanceActions": [{ "key": "i", "label": "reindex", "script": "~/scripts/reindex.ps1", "confirmTwice": true }]`).

### Secrets

`secretsRoot` is a directory of per-project subdirectories whose FILES become environment variables (file name = variable name, content = value), so a token never sits in a config file. `.md` files and empty files are skipped. Three tiers apply in order, each later one overriding the earlier: `shared/`, `<slug>/org/` (opted in with a junction, never automatic), then `<slug>/` for the current working directory.

## What each screen does

- **Launch screen** - rows for account (a tab strip carrying each account's five-hour usage %), action (`worktree` starts the session in a new git worktree, `-w`), model, effort, advisor, permission and mode (`safe` disables CLAUDE.md, skills, plugins, hooks and MCP for that session). With `remote: true` a Remote row appears too. Arrows move and change, enter starts, `u` opens maintenance, esc quits.
- **Maintenance** (`u`) - `u` update, `r` rename swap, `d` doctor, `m` mcp list, `p` prune, plus one hotkey per configured `maintenanceActions[]` entry, `esc` back. **Not covered by `CLAUDE_AUTO_PREVIEW`** - unlike every other screen, its actions run against your real Claude Code install even during a preview run; `tests\preview.ps1` never presses one of these keys.
- **Session picker** (action = resume) - a list with a last-exchange preview; `/` filters, `enter` opens, `f` forks, `esc` returns to the launch screen.

## Environment switches

- `CLAUDE_AUTO_CONFIG` - path to the config file, instead of `~/.claude/claude-auto.json`
- `CLAUDE_AUTO_PREFS` - path to the remembered-choices file, instead of `~/.claude/claude-auto-prefs.json` (the test harness points this at a throwaway file so driving the preview seam never touches the real one)
- `CLAUDE_AUTO_NO_MOUSE=1` - never arms mouse input
- `CLAUDE_AUTO_INPUT_TRACE=1` - logs every raw input record to `~/.claude/claude-auto-logs/input-<date>-<pid>.log`
- `CLAUDE_AUTO_ASCII=1` - forces ASCII box-drawing (otherwise auto-detected from the console code page)
- `CLAUDE_AUTO_PREVIEW=1` - drives the whole UI and prints the `claude` command it would run, without running it. What the test suite uses. The maintenance screen is the one exception: its actions hit your real install even here
- `CLAUDE_AUTO_PREVIEW_KEYS` - a key script fed to a preview run, so the seam can be driven headlessly
- `CLAUDE_NO_ROAM=1` - never routes a session through the companion server, even with `remote: true`
- `CLAUDE_REMOTE_ROOT` - path to the (unpublished) `remote-control-claude-code` checkout
- `NO_COLOR` / `TERM=dumb` - disable colour output

## Files it writes

- `~/.claude/claude-auto-prefs.json` - remembered account, model/effort/advisor/permission/remote
- `~/.claude/launcher-logs/*.jsonl` - one file per day, 14-day retention. Records each launch: the working directory, the launcher's own argument vector, the PowerShell version, which terminal it came out of, and the choices made. **A prompt passed positionally (`claude-auto "fix the build"`) lands there in plain text.** Local only — nothing is sent anywhere
- `~/.claude/claude-auto-hash-cache.json` - the maintenance screen's version-check cache
- `~/.claude/claude-auto-sessions.json` - the session picker's summary cache (one per account root)
- `%TEMP%\claude-mcp-rider-<pid>.json` - per-launch Rider MCP config, swept after a day
- `%TEMP%\claude-rider-mcp-port.txt` - cached Rider MCP port, so most launches skip the port scan
- `claude-auto\ConsoleInput.dll` - compiled from `ConsoleInput.cs` into this clone on first run (gitignored)
- `<file>.pre-relink` beside a shared file a sharing repair just replaced (see Multi-account sharing)

The launch screen's usage bars need a `~/.claude/rate-limits/<key>.json` writer; nothing in this repo writes that file.

## Tests

```
pwsh -File tests\checkpoint.ps1
```

15 checks: 11 unit suites (`Theme`, `Layout`, `Sessions`, `Ui`, `Maintenance`, `Prefs`, `Env`,
`Config`, `Input`, `Install`, `Mirror`), a privacy scan, and three launcher-level checks. Every exit
code is read on its own line, and **`2` means the check could not run — never a pass**.

Any suite runs alone: `pwsh -File tests\Test-Ui.ps1`. `Test-Input.ps1 -Live` adds the console-mode
and mouse assertions, which self-spawn a hidden child console; the checkpoint always passes `-Live`.
`Test-Mirror.ps1` needs Node.js and exits `2` without it, so the checkpoint can never be green on a
machine with no `node`.

`check-clean.ps1` is the privacy scan: it greps the git-tracked worktree for the current Windows
username plus an optional private pattern list (`CLAUDE_AUTO_CLEAN_PATTERNS`, one regex per line).
It never reads commit history or commit authorship.

On a fresh clone `tests\reference-output.local.txt` does not exist yet (it is gitignored), so the
checkpoint reports BOTH `regression 2 DID NOT RUN` and `droplist 2 DID NOT RUN` - correctly, not a
pass - until you record one (droplist reads the same reference file, so recording it fixes both rows):

```
pwsh -File tests\check-regression.ps1 -Record
```

`check-preview` similarly needs its own recorded reference (`tests\preview-reference.local.txt`,
also gitignored) before it reports anything but `2 DID NOT RUN`:

```
pwsh -File tests\check-preview.ps1 -Record
```

## Multi-account sharing

`sharing: true` (two or more accounts; OFF in `config.example.json` on purpose) hardlinks `settings.json` and `statusline.js`, and junctions `projects`, `plugins`, `hooks`, `agents`, `skills`, `rules`, `sessions`, `file-history`, `session-env`, `tasks` and `shell-snapshots` from every secondary account's root back to the canonical (`~/.claude`) one, repaired on every launch. The repair links from whichever copy is NEWEST anywhere, so a newer secondary copy can REPLACE the canonical one - the losing copy survives as `<file>.pre-relink`. Enabling it also CREATES every secondary account's root directory on the next launch, and each junction it creates gets two `icacls` deny ACEs, so removing one later needs `icacls <path> /L /remove:d "<user>"` before a plain delete will work. Turn it on only once you accept those effects under directories the launcher itself now owns and ACL-locks.

The MCP mirror that `sharing` runs copies server definitions verbatim between accounts, so a server
whose `env` block holds an API token carries that token into the other account's `.claude.json`.

## Troubleshooting

**`claude-auto` is not recognised.** `install.ps1` writes PATH into `HKCU\Environment`, which only a
*new* terminal reads. Open one.

**The maintenance screen says an update succeeded and the version did not change.** On Windows
`claude update` can report success while the running binary is untouched — usually a file lock on the
in-use executable. That is what `r` (rename swap) is for: it renames the running binary aside and
installs the new build in its place.

**The whole roster collapsed to one `work` account.** Any single invalid account discards the entire
roster by design; the warning at launch names the culprit and its value. The usual cause is a
Windows path in JSON: `"C:\Users\me\.claude"` is invalid JSON escaping. Write `~/...`, forward
slashes, or `C:\\Users\\me\\.claude`.

**The box drawing is mojibake.** Set `CLAUDE_AUTO_ASCII=1`, or switch the console to UTF-8.

**Arrows or the mouse behave oddly in an embedded terminal.** Some terminals (Rider's, for one) send
mouse reports as VT text rather than as console records; the launcher decodes them, but if it goes
wrong, `CLAUDE_AUTO_NO_MOUSE=1` disables mouse input entirely and
`CLAUDE_AUTO_INPUT_TRACE=1` logs every raw input record for a bug report.

**It launched a bare `claude` with none of the screens.** A module failed to load. Run
`pwsh -File claude-auto.ps1` directly and read the error it prints before the fallback.

## Contributing, security, licence

- [CONTRIBUTING.md](CONTRIBUTING.md) — how to run the suite and what a PR needs.
- [SECURITY.md](SECURITY.md) — what this touches on your machine, and how to report a hole privately.
- [CHANGELOG.md](CHANGELOG.md).
- [MIT](LICENSE).
