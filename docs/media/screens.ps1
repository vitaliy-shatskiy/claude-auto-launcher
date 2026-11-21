#Requires -Version 7.4
# Screenshots of every screen at the sizes the owner runs it: PNGs of the real binary, drawn on the
# demo home (demo-setup.ps1), never the machine's own stores. Nothing opens a window: each frame is
# taken from a preview run (CLAUDE_AUTO_PREVIEW=1, scripted keys), then VHS prints it in its
# headless terminal and screenshots it.
# PowerShell, not Rust: glue around vhs, like render.ps1 (the media pipeline's own convention).
#
#   pwsh -NoProfile -File docs/media/screens.ps1                       every screen at the seven sizes below
#   -Screens 'launch,project'  -Sizes '130x45'  -Exe <path>            a subset, another build
#
# Run from the repository root after `cargo build`. Output: target/screens/<screen>-<WxH>.png.
# Exit: 0 every PNG written, 1 one failed, 2 did not run (no binary, no vhs, an error).
# Sizes: 80x24 classic; 120x30 the Windows Terminal default window (initialCols 120, initialRows
# 30: learn.microsoft.com/windows/terminal/customize-settings/startup, read 21.11.2026);
# 160x45; 211x52 FullHD maximized (Cascadia Mono 12pt); 280x72 2K maximized (measured); 320x85
# 2K full screen; 55x20 narrow. home-projects and home-sessions: the launch page's focus moved
# to the list and to the sessions (wide only; under 120 columns they are the launch page).
[CmdletBinding()]
param(
    [string]$Screens = 'launch,home-projects,home-sessions,project,picker-all,maintenance,navigator,rows-editor',
    [string]$Sizes = '80x24,120x30,160x45,211x52,280x72,320x85,55x20',
    [string]$Exe
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version 3.0

# Keys from the launch screen to each screen (the project list: the cwd row, then the demo projects
# newest first, `enter a path...` under them: {down} walks there). {drive} is the demo drive's letter.
$keyScripts = [ordered]@{
    'launch'        = ''
    'home-projects' = 'Tab,s'
    'home-sessions' = 'Tab,s,Tab'
    'project'     = 'Enter,s'
    'picker-all'  = 'Enter,s,r,Tab'
    'maintenance' = 'u'
    'navigator'   = 'Enter,{down},Enter,{drive},:,\'
    'rows-editor' = 'u,l'
}

# The frame the last key of a script draws. A preview prints one frame per key, the first before
# any key; a screen left inside another redraws the outer ones as the keys run out, so the frame is
# picked by its index. A frame starts where a top border follows the footer lines of the one
# before (footer lines start with spaces, panel lines never do).
function Get-LastFrame([string]$Binary, [string]$KeyScript, [string]$Size) {
    $PSNativeCommandUseErrorActionPreference = $false
    $env:CLAUDE_AUTO_PREVIEW = '1'
    $env:CLAUDE_AUTO_PREVIEW_SIZE = $Size
    $env:CLAUDE_AUTO_PREVIEW_KEYS = $KeyScript
    $want = @($KeyScript -split ',' | Where-Object { $_ -ne '' }).Count
    $index = 0
    $footerSeen = $false
    foreach ($line in @(& $Binary 2>$null)) {
        $plain = $line -replace '\x1b\[[0-9;?]*[A-Za-z]', ''
        if ($footerSeen -and $plain -match '^[\u256D+]') {
            $index++
            $footerSeen = $false
        }
        if ($index -gt $want) { break }
        if ($plain -match '^ ') { $footerSeen = $true }
        if ($index -eq $want) { $line }
    }
}

# A tape that prints the frame file in VHS's terminal and screenshots it. The window is a few
# columns and rows larger than the frame (VHS's cell size varies with the window: a window of
# exactly the frame's columns wraps it); Set-ShotCrop then cuts the PNG to the preview's size.
function Format-ShotTape([string]$Ans, [string]$Png, [int]$Columns, [int]$Rows) {
    $px = [int][Math]::Ceiling(($Columns + 6) * 9.3) + 24
    $py = ($Rows + 3) * 17 + 24
    $print = "function global:prompt { [char]27 + '[?25l' }; Clear-Host; [Console]::Out.Write([IO.File]::ReadAllText('$($Ans -replace "'", "''")'))"
    @(
        "Output `"$([IO.Path]::ChangeExtension($Png, '.gif') -replace '\\', '/')`""
        'Set Shell "pwsh"'
        'Set FontFamily "Cascadia Mono"'
        'Set FontSize 14'
        "Set Width $px"
        "Set Height $py"
        'Set Padding 12'
        'Set Theme "GitHub Dark"'
        'Set TypingSpeed 1ms'
        'Hide'
        "Type `"$($print -replace '"', '\"')`""
        'Enter'
        'Sleep 1500ms'
        'Show'
        "Screenshot `"$($Png -replace '\\', '/')`""
        'Sleep 300ms'
    ) -join "`n"
}

# The PNG cut to the terminal the preview drew: the frame's top-left corner to its last drawn
# column plus the terminal's free last column, and its last row (the footer), each with the
# 12 px padding. What is left out is the unused part of VHS's larger window.
function Set-ShotCrop {
    [CmdletBinding(SupportsShouldProcess)]
    param([string]$Png, [int]$Columns)
    Add-Type -AssemblyName System.Drawing
    $src = [System.Drawing.Bitmap]::new($Png)
    try {
        $bg = $src.GetPixel(2, 2)
        $far = { param($p) [Math]::Abs($p.R - $bg.R) + [Math]::Abs($p.G - $bg.G) + [Math]::Abs($p.B - $bg.B) -gt 40 }
        # Scanned from the far edges inward: the first column and row holding a drawn pixel.
        $maxX = 0
        for ($x = $src.Width - 1; $x -gt 0 -and $maxX -eq 0; $x--) {
            for ($y = 0; $y -lt $src.Height; $y += 2) { if (& $far $src.GetPixel($x, $y)) { $maxX = $x; break } }
        }
        $maxY = 0
        for ($y = $src.Height - 1; $y -gt 0 -and $maxY -eq 0; $y--) {
            for ($x = 0; $x -lt $src.Width; $x += 2) { if (& $far $src.GetPixel($x, $y)) { $maxY = $y; break } }
        }
        # The frame's widest line is Columns-1 cells: one cell more, then the padding.
        $cell = ($maxX - 12) / [Math]::Max(1, $Columns - 1)
        $w = [Math]::Min($src.Width, [int][Math]::Ceiling($maxX + $cell + 12))
        $h = [Math]::Min($src.Height, $maxY + 14)
        $cut = $src.Clone([System.Drawing.Rectangle]::new(0, 0, $w, $h), $src.PixelFormat)
    } finally { $src.Dispose() }
    if ($PSCmdlet.ShouldProcess($Png, 'crop to the preview size')) {
        try { $cut.Save($Png, [System.Drawing.Imaging.ImageFormat]::Png) } finally { $cut.Dispose() }
    }
}

# VHS runs under the real profile: its browser and ttyd live there, not in the demo home.
function Invoke-VhsTape([string]$Tape, [string]$UserProfile) {
    $PSNativeCommandUseErrorActionPreference = $false
    $demoHome = $env:USERPROFILE
    $env:USERPROFILE = $UserProfile
    try { vhs $Tape *> $null } finally { $env:USERPROFILE = $demoHome }
}

function Remove-DemoDrive {
    [CmdletBinding(SupportsShouldProcess)]
    param()
    $PSNativeCommandUseErrorActionPreference = $false
    $demo = Join-Path ([IO.Path]::GetTempPath()) 'claude-auto-demo'
    foreach ($line in @(subst)) {
        if ($line -like "*=> $demo\*" -and $PSCmdlet.ShouldProcess($line.Substring(0, 2), 'drop the demo drive')) { subst $line.Substring(0, 2) /D | Out-Null }
    }
}

try {
    if (-not (Get-Command vhs -ErrorAction SilentlyContinue)) { Write-Information 'CANNOT RUN: vhs is not on PATH' -InformationAction Continue; exit 2 }
    $repo = (Get-Location).Path
    if (-not (Test-Path -LiteralPath (Join-Path $repo 'docs/media/demo-setup.ps1'))) { Write-Information 'CANNOT RUN: run from the repository root' -InformationAction Continue; exit 2 }
    if (-not $Exe) { $Exe = Join-Path $repo 'target/debug/claude-auto.exe' }
    if (-not (Test-Path -LiteralPath $Exe)) { Write-Information "CANNOT RUN: no binary at $Exe (cargo build first)" -InformationAction Continue; exit 2 }
    $binary = (Resolve-Path -LiteralPath $Exe).Path
    $realProfile = $env:USERPROFILE
    # The demo home: a throwaway USERPROFILE, the demo roster, invented projects on a subst drive;
    # CLAUDE_AUTO_DEMO_MANY: as many projects and sessions as a real home, two of them running.
    $env:CLAUDE_AUTO_DEMO_MANY = '1'
    . (Join-Path $repo 'docs/media/demo-setup.ps1')
    # The project list: the cwd row, every demo project, then `enter a path...`.
    $projects = @($demoSessions | ForEach-Object project | Sort-Object -Unique).Count
    $down = (@('s') * ($projects + 1)) -join ','
    $drive = $env:CLAUDE_AUTO_DEMO_DRIVE
    $out = Join-Path $repo 'target/screens'
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    $failed = 0
    $written = 0
    Push-Location "$drive\"
    try {
        foreach ($size in @($Sizes -split ',')) {
            $cols, $rows = [int[]]($size -split 'x')
            foreach ($screen in @($Screens -split ',')) {
                $name = "$screen-$size"
                $frame = @(Get-LastFrame -Binary $binary -KeyScript ($keyScripts[$screen] -replace '\{drive\}', $drive.Substring(0, 1) -replace '\{down\}', $down) -Size $size)
                if ($frame.Count -lt 3) { Write-Information "FAILED  ${name}: the preview drew no frame" -InformationAction Continue; $failed++; continue }
                $ans = Join-Path $out "$name.ans"
                [IO.File]::WriteAllText($ans, ($frame -join "`n"))
                $png = Join-Path $out "$name.png"
                if (Test-Path -LiteralPath $png) { Remove-Item -LiteralPath $png }
                $tape = Join-Path $out "$name.tape"
                [IO.File]::WriteAllText($tape, (Format-ShotTape -Ans $ans -Png $png -Columns $cols -Rows $rows) + "`n")
                Invoke-VhsTape -Tape $tape -UserProfile $realProfile
                if (Test-Path -LiteralPath $png) { Set-ShotCrop -Png $png -Columns $cols; $written++; Write-Information "ok      $png" -InformationAction Continue }
                else { Write-Information "FAILED  ${name}: vhs wrote no PNG ($tape)" -InformationAction Continue; $failed++ }
            }
        }
    } finally {
        Pop-Location
        Remove-DemoDrive
    }
    Write-Information "$written written, $failed failed" -InformationAction Continue
    exit $(if ($failed -gt 0) { 1 } else { 0 })
} catch {
    Write-Error $_ -ErrorAction Continue
    exit 2
}
