#Requires -Version 7.4
# Screenshots of every screen at the sizes the owner runs it: PNGs of the real binary, drawn on the
# demo home (demo-setup.ps1), never the machine's own stores. Nothing opens a window: each frame is
# taken from a preview run (CLAUDE_AUTO_PREVIEW=1, scripted keys), then VHS prints it in its
# headless terminal and screenshots it.
# PowerShell, not Rust: glue around vhs, like render.ps1 (the media pipeline's own convention).
#
#   pwsh -NoProfile -File docs/media/screens.ps1                       every screen at 280x72, 130x45, 55x20
#   -Screens 'launch,project'  -Sizes '130x45'  -Exe <path>            a subset, another build
#
# Run from the repository root after `cargo build`. Output: target/screens/<screen>-<WxH>.png.
# Exit: 0 every PNG written, 1 one failed, 2 did not run (no binary, no vhs, an error).
[CmdletBinding()]
param(
    [string]$Screens = 'launch,project,picker-all,maintenance,navigator,rows-editor',
    [string]$Sizes = '280x72,130x45,55x20',
    [string]$Exe
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version 3.0

# Keys from the launch screen to each screen (the project list: the cwd row, then the demo projects
# newest first, `enter a path...` four rows down). {drive} is the demo drive's letter.
$keyScripts = [ordered]@{
    'launch'      = ''
    'project'     = 'Enter,s'
    'picker-all'  = 'Enter,s,r,Tab'
    'maintenance' = 'u'
    'navigator'   = 'Enter,s,s,s,s,Enter,{drive},:,\'
    'rows-editor' = 'u,l'
}

# The last frame a key script draws: the run one key short draws every frame before it.
function Get-LastFrame([string]$Binary, [string]$KeyScript, [string]$Size) {
    $PSNativeCommandUseErrorActionPreference = $false
    $env:CLAUDE_AUTO_PREVIEW = '1'
    $env:CLAUDE_AUTO_PREVIEW_SIZE = $Size
    $parts = @($KeyScript -split ',' | Where-Object { $_ -ne '' })
    $skip = 0
    if ($parts.Count -gt 0) {
        $env:CLAUDE_AUTO_PREVIEW_KEYS = ($parts | Select-Object -SkipLast 1) -join ','
        $skip = @(& $Binary 2>$null).Count
    }
    $env:CLAUDE_AUTO_PREVIEW_KEYS = $KeyScript
    @(& $Binary 2>$null) | Select-Object -Skip $skip
}

# A tape that prints the frame file in VHS's terminal and screenshots it. The window is a few
# columns wider than the frame: a preview of exactly its columns wraps.
function Format-ShotTape([string]$Ans, [string]$Png, [int]$Columns, [int]$Rows) {
    $px = [int][Math]::Ceiling(($Columns + 6) * 9.1) + 24
    $py = ($Rows + 2) * 17 + 24
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
    # The demo home: a throwaway USERPROFILE, the demo roster, invented projects on a subst drive.
    . (Join-Path $repo 'docs/media/demo-setup.ps1')
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
                $frame = @(Get-LastFrame -Binary $binary -KeyScript ($keyScripts[$screen] -replace '\{drive\}', $drive.Substring(0, 1)) -Size $size)
                if ($frame.Count -lt 3) { Write-Information "FAILED  ${name}: the preview drew no frame" -InformationAction Continue; $failed++; continue }
                $ans = Join-Path $out "$name.ans"
                [IO.File]::WriteAllText($ans, ($frame -join "`n"))
                $png = Join-Path $out "$name.png"
                if (Test-Path -LiteralPath $png) { Remove-Item -LiteralPath $png }
                $tape = Join-Path $out "$name.tape"
                [IO.File]::WriteAllText($tape, (Format-ShotTape -Ans $ans -Png $png -Columns $cols -Rows $rows) + "`n")
                Invoke-VhsTape -Tape $tape -UserProfile $realProfile
                if (Test-Path -LiteralPath $png) { $written++; Write-Information "ok      $png" -InformationAction Continue }
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
