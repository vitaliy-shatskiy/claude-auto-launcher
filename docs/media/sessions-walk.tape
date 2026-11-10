# The session picker: r on a project opens its sessions with a preview of each, the preview
# follows the cursor, tab widens to every project, esc goes back to the project screen and the
# launch screen, enter and s come back to the project. The loop then returns to the picker.
# The first frame (the poster GitHub shows) is the picker with a preview.
# Run from the repository root after `cargo build --release`:  vhs docs/media/sessions-walk.tape
# Nothing is launched: every step is a preview run (CLAUDE_AUTO_PREVIEW=1) whose scripted keys end
# on the screen shown, drawn on the demo home and the invented projects of demo-setup.ps1.

Output docs/media/sessions-walk.gif
Output docs/media/sessions-walk.mp4
Set Shell "pwsh"
Set FontFamily "Cascadia Mono"
Set FontSize 22
Set Width 1472
Set Height 640
Set Padding 24
Set Margin 40
Set MarginFill "target/vhs/backdrop-sessions-walk.png"
Set WindowBar Colorful
Set WindowBarSize 44
Set BorderRadius 10
Set Theme "GitHub Dark"
Set Framerate 24
Set TypingSpeed 10ms

# A preview run prints one frame per key. Each step runs the key script one key short as well and
# prints only the lines past it: the last frame alone. $null as the shorter script = no frames.
# The short sleep after each Hide keeps the next typed command out of the recording.
Hide
Type ". ./docs/media/demo-setup.ps1; $env:CLAUDE_AUTO_PREVIEW = '1'; $env:CLAUDE_AUTO_PREVIEW_SIZE = '100x23'; function global:prompt { [char]27 + '[?25l' }; function step($k, $p) { $n = 0; if ($null -ne $p) { $env:CLAUDE_AUTO_PREVIEW_KEYS = $p; $n = @(claude-auto 2>$null).Count }; $env:CLAUDE_AUTO_PREVIEW_KEYS = $k; $all = @(claude-auto 2>$null); Clear-Host; $all | Select-Object -Skip $n | ForEach-Object { [Console]::WriteLine($_) } }; Set-Location $env:CLAUDE_AUTO_DEMO_DRIVE\; Clear-Host"
Enter
Sleep 3s

# r on aurora: its newest session previewed.
Type "step 'Enter,s,r' 'Enter,s'"
Enter
Sleep 2s
Show
Sleep 2.5s

# Down through the sessions: the preview follows the cursor.
Hide
Sleep 200ms
Type "step 'Enter,s,r,s' 'Enter,s,r'"
Enter
Sleep 1.5s
Show
Sleep 2.5s
Hide
Sleep 200ms
Type "step 'Enter,s,r,s,s' 'Enter,s,r,s'"
Enter
Sleep 1.5s
Show
Sleep 2.5s

# tab: every project's sessions.
Hide
Sleep 200ms
Type "step 'Enter,s,r,s,s,Tab' 'Enter,s,r,s,s'"
Enter
Sleep 1.5s
Show
Sleep 2.5s

# esc: the project screen, esc again: the launch screen.
Hide
Sleep 200ms
Type "step 'Enter,s' 'Enter'"
Enter
Sleep 1s
Show
Sleep 1.2s
Hide
Sleep 200ms
Type "step '' $null"
Enter
Sleep 1s
Show
Sleep 1.5s

# enter: the project screen, s: aurora. The loop goes on with r.
Hide
Sleep 200ms
Type "step 'Enter' ''"
Enter
Sleep 1s
Show
Sleep 1s
Hide
Sleep 200ms
Type "step 'Enter,s' 'Enter'"
Enter
Sleep 1s
Show
Sleep 1.2s
