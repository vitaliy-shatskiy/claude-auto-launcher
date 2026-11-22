#Requires -Version 7.4
# Redraws every README image: the SVG figures, then the four VHS recordings of the real binary.
# Run from the repository root after `cargo build --release`:  pwsh -File docs/media/render.ps1
# Needs node and vhs (with ttyd and ffmpeg) on PATH. Exit 0 drawn, 1 a step failed.
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version 3.0

node docs/media/draw.mjs
New-Item -ItemType Directory -Force -Path 'target/vhs' | Out-Null

# No backdrop margin: the 122 columns fill the image so the README's 880 px keeps the text legible.
foreach ($tape in 'launch-screen', 'launch-walk', 'sessions-walk', 'navigator-walk') {
    vhs "docs/media/$tape.tape"
}

# demo-setup.ps1 substitutes a drive for the demo projects: drop it.
$demo = Join-Path ([IO.Path]::GetTempPath()) 'claude-auto-demo'
foreach ($line in @(subst)) {
    if ($line -like "*=> $demo\*") { subst $line.Substring(0, 2) /D }
}
