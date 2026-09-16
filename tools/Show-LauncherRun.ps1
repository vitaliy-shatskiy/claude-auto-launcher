# Prints one claude-auto launcher run from ~/.claude/launcher-logs as a timeline, so "what happened"
# is answerable without reading JSONL by eye. Read-only.
#   pwsh -File Show-LauncherRun.ps1 -Last            # the newest run that reached the UI (or any, with -Any)
#   pwsh -File Show-LauncherRun.ps1 -Run c090572c2b1a
# Exit: 0 printed · 1 run not found · 2 no log directory or unreadable log (not a pass).
[CmdletBinding()]
param(
    [string]$Run,
    [switch]$Last,
    [switch]$Any,
    [string]$LogRoot = (Join-Path $HOME '.claude\launcher-logs'),
    [int]$Days = 3
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $LogRoot)) { Write-Host "no log directory: $LogRoot"; exit 2 }
$files = Get-ChildItem -LiteralPath $LogRoot -Filter 'claude-auto-*.jsonl' | Sort-Object Name -Descending | Select-Object -First $Days
if (-not $files) { Write-Host "no log files under $LogRoot"; exit 2 }
$records = @()
foreach ($f in $files) {
    foreach ($line in [IO.File]::ReadAllLines($f.FullName)) {
        if (-not $line.Trim()) { continue }
        try { $records += ($line | ConvertFrom-Json -AsHashtable) } catch { }
    }
}
if (-not $records) { Write-Host 'no readable records'; exit 2 }
if (-not $Run) {
    if (-not $Last) { Write-Host 'give -Run <id> or -Last'; exit 1 }
    $candidates = $records | Where-Object { $_.run } | Sort-Object { [datetime]$_.ts } -Descending
    if (-not $Any) { $uiRuns = ($candidates | Where-Object { $_.stage -eq 'ui' -or ($_.stage -eq 'decision' -and $_.useUi) } | Select-Object -ExpandProperty run -Unique); $candidates = $candidates | Where-Object { $uiRuns -contains $_.run } }
    $Run = ($candidates | Select-Object -First 1).run
    if (-not $Run) { Write-Host 'no run found'; exit 1 }
}
$mine = @($records | Where-Object { $_.run -eq $Run } | Sort-Object { [datetime]$_.ts })
if ($mine.Count -eq 0) { Write-Host "run not found: $Run"; exit 1 }
$t0 = [datetime]$mine[0].ts
Write-Host ("run {0}  pid {1}  {2:yyyy-MM-dd HH:mm:ss} (machine clock)" -f $Run, $mine[0].pid, $t0)
$skip = @('ts', 'stage', 'run', 'pid')
foreach ($r in $mine) {
    $t = [datetime]$r.ts
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
        if ($s.Length -gt 160) { $s = $s.Substring(0, 160) + '…' }
        $parts += "$k=$s"
    }
    Write-Host ("{0:HH:mm:ss.fff} +{1,6:0.0}s  {2,-9} {3}" -f $t, ($t - $t0).TotalSeconds, $r.stage, ($parts -join '  '))
}
exit 0
