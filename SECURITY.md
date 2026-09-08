# Security

## Reporting

Report privately through [GitHub Security Advisories](https://github.com/vitaliy-shatskiy/claude-auto-launcher/security/advisories/new).
Please do not open a public issue for anything exploitable. Expect a first reply within a week.

## What this tool touches on your machine

Worth knowing before you run it, and the places a vulnerability would matter:

- **PATH.** `install.ps1` appends this folder to `HKCU\Environment`, read and written raw so an
  existing `%VAR%` entry is not expanded and rewritten.
- **`~/.claude/`.** Reads `settings.json`, writes `claude-auto-prefs.json`, `launcher-logs/*.jsonl`
  (14-day retention), and two caches. The logs record the launcher's own argument vector and working
  directory — a prompt passed positionally lands there in plain text.
- **Environment variables from files.** With `secretsRoot` configured, every file under it becomes an
  environment variable in the launched session. Anything readable there reaches the child process.
- **Your Claude Code install.** The maintenance screen updates, renames and prunes binaries under
  the real install directory.
- **Other account roots.** With `sharing: true` it hardlinks and junctions directories between
  account roots and applies `icacls` deny ACEs. Read that section of the README first.

## Not in scope

The `claude` CLI itself, the Claude Code service, and the unpublished `crc` companion used by the
optional `remote` feature.
