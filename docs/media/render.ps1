#Requires -Version 7.4
# Redraws every README image: the SVG figures, then the four VHS recordings of the real binary.
# Run from the repository root after `cargo build --release`:  pwsh -File docs/media/render.ps1
# Needs node and vhs (with ttyd and ffmpeg) on PATH. Exit 0 drawn, 1 a step failed.
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version 3.0

node docs/media/draw.mjs
New-Item -ItemType Directory -Force -Path 'target/vhs' | Out-Null

# Each tape frames its window on a backdrop of its own size: the launcher's orange fading to the dark page.
$tapes = [ordered]@{ 'launch-screen' = '2580x1240'; 'launch-walk' = '1600x760'; 'sessions-walk' = '1600x760'; 'navigator-walk' = '1600x760' }
foreach ($e in $tapes.GetEnumerator()) {
    $w, $h = $e.Value -split 'x'
    $grad = "gradients=s=$($e.Value):c0=0x5a2a1a:c1=0x131922:x0=0:y0=0:x1=${w}:y1=${h}:nb_colors=2:type=linear"
    ffmpeg -y -v error -f lavfi -i $grad -frames:v 1 "target/vhs/backdrop-$($e.Key).png"
    vhs "docs/media/$($e.Key).tape"
}

# demo-setup.ps1 substitutes a drive for the demo projects: drop it.
$demo = Join-Path ([IO.Path]::GetTempPath()) 'claude-auto-demo'
foreach ($line in @(subst)) {
    if ($line -like "*=> $demo\*") { subst $line.Substring(0, 2) /D }
}
