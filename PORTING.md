# PORTING - history

The launcher was first written in PowerShell, then ported to Rust module by module. The PowerShell
launcher was removed once the Rust one replaced it. Its last commit is `7348f59`: there,
`claude-auto.ps1`, `claude-auto/*.ps1` and `tests/Test-*.ps1` are the code and tests that the
`(Env.ps1)`-style references in `src/` comments name, and this file holds the full module map and
the list of behaviour differences.

`git show 7348f59:PORTING.md` prints the porting record.
