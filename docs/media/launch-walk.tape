# A keyboard walk over the launch page: switch accounts (each brings its remembered choices),
# then step model, effort and advisor; in maintenance's rows editor move a row and hide mode.
# The recording stops on the launch page: nothing is launched.
# Run from the repository root after `cargo build --release`:  vhs docs/media/launch-walk.tape
# The demo home from demo-setup.ps1 is a throwaway folder under %TEMP%: no real config, prefs or
# projects are read, and the run's own writes (logs, caches) land there.
# 122 columns: the workspace (launch, projects, the right column) shows from 120.

Output docs/media/launch-walk.gif
Output docs/media/launch-walk.mp4
Set Shell "pwsh"
Set FontFamily "Cascadia Mono"
Set FontSize 20
Set Width 1502
Set Height 660
Set Padding 2
Set Margin 0
Set WindowBar Colorful
Set WindowBarSize 40
Set BorderRadius 0
Set Theme "GitHub Dark"
Set Framerate 24
Set TypingSpeed 10ms

# The typing stays hidden, so the first frame (the poster GitHub shows) is the launch page.
Hide
Type ". ./docs/media/demo-setup.ps1; Set-Location $env:CLAUDE_AUTO_DEMO_DRIVE\; Clear-Host; claude-auto"
Enter
Sleep 4s
Show
Sleep 1.5s

# Accounts: each tab restores its own model, effort, advisor and permission, and its limits.
Right
Sleep 1.2s
Right
Sleep 1.2s
Left
Sleep 600ms
Left
Sleep 1.2s

# Rows: model, effort, advisor.
Down
Sleep 600ms
Right
Sleep 900ms
Left
Sleep 600ms
Down
Sleep 600ms
Right
Sleep 900ms
Left
Sleep 600ms
Down
Sleep 600ms
Right
Sleep 900ms
Left
Sleep 1.2s

# Row editing lives in maintenance: u, then l opens the rows editor; ctrl+w moves advisor above
# effort, ctrl+x hides mode (it stays in place, dimmed), enter saves, esc back to the launch page.
Type "u"
Sleep 1.5s
Type "l"
Sleep 1s
Type "ssss"
Sleep 600ms
Ctrl+W
Sleep 1.2s
Type "sss"
Sleep 600ms
Ctrl+X
Sleep 1.2s
Enter
Sleep 1.5s
Escape
Sleep 2.5s

# Quit off camera: the last frame stays the launch page.
Hide
Escape
Sleep 1s
