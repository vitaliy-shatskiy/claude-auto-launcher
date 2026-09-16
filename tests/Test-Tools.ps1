# Assertions for tools/Show-LauncherRun.ps1 (the read-only launcher-run timeline printer).
# Run: pwsh -NoProfile -File Test-Tools.ps1
#
# Runs the tool as a CHILD process against a throwaway -LogRoot fixture under $env:TEMP - it must
# never read the real ~/.claude/launcher-logs. Fixture ids, dates and paths below are invented.

$ToolPath = Join-Path $PSScriptRoot '..\tools\Show-LauncherRun.ps1'
if (-not (Test-Path -LiteralPath $ToolPath)) { Write-Host "COULD NOT RUN: tool not found: $ToolPath"; exit 2 }

$script:Failed = 0
$script:Ran = 0
function Assert-Equal {
    param($Expected, $Actual, [string]$Because)
    $script:Ran++
    if ("$Expected" -ne "$Actual") {
        Write-Host "FAIL  $Because"
        Write-Host "      expected: $Expected"
        Write-Host "      actual:   $Actual"
        $script:Failed++
    } else {
        Write-Host "ok    $Because"
    }
}

function Assert-True {
    param([bool]$Actual, [string]$Because)
    $script:Ran++
    if (-not $Actual) { Write-Host "FAIL  $Because"; $script:Failed++ } else { Write-Host "ok    $Because" }
}

function Invoke-Tool {
    param([string[]]$ToolArgs)
    $out = @(& pwsh -NoProfile -File $ToolPath @ToolArgs 2>&1 | ForEach-Object { "$_" })
    [pscustomobject]@{ Out = $out; Code = $LASTEXITCODE }
}

$root = Join-Path $env:TEMP ("launcher-run-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $root | Out-Null

try {
    # --- Fixture: two day files, run A (reaches the UI), run B (useUi=false), one garbage line, one
    # blank line. Run A is entirely BEFORE run B in time, so run B is the overall-newest run while
    # run A is the newest run that reached the UI - the two `-Last` behaviours diverge only if that
    # ordering holds.
    $longNote = 'x' * 200

    $fileA = @(
        (@{ ts = '2026-09-15T10:00:00'; stage = 'start'; run = 'AAA111'; pid = 4001; cmd = 'claude-auto.ps1' } | ConvertTo-Json -Compress),
        (@{ ts = '2026-09-15T10:00:02'; stage = 'decision'; run = 'AAA111'; pid = 4001; useUi = $true; account = 'work' } | ConvertTo-Json -Compress),
        '{not valid json at all',
        '',
        (@{ ts = '2026-09-15T11:00:00'; stage = 'start'; run = 'BBB222'; pid = 4002; cmd = 'claude-auto.ps1' } | ConvertTo-Json -Compress)
    )
    Set-Content -LiteralPath (Join-Path $root 'claude-auto-2026-09-15.jsonl') -Value $fileA

    $fileB = @(
        (@{ ts = '2026-09-15T10:00:05'; stage = 'ui'; run = 'AAA111'; pid = 4001;
            saved   = @{ account = 'work'; profile = 'default' };
            parents = @(@{ name = 'explorer'; pid = 100 }, @{ name = 'wt'; pid = 200 });
            note    = $longNote } | ConvertTo-Json -Compress -Depth 5),
        (@{ ts = '2026-09-15T10:00:20'; stage = 'exit'; run = 'AAA111'; pid = 4001; code = 0 } | ConvertTo-Json -Compress),
        (@{ ts = '2026-09-15T11:00:02'; stage = 'decision'; run = 'BBB222'; pid = 4002; useUi = $false; account = 'work' } | ConvertTo-Json -Compress),
        (@{ ts = '2026-09-15T11:00:10'; stage = 'exit'; run = 'BBB222'; pid = 4002; code = 0 } | ConvertTo-Json -Compress)
    )
    Set-Content -LiteralPath (Join-Path $root 'claude-auto-2026-09-16.jsonl') -Value $fileB

    # --- 2. -Run A: header, one line per record in ts order, monotonic offsets, parents, truncation
    $r = Invoke-Tool @('-Run', 'AAA111', '-LogRoot', $root)
    Assert-Equal 0 $r.Code '-Run AAA111 exits 0'
    $joined = $r.Out -join "`n"
    Assert-True ($joined -match 'run AAA111\s+pid 4001') 'header line names the run id and pid'

    $bodyLines = @($r.Out | Where-Object { $_ -match '^\d{2}:\d{2}:\d{2}\.\d{3}\s+\+' })
    Assert-Equal 4 $bodyLines.Count 'one line per record (start/decision/ui/exit)'

    $stages = @(); $offsets = @()
    foreach ($line in $bodyLines) {
        if ($line -match '^\d{2}:\d{2}:\d{2}\.\d{3}\s+\+\s*([\d.]+)s\s+(\S+)') {
            $offsets += [double]$Matches[1]
            $stages += $Matches[2]
        }
    }
    Assert-Equal 'start,decision,ui,exit' ($stages -join ',') 'records print in ts order'
    $monotonic = $true
    for ($i = 1; $i -lt $offsets.Count; $i++) { if ($offsets[$i] -lt $offsets[$i - 1]) { $monotonic = $false } }
    Assert-True $monotonic '+seconds offsets are monotonic'

    Assert-True ($joined -match 'explorer#100 <- wt#200') 'parents render as name#pid <- name#pid'

    if ($joined -match 'note=(x+)(…)') { $noteLen = $Matches[1].Length } else { $noteLen = -1 }
    Assert-Equal 160 $noteLen 'a value over 160 chars is truncated to 160 chars plus an ellipsis'

    # --- 3. -Last vs -Last -Any diverge: A reached the UI, B is merely the newest overall
    $rLast = Invoke-Tool @('-Last', '-LogRoot', $root)
    Assert-Equal 0 $rLast.Code '-Last exits 0'
    Assert-True (($rLast.Out -join "`n") -match 'run AAA111') '-Last (no -Any) picks the newest run that reached the UI'

    $rLastAny = Invoke-Tool @('-Last', '-Any', '-LogRoot', $root)
    Assert-Equal 0 $rLastAny.Code '-Last -Any exits 0'
    Assert-True (($rLastAny.Out -join "`n") -match 'run BBB222') '-Last -Any picks the newest run overall'

    # --- 4. Exit-code contract: 1 = run not found / no selection, 2 = no directory / no records
    $rNope = Invoke-Tool @('-Run', 'nope', '-LogRoot', $root)
    Assert-Equal 1 $rNope.Code '-Run nope exits 1'
    Assert-True (($rNope.Out -join "`n") -match 'run not found') '-Run nope reports "run not found"'

    $missingRoot = Join-Path $env:TEMP ("no-such-launcher-logs-" + [guid]::NewGuid().ToString('N'))
    $rMissing = Invoke-Tool @('-Run', 'AAA111', '-LogRoot', $missingRoot)
    Assert-Equal 2 $rMissing.Code 'a missing -LogRoot directory exits 2'

    $garbageRoot = Join-Path $env:TEMP ("garbage-launcher-logs-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force $garbageRoot | Out-Null
    try {
        Set-Content -LiteralPath (Join-Path $garbageRoot 'claude-auto-2026-09-01.jsonl') -Value @('not json at all', '')
        $rGarbage = Invoke-Tool @('-Last', '-LogRoot', $garbageRoot)
        Assert-Equal 2 $rGarbage.Code 'a -LogRoot with only garbage/blank lines exits 2'
    } finally { Remove-Item -LiteralPath $garbageRoot -Recurse -Force -ErrorAction SilentlyContinue }

} finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

if ($script:Failed -gt 0) {
    Write-Host "$script:Failed assertion(s) failed" -ForegroundColor Red
    exit 1
}
Write-Host "$script:Ran assertions, all pass" -ForegroundColor Green
exit 0
