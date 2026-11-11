# A keyboard walk over the launch screen: switch accounts (each brings its remembered choices),
# then step model, effort and advisor; in maintenance's rows editor move a row and hide mode.
# Esc quits before anything is launched.
# Run from the repository root after `cargo build --release`:  vhs docs/media/launch-walk.tape
# The demo home from demo-setup.ps1 is a throwaway folder under %TEMP%: no real config, prefs or
# projects are read, and the run's own writes (logs, caches) land there.

Output docs/media/launch-walk.gif
Output docs/media/launch-walk.mp4
Set Shell "pwsh"
Set FontFamily "Cascadia Mono"
Set FontSize 22
Set Width 1472
Set Height 592
Set Padding 24
Set Margin 40
Set MarginFill "target/vhs/backdrop-launch-walk.png"
Set WindowBar Colorful
Set WindowBarSize 44
Set BorderRadius 10
Set Theme "GitHub Dark"
Set Framerate 24
Set TypingSpeed 60ms

# The typing stays hidden, so the first frame (the poster GitHub shows) is the launch screen.
Hide
Type ". ./docs/media/demo-setup.ps1; Clear-Host; claude-auto"
Enter
Sleep 3s
Show
Sleep 1.5s

# Accounts: each tab restores its own model, effort, advisor and permission.
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
# effort, ctrl+x hides mode (it stays in place, dimmed), enter saves, esc back to the launch screen.
Type "u"
Sleep 1.2s
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
Sleep 2s

Escape
Sleep 1s
