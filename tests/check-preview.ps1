# Guards the launch screen's DECISION path - not just the headless --version path
# check-regression.ps1 already covers. Same shape as check-regression.ps1, same three outcomes.
#
# Why this exists: the 13-check gate asserted exactly ONE path through the 500+ line
# claude-auto.ps1 (the headless --version branch). tests\preview.ps1 already prints a full decision
# summary - account, CLAUDE_CONFIG_DIR, remote, launch args, command, argv count - for any scripted
# key sequence, and nothing read it. That is how the session-picker-ignores-the-chosen-account
# defect (Get-ClaudeSessions called with no root at all) lived in a repository with a green
# checkpoint: no check ever drove the launch screen past its default account.
#
# A FIXTURE config (CLAUDE_AUTO_CONFIG) is used so the reference does not depend on the owner's own
# accounts.json - only its 'work' entry needs to be the real ~/.claude (the canonical-root rule in
# Config.ps1 requires exactly one such account; its CLAUDE_CONFIG_DIR line is always the same empty
# value regardless of what that directory actually contains). preview.ps1 already redirects
# CLAUDE_AUTO_PREFS to its own throwaway file per run, so remembered choices cannot change the output.
#
# Exit 2 is "could not run", never a pass: a missing launcher, a missing fixture or a missing
# reference all return 2, same as check-regression.ps1.
#
# Refresh the reference deliberately, only after confirming the new output is correct:
#   pwsh -File tests/check-preview.ps1 -Record

[CmdletBinding()]
param(
    [string]$Preview = (Join-Path $PSScriptRoot 'preview.ps1'),
    [string]$Launcher = (Join-Path $PSScriptRoot '..\claude-auto.ps1'),
    [string]$FixtureConfig = (Join-Path $PSScriptRoot 'fixtures\config-preview.json'),
    [string]$Reference = (Join-Path $PSScriptRoot 'preview-reference.local.txt'),
    # Capture a fresh reference from the launcher's current preview output instead of comparing.
    [switch]$Record
)

$ErrorActionPreference = 'Stop'

# One run at the default (Enter alone) and one that CHANGES THE ACCOUNT - the exact shape of the
# scenario finding 1 needed and nothing exercised. Named so a failing line names its own run.
$script:Runs = [ordered]@{
    'default-enter'     = 'Enter'
    'switch-account'    = 'RightArrow,Enter'
    # Account, then Action row (new -> continue -> resume), Enter opens the picker, Escape cancels
    # it. The fixture's 'second' account root has no projects directory at all, so a CORRECT
    # session picker shows nothing and Escape returns 'picker cancelled'. A regression of finding 1
    # (Get-ClaudeSessions called with no root) would instead read the CANONICAL account's real
    # sessions - non-empty on any machine that has used Claude Code - and Enter picks the first one,
    # printing a real --resume id in launch args instead of cancelling. That divergence is the guard.
    'switch-and-resume' = 'RightArrow,DownArrow,RightArrow,RightArrow,Enter,Escape'
}

# preview.ps1's own (non--Full) summary filter does not include CLAUDE_CONFIG_DIR or argv count -
# both are named explicitly by the finding this check exists for (the session picker's account
# resolution shows up in CLAUDE_CONFIG_DIR, not in the launch args), so this check runs -Full and
# applies its own, wider filter rather than silently missing them.
#
# 'sessions' (the picker frame's own title, "resume - N sessions", and its "no sessions found"
# message) is the one line that actually distinguishes a regression of finding 1 from the fix: an
# Escape always cancels the picker regardless of what it found, so "picker cancelled" alone reads
# identically whether the list was empty (correct, for a fixture account with no projects
# directory) or full of the CANONICAL account's real sessions (the regression) - the session COUNT
# printed in the frame's own title is what tells the two apart without completing a pick.
$script:WantedPattern = 'launch args\s*:|command\s*:|remote\s*:|account\s*:|CLAUDE_CONFIG_DIR\s*:|argv count\s*:|picker cancelled|remote off for this session|crc |sessions|no sessions found'

function Invoke-PreviewRun {
    param([Parameter(Mandatory)][string]$Keys)
    $savedConfig = $env:CLAUDE_AUTO_CONFIG
    try {
        $env:CLAUDE_AUTO_CONFIG = $FixtureConfig
        $out = & pwsh -NoProfile -File $Preview -Keys $Keys -Launcher $Launcher -Full 2>&1
        $code = $LASTEXITCODE
        $filtered = @($out | ForEach-Object { "$_" } | Where-Object { $_ -match $script:WantedPattern })
        return @($filtered) + @("preview.ps1 exit: $code")
    } finally {
        if ($null -eq $savedConfig) { Remove-Item Env:CLAUDE_AUTO_CONFIG -ErrorAction SilentlyContinue }
        else { $env:CLAUDE_AUTO_CONFIG = $savedConfig }
    }
}

function Get-AllRunOutput {
    $lines = New-Object System.Collections.Generic.List[string]
    foreach ($name in $script:Runs.Keys) {
        $lines.Add("=== $name ===")
        foreach ($l in (Invoke-PreviewRun -Keys $script:Runs[$name])) { $lines.Add($l) }
    }
    return , @($lines)
}

if (-not (Test-Path -LiteralPath $Launcher)) {
    Write-Host "CANNOT CHECK: launcher not found at $Launcher" -ForegroundColor Red
    exit 2
}
if (-not (Test-Path -LiteralPath $Preview)) {
    Write-Host "CANNOT CHECK: preview.ps1 not found at $Preview" -ForegroundColor Red
    exit 2
}
if (-not (Test-Path -LiteralPath $FixtureConfig)) {
    Write-Host "CANNOT CHECK: fixture config not found at $FixtureConfig" -ForegroundColor Red
    exit 2
}

if ($Record) {
    try {
        $lines = Get-AllRunOutput
        if ($lines.Count -eq 0) {
            Write-Host "CANNOT RECORD: the preview runs produced no output at all" -ForegroundColor Red
            exit 2
        }
        Set-Content -LiteralPath $Reference -Value $lines -Encoding utf8
        Write-Host "recorded $($lines.Count) line(s) to $Reference" -ForegroundColor Green
        exit 0
    } catch {
        Write-Host "CANNOT RECORD: $($_.Exception.Message)" -ForegroundColor Red
        exit 2
    }
}

if (-not (Test-Path -LiteralPath $Reference)) {
    Write-Host "CANNOT CHECK: no reference output at $Reference" -ForegroundColor Red
    Write-Host "Capture one first: pwsh -File tests\check-preview.ps1 -Record - do not treat this as a pass." -ForegroundColor Red
    exit 2
}

try {
    $actual = Get-AllRunOutput
} catch {
    Write-Host "CANNOT CHECK: $($_.Exception.Message)" -ForegroundColor Red
    exit 2
}

$refLines = @(Get-Content -LiteralPath $Reference -ErrorAction SilentlyContinue)
if ($refLines.Count -eq 0) {
    Write-Host "CANNOT CHECK: the reference at $Reference is empty" -ForegroundColor Red
    exit 2
}

# Positional, not set semantics: order (which run's block comes first, and each line within it) is
# part of the decision path being pinned.
$max = [Math]::Max($refLines.Count, $actual.Count)
$pairs = @()
for ($i = 0; $i -lt $max; $i++) {
    $e = if ($i -lt $refLines.Count) { $refLines[$i] } else { $null }
    $a = if ($i -lt $actual.Count) { $actual[$i] } else { $null }
    if ($e -cne $a) { $pairs += [pscustomobject]@{ Line = $i + 1; Expected = $e; Actual = $a } }
}

if ($pairs.Count -eq 0) {
    Write-Host "OK: preview decision summary unchanged ($($refLines.Count) comparable line(s), $($script:Runs.Count) run(s))" -ForegroundColor Green
    exit 0
}

$none = '(no such line)'
Write-Host "REGRESSION: the preview decision summary changed - $($pairs.Count) line(s) differ" -ForegroundColor Red
foreach ($p in $pairs) {
    Write-Host "  line $($p.Line)" -ForegroundColor Red
    Write-Host "    expected: $(if ($null -eq $p.Expected) { $none } else { $p.Expected })" -ForegroundColor DarkGray
    Write-Host "    actual  : $(if ($null -eq $p.Actual) { $none } else { $p.Actual })" -ForegroundColor Yellow
}
Write-Host "reference: $Reference" -ForegroundColor DarkGray
exit 1
