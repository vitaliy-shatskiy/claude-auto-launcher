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
    $multiLineValue = "line-one`nline-two`nline-three"

    # Day files and their ts values are relative to "now", never hardcoded - a suite pinned to two
    # absolute dates goes red the day the -Days window rolls past them (N1). ANCIENT stays fixed: it
    # must always be older than any window this suite exercises.
    $today = (Get-Date).Date
    $yesterday = $today.AddDays(-1)
    function Fmt([datetime]$d) { $d.ToString('yyyy-MM-ddTHH:mm:ss') }

    $fileA = @(
        (@{ ts = (Fmt ($yesterday.AddHours(10))); stage = 'start'; run = 'AAA111'; pid = 4001; cmd = 'claude-auto.ps1'; prompt = $multiLineValue } | ConvertTo-Json -Compress),
        (@{ ts = (Fmt ($yesterday.AddHours(10).AddSeconds(2))); stage = 'decision'; run = 'AAA111'; pid = 4001; useUi = $true; account = 'work' } | ConvertTo-Json -Compress),
        '{not valid json at all',
        '',
        (@{ ts = (Fmt ($yesterday.AddHours(11))); stage = 'start'; run = 'BBB222'; pid = 4002; cmd = 'claude-auto.ps1' } | ConvertTo-Json -Compress),
        # D1 lives ONLY on yesterday's file - proves -Days 1 (today only) excludes it (N2).
        (@{ ts = (Fmt ($yesterday.AddHours(9))); stage = 'start'; run = 'D1'; pid = 6001; cmd = 'claude-auto.ps1' } | ConvertTo-Json -Compress)
        # A UI-stage run (item 3): screen transitions and the decisive keys between them. Dated
        # EARLIEST of everything in the window on purpose, so adding it cannot change which run
        # -Last or -Last -Any resolves to above.
        (@{ ts = (Fmt ($yesterday.AddHours(8))); stage = 'screen'; run = 'SCR777'; pid = 4005; name = 'project'; phase = 'enter'; rows = 7; index = 0; filterLength = 0 } | ConvertTo-Json -Compress),
        (@{ ts = (Fmt ($yesterday.AddHours(8).AddSeconds(3))); stage = 'key'; run = 'SCR777'; pid = 4005; screen = 'project'; key = 'r'; index = 2; action = 'resume' } | ConvertTo-Json -Compress),
        (@{ ts = (Fmt ($yesterday.AddHours(8).AddSeconds(4))); stage = 'screen'; run = 'SCR777'; pid = 4005; name = 'project'; phase = 'leave'; ms = 3200; rows = 7; index = 2; filterLength = 0 } | ConvertTo-Json -Compress)
    )
    Set-Content -LiteralPath (Join-Path $root ('claude-auto-{0:yyyy-MM-dd}.jsonl' -f $yesterday)) -Value $fileA

    $fileB = @(
        (@{ ts = (Fmt ($yesterday.AddHours(10).AddSeconds(5))); stage = 'ui'; run = 'AAA111'; pid = 4001;
            saved   = @{ account = 'work'; profile = 'default' };
            parents = @(@{ name = 'explorer'; pid = 100 }, @{ name = 'wt'; pid = 200 });
            note    = $longNote } | ConvertTo-Json -Compress -Depth 5),
        (@{ ts = (Fmt ($yesterday.AddHours(10).AddSeconds(20))); stage = 'exit'; run = 'AAA111'; pid = 4001; code = 0 } | ConvertTo-Json -Compress),
        (@{ ts = (Fmt ($yesterday.AddHours(11).AddSeconds(2))); stage = 'decision'; run = 'BBB222'; pid = 4002; useUi = $false; account = 'work' } | ConvertTo-Json -Compress),
        (@{ ts = (Fmt ($yesterday.AddHours(11).AddSeconds(10))); stage = 'exit'; run = 'BBB222'; pid = 4002; code = 0 } | ConvertTo-Json -Compress),
        # A torn record: unparsable ts. Must not crash the reader and must not silently pass as "run
        # not found" (1) - it has no usable timestamp, which is a "2", and it must not poison -Last.
        (@{ ts = 'not-a-timestamp'; stage = 'start'; run = 'TSBAD1'; pid = 4003 } | ConvertTo-Json -Compress),
        # A torn record: no ts key at all.
        (@{ stage = 'start'; run = 'TSNOKEY'; pid = 4004 } | ConvertTo-Json -Compress)
    )
    Set-Content -LiteralPath (Join-Path $root ('claude-auto-{0:yyyy-MM-dd}.jsonl' -f $today)) -Value $fileB

    # A run dated well outside any window this suite exercises, beside the two in-window files above.
    # Intentionally NOT relative to "now" - it stays ancient forever.
    Set-Content -LiteralPath (Join-Path $root 'claude-auto-2026-01-01.jsonl') -Value @(
        (@{ ts = '2026-01-01T09:00:00'; stage = 'start'; run = 'ANCIENT'; pid = 5001; cmd = 'claude-auto.ps1' } | ConvertTo-Json -Compress)
    )

    # An impossible calendar date in a filename (the regex shape matches, the date does not exist)
    # must not crash file selection for every healthy run sharing the directory (N3).
    Set-Content -LiteralPath (Join-Path $root 'claude-auto-2026-13-45.jsonl') -Value @(
        (@{ ts = '2026-01-01T00:00:00'; stage = 'start'; run = 'IMPOSSIBLE'; pid = 7001 } | ConvertTo-Json -Compress)
    )

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

    Assert-True ($joined -match 'prompt=line-one⏎line-two⏎line-three') 'a multi-line value collapses to one line with an <ENTER> glyph, not raw newlines'

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

    # --- 5. A torn ts must not crash the reader (raw PowerShell dump, exit 1) and must not poison
    # -Last for healthy runs. Contract: unusable ts -> 2, distinct from a genuinely absent run -> 1.
    $rTsBad = Invoke-Tool @('-Run', 'TSBAD1', '-LogRoot', $root)
    Assert-Equal 2 $rTsBad.Code '-Run TSBAD1 (unparsable ts) exits 2, not a raw-dump 1'
    Assert-True (($rTsBad.Out -join "`n") -notmatch 'Cannot convert value') 'no raw PowerShell type-conversion dump for a bad ts'

    $rTsNoKey = Invoke-Tool @('-Run', 'TSNOKEY', '-LogRoot', $root)
    Assert-Equal 2 $rTsNoKey.Code '-Run TSNOKEY (missing ts key) exits 2'

    $rLastAnyTorn = Invoke-Tool @('-Last', '-Any', '-LogRoot', $root)
    Assert-Equal 0 $rLastAnyTorn.Code '-Last -Any still succeeds with a torn record present in the window'
    Assert-True (($rLastAnyTorn.Out -join "`n") -match 'run BBB222') '-Last -Any still resolves to the healthy newest run, not derailed by TSBAD1/TSNOKEY'

    # --- 6. -Days is a real date window, and a bad -Days value is a clean parameter error.
    $rAncient = Invoke-Tool @('-Run', 'ANCIENT', '-LogRoot', $root)
    Assert-Equal 1 $rAncient.Code 'a run outside the default -Days window is "not found" (1), not silently served'
    Assert-True (($rAncient.Out -join "`n") -match 'not in the last \d+ day') 'the miss names the window, not a bare not-found'

    $windowRoot = Join-Path $env:TEMP ("launcher-run-window-test-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force $windowRoot | Out-Null
    try {
        Set-Content -LiteralPath (Join-Path $windowRoot 'claude-auto-2026-01-01.jsonl') -Value @(
            (@{ ts = '2026-01-01T09:00:00'; stage = 'start'; run = 'ANCIENT'; pid = 5001 } | ConvertTo-Json -Compress)
        )
        $rWindowEmpty = Invoke-Tool @('-Last', '-LogRoot', $windowRoot)
        Assert-Equal 2 $rWindowEmpty.Code 'a -LogRoot with only files outside the -Days window exits 2 (no files in window)'
    } finally { Remove-Item -LiteralPath $windowRoot -Recurse -Force -ErrorAction SilentlyContinue }

    $rBadDays = Invoke-Tool @('-Last', '-LogRoot', $root, '-Days', '-1')
    Assert-Equal 1 $rBadDays.Code '-Days -1 is a clean parameter-validation error (documented exit 1), not a -First -1 binding crash'
    Assert-True (($rBadDays.Out -join "`n") -notmatch 'Select-Object') 'no raw Select-Object -First -1 error text'

    # --- N2: -Days N must cover exactly N calendar days (today .. today-(N-1)), never N+1.
    $rDays1 = Invoke-Tool @('-Run', 'D1', '-LogRoot', $root, '-Days', '1')
    Assert-Equal 1 $rDays1.Code "-Days 1 excludes yesterday's file (D1 only lives there)"
    Assert-True (($rDays1.Out -join "`n") -match 'not in the last 1 day') 'the -Days 1 miss names the 1-day window'

    # --- N1 proof: AAA111 is dated yesterday, never a hardcoded calendar date - it must resolve at
    # the tightest window that still covers it (2 days), today included.
    $rBoundary = Invoke-Tool @('-Run', 'AAA111', '-LogRoot', $root, '-Days', '2')
    Assert-Equal 0 $rBoundary.Code 'AAA111 (dated yesterday) resolves at the exact 2-day window it needs'

    # --- N3: an impossible filename date (claude-auto-2026-13-45.jsonl) has sat beside every fixture
    # file since setup - every -Run/-Last call above already ran with it present. Confirm none of them
    # dumped a raw Where-Object error instead of answering.
    Assert-True (($r.Out -join "`n") -notmatch 'Where-Object') 'no raw Where-Object dump from the impossible filename date (-Run)'
    Assert-True (($rLast.Out -join "`n") -notmatch 'Where-Object') 'no raw Where-Object dump from the impossible filename date (-Last)'

    # --- 6b. The UI stages read as a sentence, one line each (item 3). A `screen` record whose two
    # leading fields print as name=project phase=enter is a dump; the point of this tool is that a
    # run can be read by eye, and the timeline is mostly these records now.
    $rScr = Invoke-Tool @('-Run', 'SCR777', '-LogRoot', $root)
    Assert-Equal 0 $rScr.Code '-Run SCR777 (a UI-stage run) exits 0'
    $scrJoined = $rScr.Out -join "`n"
    $scrBody = @($rScr.Out | Where-Object { $_ -match '^\d{2}:\d{2}:\d{2}\.\d{3}\s+\+' })
    Assert-Equal 3 $scrBody.Count 'one line per UI record, no more'
    # \s+ between FIELDS (the tool separates them by two spaces, as it always has); the single space
    # inside 'project enter' is the assertion - those two are one phrase, not two fields.
    Assert-True ($scrJoined -match 'screen\s+project enter\s+rows=7\s+index=0\s+filterLength=0') 'a screen record renders as "screen  project enter rows=… index=…", in that field order'
    Assert-True ($scrJoined -match 'screen\s+project leave\s+ms=3200\s+rows=7\s+index=2') 'and a leave record leads with how long the screen was up'
    Assert-True ($scrJoined -match 'key\s+project r\s+index=2\s+action=resume') 'a key record renders as "key  <screen> <key>", then what it changed'
    Assert-True ($scrJoined -notmatch 'name=project') 'the leading fields print BARE - no name= on a screen record'
    Assert-True ($scrJoined -notmatch 'phase=') 'and no phase= either'
    Assert-True ($scrJoined -notmatch 'screen=project') 'nor screen= on a key record'

    # --- 6c. An ERROR-ONLY run has to be reachable (review W3). A module that fails to load takes the
    # launcher out through the bare-session fallback: it never writes a ui record and never reaches
    # the decision, so -Last used to skip it and the only trace of that failure was findable by an id
    # nobody had. Its own root, so this cannot shift which run -Last picks anywhere else.
    $modRoot = Join-Path $env:TEMP ("modload-launcher-logs-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force $modRoot | Out-Null
    try {
        Set-Content -LiteralPath (Join-Path $modRoot ('claude-auto-{0:yyyy-MM-dd}.jsonl' -f $today)) -Value @(
            (@{ ts = (Fmt ($today.AddHours(7))); stage = 'start'; run = 'MOD999'; pid = 8001; cmd = 'claude-auto.ps1' } | ConvertTo-Json -Compress),
            (@{ ts = (Fmt ($today.AddHours(7).AddSeconds(1))); stage = 'error'; run = 'MOD999'; pid = 8001;
                where = 'module-load'; module = 'Env.ps1'; type = 'ParseException'; message = 'missing closing brace' } | ConvertTo-Json -Compress)
        )
        $rMod = Invoke-Tool @('-Last', '-LogRoot', $modRoot)
        Assert-Equal 0 $rMod.Code '-Last reaches a run whose only records are start and a module-load error'
        $modJoined = $rMod.Out -join "`n"
        Assert-True ($modJoined -match 'run MOD999') 'and names it'
        Assert-True ($modJoined -match 'where=module-load') 'rendering the failure'
        Assert-True ($modJoined -match 'module=Env\.ps1') 'with the module that caused it'
    } finally { Remove-Item -LiteralPath $modRoot -Recurse -Force -ErrorAction SilentlyContinue }

    # --- 7. The reader must not lock the file against a concurrent launcher append (item 6): open
    # it the same way the tool now does (Read, sharing ReadWrite) and prove a live append still works.
    $concurrentFile = Join-Path $root ('claude-auto-{0:yyyy-MM-dd}.jsonl' -f $today)
    $handle = [IO.File]::Open($concurrentFile, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    try {
        $appendOk = $true
        try { [IO.File]::AppendAllText($concurrentFile, '') } catch { $appendOk = $false }
        Assert-True $appendOk 'a concurrent append succeeds while a FileShare.ReadWrite read handle is open'
    } finally { $handle.Dispose() }

} finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

if ($script:Failed -gt 0) {
    Write-Host "$script:Failed assertion(s) failed" -ForegroundColor Red
    exit 1
}
Write-Host "$script:Ran assertions, all pass" -ForegroundColor Green
exit 0
