# Security

## Reporting

Report privately through [GitHub Security Advisories](https://github.com/vitaliy-shatskiy/claude-auto-launcher/security/advisories/new).
Please do not open a public issue for anything exploitable. Expect a first reply within a week.

## What this tool touches on your machine

Worth knowing before you run it, and the places a vulnerability would matter:

- **`~/.claude/`.** Reads `settings.json`, writes `claude-auto-prefs.json`, `launcher-logs/*.jsonl`
  (14-day retention), and the `claude-auto-*.json` caches (full list in the README, "Files it
  writes"). The logs record the launcher's own argument vector and working directory: a prompt
  passed positionally lands there in plain text.
- **Environment variables from vsec or files.** When a `vsec` tool is found (`vsecPath`,
  `CLAUDE_AUTO_VSEC` or PATH), the launcher runs it and every variable it returns reaches the
  launched session: whatever is first on PATH under that name is trusted. Without vsec, every file
  under `secretsRoot` becomes an environment variable in the launched session. Anything readable
  there reaches the child process, except Claude Code's own auth variables and the store's key,
  which are refused. Values are never printed.
- **Your Claude Code install.** The maintenance screen updates, renames and prunes binaries under
  the real install directory.
- **Its own exe (claude-auto self-update).** Once a day it asks the GitHub releases of `updateRepo`
  (default this repository; a fork can point it at its own) for a newer version. An update
  downloads the exe over https only, checks it against the release's `.sha256`, renames the
  running exe to `claude-auto.exe.old` and puts the download in its place; `b` swaps them back.
  Whoever controls the `updateRepo` releases controls what runs next. `selfUpdate: "off"` or
  `CLAUDE_AUTO_NO_SELF_UPDATE=1` turns it off.
- **Plugins.** Every program listed in config `plugins` runs with the launcher's own
  environment (the session's secrets are not in it) and can add variables and arguments to the session. A plugin may not set the
  account or its credentials (`CLAUDE_CONFIG_DIR`, `ANTHROPIC_*`, `CLAUDE_CODE_OAUTH_TOKEN`,
  `CLAUDE_CODE_USE_*`, `USERPROFILE`, `HOME`). Nothing unlisted runs; a listed path is never
  looked up on PATH.
- **`--launcher-run '<json>'`.** Starts claude from a JSON request, checked against what the launch
  screen would offer that account; anything that can run `claude-auto` can launch a session this
  way.
- **Other account roots.** With `sharing: true` it hardlinks and junctions directories between
  account roots and applies `icacls` deny ACEs. Read that section of the README first.

## Not in scope

The `claude` CLI itself, the Claude Code service, and the unpublished `crc` companion used by the
optional `remote` feature.
