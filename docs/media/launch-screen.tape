# The launch screen of the Rust launcher, captured in preview mode on the demo roster.
# Run from the repository root after `cargo build --release`:  vhs docs/media/launch-screen.tape
# Nothing is launched: CLAUDE_AUTO_PREVIEW=1, a throwaway home from demo-setup.ps1, one Escape key.

Output target/vhs/launch-screen.gif
Set Shell "pwsh"
Set FontFamily "Cascadia Mono"
Set FontSize 16
Set Width 1080
Set Height 400
Set Padding 20
Set Theme "Builtin Dark"
Set TypingSpeed 40ms

Hide
Type ". ./docs/media/demo-setup.ps1; $env:CLAUDE_AUTO_PREVIEW = '1'; $env:CLAUDE_AUTO_PREVIEW_SIZE = '100x20'; $env:CLAUDE_AUTO_PREVIEW_KEYS = 'Escape'; Clear-Host"
Enter
Sleep 2s
Show

Type "claude-auto"
Sleep 300ms
Enter
Sleep 2s
Screenshot docs/media/launch-screen.png
Sleep 1s
