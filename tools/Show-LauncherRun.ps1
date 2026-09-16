# Prints one claude-auto launcher run from ~/.claude/launcher-logs as a timeline, so "what happened"
# is answerable without reading JSONL by eye. Read-only, tolerant of a concurrent writer.
#   pwsh -File Show-LauncherRun.ps1 -Last            # the newest run that reached the UI (or any, with -Any)
#   pwsh -File Show-LauncherRun.ps1 -Run c090572c2b1a
# Exit: 0 printed · 1 run not found (absent, or outside -Days) · 2 no log directory/files in the
# window, no readable records, or the selected run has no record with a usable timestamp - not a pass.
[CmdletBinding()]
param(
    [string]$Run,
    [switch]$Last,
    [switch]$Any,
    [string]$LogRoot = (Join-Path $HOME '.claude\launcher-logs'),
    [ValidateRange(1, 365)][int]$Days = 3
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $LogRoot)) { Write-Host "no log directory: $LogRoot"; exit 2 }

# -Days is a DATE window (files named claude-auto-<yyyy-MM-dd>.jsonl), not "N newest files": a run
# that fell out of the window must be reported as such, never silently denied or silently served.
function Get-LogFiles([string]$Root, [datetime]$CutoffDate) {
    Get-ChildItem -LiteralPath $Root -Filter 'claude-auto-*.jsonl' | Where-Object {
        if ($_.BaseName -match 'claude-auto-(\d{4}-\d{2}-\d{2})$') { ([datetime]$Matches[1]) -ge $CutoffDate } else { $false }
    }
}

# A concurrent launcher keeps appending to today's file: FileShare.ReadWrite so our read never
# blocks that append (plain [IO.File]::ReadAllLines opens with FileShare.Read only, and does).
function Read-LogRecords([System.IO.FileInfo[]]$Files) {
    $raw = @()
    foreach ($f in $Files) {
        $stream = [IO.File]::Open($f.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
        try {
            $reader = New-Object IO.StreamReader($stream, [System.Text.Encoding]::UTF8, $true)
            try {
                while (-not $reader.EndOfStream) {
                    $line = $reader.ReadLine()
                    if (-not $line.Trim()) { continue }
                    try { $raw += ($line | ConvertFrom-Json -AsHashtable) } catch { }
                }
            } finally { $reader.Dispose() }
        } finally { $stream.Dispose() }
    }
    return $raw
}

$cutoff = (Get-Date).Date.AddDays(-$Days)
$files = @(Get-LogFiles -Root $LogRoot -CutoffDate $cutoff)
if (-not $files) { Write-Host "no log files under $LogRoot in the last $Days day(s)"; exit 2 }

$rawRecords = Read-LogRecords -Files $files
if (-not $rawRecords) { Write-Host 'no readable records'; exit 2 }

# `-as` never throws on a torn ts (unparsable or absent) - it yields $null, which is dropped from
# the timestamp-sorted pool instead of taking the whole query down (a bad record must not poison
# -Last for every healthy run sharing the window).
foreach ($rec in $rawRecords) { $rec['_ts'] = $rec.ts -as [datetime] }
$records = @($rawRecords | Where-Object { $null -ne $_._ts })
if (-not $records) { Write-Host 'no readable records (every record had an unusable timestamp)'; exit 2 }

if (-not $Run) {
    if (-not $Last) { Write-Host 'give -Run <id> or -Last'; exit 1 }
    $candidates = $records | Where-Object { $_.run } | Sort-Object { $_._ts } -Descending
    if (-not $Any) { $uiRuns = ($candidates | Where-Object { $_.stage -eq 'ui' -or ($_.stage -eq 'decision' -and $_.useUi) } | Select-Object -ExpandProperty run -Unique); $candidates = $candidates | Where-Object { $uiRuns -contains $_.run } }
    $Run = ($candidates | Select-Object -First 1).run
    if (-not $Run) { Write-Host 'no run found'; exit 1 }
}
$mine = @($records | Where-Object { $_.run -eq $Run } | Sort-Object { $_._ts })
if ($mine.Count -eq 0) {
    if ($rawRecords | Where-Object { $_.run -eq $Run }) { Write-Host "run has no usable timestamp: $Run"; exit 2 }
    $outsideWindow = @(Get-LogFiles -Root $LogRoot -CutoffDate ([datetime]::MinValue) | Where-Object { $files.FullName -notcontains $_.FullName })
    if ($outsideWindow -and (Read-LogRecords -Files $outsideWindow | Where-Object { $_.run -eq $Run })) {
        Write-Host "run not found: $Run (not in the last $Days days)"; exit 1
    }
    Write-Host "run not found: $Run"; exit 1
}
$t0 = $mine[0]._ts
Write-Output ("run {0}  pid {1}  {2:yyyy-MM-dd HH:mm:ss} (machine clock)" -f $Run, $mine[0].pid, $t0)
$skip = @('ts', 'stage', 'run', 'pid', '_ts')
foreach ($r in $mine) {
    $t = $r._ts
    $parts = @()
    foreach ($k in $r.Keys) {
        if ($skip -contains $k) { continue }
        $v = $r[$k]
        if ($null -eq $v) { continue }
        if ($k -eq 'parents' -and $v -is [System.Collections.IList]) { $v = ($v | ForEach-Object { "$($_.name)#$($_.pid)" }) -join ' <- ' }
        elseif ($k -eq 'env' -and $v -is [System.Collections.IDictionary]) { $v = (($v.Keys | Where-Object { $null -ne $v[$_] } | ForEach-Object { "$_=$($v[$_])" }) -join ' ') }
        elseif ($v -is [System.Collections.IList]) { $v = ($v -join ' ') }
        elseif ($v -is [System.Collections.IDictionary]) { $v = ($v | ConvertTo-Json -Compress -Depth 4) }
        $s = [string]$v
        # A value with embedded CR/LF (a multi-line prompt, an args blob) must not break the
        # one-record-one-line contract: collapse it to a glyph before the 160-char truncation.
        $s = $s -replace "`r`n", ([string][char]0x23CE) -replace "`n", ([string][char]0x23CE) -replace "`r", ([string][char]0x23CE)
        if ($s.Length -gt 160) { $s = $s.Substring(0, 160) + '…' }
        $parts += "$k=$s"
    }
    Write-Output ("{0:HH:mm:ss.fff} +{1,6:0.0}s  {2,-9} {3}" -f $t, ($t - $t0).TotalSeconds, $r.stage, ($parts -join '  '))
}
exit 0
