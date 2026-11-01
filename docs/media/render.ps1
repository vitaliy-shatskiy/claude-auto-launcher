#Requires -Version 7.4
# Redraws every README image: the SVG figures, then the two VHS recordings of the real binary.
# Run from the repository root after `cargo build --release`:  pwsh -File docs/media/render.ps1
# Needs node and vhs (with ttyd and ffmpeg) on PATH. Exit 0 drawn, 1 a step failed.
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version 3.0

node docs/media/draw.mjs
New-Item -ItemType Directory -Force -Path 'target/vhs' | Out-Null

# Each tape frames its window on a backdrop of its own size: the launcher's orange fading to the dark page.
$tapes = [ordered]@{ 'launch-screen' = '2092x912'; 'launch-walk' = '1472x592' }
foreach ($e in $tapes.GetEnumerator()) {
    $w, $h = $e.Value -split 'x'
    $grad = "gradients=s=$($e.Value):c0=0x5a2a1a:c1=0x131922:x0=0:y0=0:x1=${w}:y1=${h}:nb_colors=2:type=linear"
    ffmpeg -y -v error -f lavfi -i $grad -frames:v 1 "target/vhs/backdrop-$($e.Key).png"
    vhs "docs/media/$($e.Key).tape"
}

# VHS always draws its cursor block under the last output line: paint it with the terminal background.
$png = 'docs/media/launch-screen.png'
$tmp = 'target/vhs/launch-screen-nocursor.png'
ffmpeg -y -v error -i $png -vf 'drawbox=x=82:y=726:w=48:h=56:color=0x0F1015:t=fill' $tmp
Move-Item -Force -LiteralPath $tmp -Destination $png
