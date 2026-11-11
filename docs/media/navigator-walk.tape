# The path navigator: enter a path... on the project screen lists the typed parent's folders,
# a letter filters them, tab completes, a missing folder asks before it is created, esc steps back.
# The first frame (the poster GitHub shows) is the navigator listing the demo drive.
# Run from the repository root after `cargo build --release`:  vhs docs/media/navigator-walk.tape
# Nothing is launched and nothing is created: every step is a preview run (CLAUDE_AUTO_PREVIEW=1)
# whose scripted keys end on the screen shown (the create question is answered esc), drawn on the
# demo home and the invented projects of demo-setup.ps1.

Output docs/media/navigator-walk.gif
Output docs/media/navigator-walk.mp4
Set Shell "pwsh"
Set FontFamily "Cascadia Mono"
Set FontSize 22
Set Width 1472
Set Height 640
Set Padding 24
Set Margin 40
Set MarginFill "target/vhs/backdrop-navigator-walk.png"
Set WindowBar Colorful
Set WindowBarSize 44
Set BorderRadius 10
Set Theme "GitHub Dark"
Set Framerate 24
Set TypingSpeed 10ms

# A preview run prints one frame per key. Each step runs the key script one key short as well and
# prints only the lines past it: the last frame alone. $P opens the navigator on the demo drive
# root, $Q is $P one key short.
Hide
Type ". ./docs/media/demo-setup.ps1; $env:CLAUDE_AUTO_PREVIEW = '1'; $env:CLAUDE_AUTO_PREVIEW_SIZE = '100x18'; function global:prompt { [char]27 + '[?25l' }; function step($k, $p) { $n = 0; if ($null -ne $p) { $env:CLAUDE_AUTO_PREVIEW_KEYS = $p; $n = @(claude-auto 2>$null).Count }; $env:CLAUDE_AUTO_PREVIEW_KEYS = $k; $all = @(claude-auto 2>$null); Clear-Host; $all | Select-Object -Skip $n | ForEach-Object { [Console]::WriteLine($_) } }; $Q = 'Enter,s,s,s,s,Enter,' + $env:CLAUDE_AUTO_DEMO_DRIVE[0] + ',:'; $P = $Q + ',' + [char]92; Set-Location $env:CLAUDE_AUTO_DEMO_DRIVE\; Clear-Host"
Enter
Sleep 3s

# The demo drive's folders.
Type "step $P $Q"
Enter
Sleep 2s
Show
Sleep 2.5s

# h filters them, tab completes harbor-api.
Hide
Sleep 200ms
Type "step ($P + ',h') $P"
Enter
Sleep 1.5s
Show
Sleep 2s
Hide
Sleep 200ms
Type "step ($P + ',h,Tab') ($P + ',h')"
Enter
Sleep 1.5s
Show
Sleep 2s

# A folder that does not exist yet: enter asks before creating it, esc says no.
Hide
Sleep 200ms
Type "step ($P + ',h,Tab,d,o,c,s') ($P + ',h,Tab,d,o,c')"
Enter
Sleep 1.5s
Show
Sleep 1.5s
Hide
Sleep 200ms
Type "step ($P + ',h,Tab,d,o,c,s,Enter') ($P + ',h,Tab,d,o,c,s')"
Enter
Sleep 1.5s
Show
Sleep 2.5s
Hide
Sleep 200ms
Type "step ($P + ',h,Tab,d,o,c,s,Enter,Escape') ($P + ',h,Tab,d,o,c,s,Enter')"
Enter
Sleep 1.5s
Show
Sleep 1.5s

# esc: back to the project list, on the enter a path... row.
Hide
Sleep 200ms
Type "step ($P + ',h,Tab,d,o,c,s,Enter,Escape,Escape') ($P + ',h,Tab,d,o,c,s,Enter,Escape')"
Enter
Sleep 1.5s
Show
Sleep 2s
