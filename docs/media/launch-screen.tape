# The launch screen of the Rust launcher, captured in preview mode on the four-account test fixture.
# Run from the repository root after `cargo build`:  vhs docs/media/launch-screen.tape
# Nothing is launched: CLAUDE_AUTO_PREVIEW=1, a throwaway USERPROFILE and prefs file, one Escape key.

Output target/vhs/launch-screen.gif
Set Shell "pwsh"
Set FontFamily "Cascadia Mono"
Set FontSize 16
Set Width 1080
Set Height 370
Set Padding 20
Set Theme "Builtin Dark"
Set TypingSpeed 40ms

Hide
Type "$t = Join-Path ([IO.Path]::GetTempPath()) 'claude-auto-demo'; New-Item -ItemType Directory -Force (Join-Path $t '.claude') | Out-Null; $env:USERPROFILE = $t; $env:CLAUDE_AUTO_PREFS = Join-Path $t 'prefs.json'; $env:CLAUDE_AUTO_PROJECTS_ROOT = Join-Path $t 'projects'; $env:CLAUDE_AUTO_CONFIG = (Resolve-Path 'tests/fixtures/config-four.json').Path; $env:CLAUDE_AUTO_PREVIEW = '1'; $env:CLAUDE_AUTO_PREVIEW_SIZE = '100x16'; $env:CLAUDE_AUTO_PREVIEW_KEYS = 'Escape'; $env:CLAUDE_NO_ROAM = '1'; Remove-Item Env:NO_COLOR, Env:CLAUDE_CONFIG_DIR -ErrorAction SilentlyContinue; $d = $env:CARGO_TARGET_DIR ?? 'target'; $x = @('release', 'debug') | ForEach-Object { Join-Path $d $_ 'claude-auto-rs.exe' } | Where-Object { Test-Path $_ } | Select-Object -First 1; function claude-auto-rs { & $x @args }; function prompt { '> ' }; Clear-Host"
Enter
Sleep 2s
Show

Type "claude-auto-rs"
Sleep 300ms
Enter
Sleep 2s
Screenshot docs/media/launch-screen.png
Sleep 1s
