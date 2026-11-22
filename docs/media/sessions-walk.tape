# Sessions on the launch page and in the session picker: tab moves focus to the projects, the right
# column follows the selected project (its facts and sessions); r opens the picker with a chat
# preview that follows the cursor, tab widens it to every project. The loop then starts over.
# The first frame (the poster GitHub shows) is the workspace on aurora with its sessions.
# Run from the repository root after `cargo build --release`:  vhs docs/media/sessions-walk.tape
# Nothing is launched: every step is a preview run (CLAUDE_AUTO_PREVIEW=1) whose scripted keys end
# on the screen shown, drawn on the demo home and the invented projects of demo-setup.ps1.
# 122 columns: the workspace (launch, projects, the right column) shows from 120.

Output docs/media/sessions-walk.gif
Output docs/media/sessions-walk.mp4
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

# A preview run prints one frame per key. Each step runs the key script one key short as well and
# prints only the lines past it: the last frame alone. $null as the shorter script = no frames.
# The short sleep after each Hide keeps the next typed command out of the recording.
# The prompt hides the cursor through the console as well: the escape alone does not hold under ConPTY.
Hide
Type ". ./docs/media/demo-setup.ps1; $env:CLAUDE_AUTO_PREVIEW = '1'; $env:CLAUDE_AUTO_PREVIEW_SIZE = '122x24'; function global:prompt { [Console]::CursorVisible = $false; [char]27 + '[?25l' }; function step($k, $p) { $n = 0; if ($null -ne $p) { $env:CLAUDE_AUTO_PREVIEW_KEYS = $p; $n = @(claude-auto 2>$null).Count }; $env:CLAUDE_AUTO_PREVIEW_KEYS = $k; $all = @(claude-auto 2>$null); Clear-Host; $all | Select-Object -Skip $n | ForEach-Object { [Console]::WriteLine($_) } }; Set-Location $env:CLAUDE_AUTO_DEMO_DRIVE\; Clear-Host"
Enter
Sleep 3s

# tab, then s on aurora: the right column shows its facts and sessions.
Type "step 'Tab,s' 'Tab'"
Enter
Sleep 2s
Show
Sleep 2.5s

# s: harbor-api, the right column follows.
Hide
Sleep 200ms
Type "step 'Tab,s,s' 'Tab,s'"
Enter
Sleep 1.5s
Show
Sleep 2s

# w back to aurora, r: the session picker, the newest session's chat previewed.
Hide
Sleep 200ms
Type "step 'Tab,s,s,w' 'Tab,s,s'"
Enter
Sleep 1.5s
Show
Sleep 1.2s
Hide
Sleep 200ms
Type "step 'Tab,s,s,w,r' 'Tab,s,s,w'"
Enter
Sleep 1.5s
Show
Sleep 2.5s

# Down through the sessions: the preview follows the cursor.
Hide
Sleep 200ms
Type "step 'Tab,s,s,w,r,s' 'Tab,s,s,w,r'"
Enter
Sleep 1.5s
Show
Sleep 2.5s
Hide
Sleep 200ms
Type "step 'Tab,s,s,w,r,s,s' 'Tab,s,s,w,r,s'"
Enter
Sleep 1.5s
Show
Sleep 2.5s

# tab: every project's sessions.
Hide
Sleep 200ms
Type "step 'Tab,s,s,w,r,s,s,Tab' 'Tab,s,s,w,r,s,s'"
Enter
Sleep 1.5s
Show
Sleep 3s
