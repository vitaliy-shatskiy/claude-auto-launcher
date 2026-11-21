# The launch page of the Rust launcher on a wide terminal, the demo home with many projects.
# Run from the repository root after `cargo build --release`:  vhs docs/media/launch-screen.tape
# The binary runs interactively on a throwaway home from demo-setup.ps1 (CLAUDE_AUTO_DEMO_MANY=1:
# ~20 invented projects and two running sessions); nothing is launched, the recording ends on the
# screen. 122 columns: the workspace (launch, projects, the right column) shows from 120.

Output target/vhs/launch-screen.gif
Set Shell "pwsh"
Set FontFamily "Cascadia Mono"
Set FontSize 32
Set Width 2580
Set Height 1240
Set Padding 36
Set Margin 56
Set MarginFill "target/vhs/backdrop-launch-screen.png"
Set WindowBar Colorful
Set WindowBarSize 64
Set BorderRadius 14
Set Theme "GitHub Dark"
Set TypingSpeed 10ms

Hide
Type "$env:CLAUDE_AUTO_DEMO_MANY = '1'; . ./docs/media/demo-setup.ps1; Set-Location $env:CLAUDE_AUTO_DEMO_DRIVE\; Clear-Host; claude-auto"
Enter
Sleep 4s
Show
Sleep 500ms
Screenshot docs/media/launch-screen.png
Sleep 500ms
