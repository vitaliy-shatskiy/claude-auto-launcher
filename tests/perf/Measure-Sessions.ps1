# Measures Get-ClaudeSessions against a REAL projects root, read-only.
#
# Cold = a scratch cache path that does not exist yet (the launcher's first run after any edit to
# Sessions.ps1, since the cache key carries the module's identity). Warm = a second call with the
# same scratch cache. Each run happens in its OWN pwsh child, because a launcher start is a fresh
# process: keeping three runs in one process would measure a JIT-warm second call as if it were
# cold. The median of -Runs runs is reported.
#
# The projects root is only ever READ. The scratch cache lives in TEMP and is deleted before each
# cold call; the real cache file beside the projects root is never touched, and its LastWriteTime
# is asserted unchanged around the whole measurement - if it moved, this script says so and exits 1.
[CmdletBinding()]
param(
    [string]$ProjectsRoot = (Join-Path $HOME '.claude\projects'),
    [int]$Runs = 3,
    [int]$Limit = 40,
    [int]$Skip = 0,
    [string]$Step = 'baseline',
    # Set by the parent when it spawns a child; not for hand use.
    [switch]$Child
)

$modules = @('Sessions.ps1', 'Projects.ps1')
$moduleDir = Join-Path (Split-Path -Path (Split-Path -Path $PSScriptRoot)) 'claude-auto'

function Invoke-OneRun {
    param([string]$Root, [int]$Limit, [int]$Skip)
    foreach ($m in $modules) { . (Join-Path $moduleDir $m) }
    $cache = Join-Path ([IO.Path]::GetTempPath()) ("claude-auto-perf-" + [guid]::NewGuid().ToString('N') + '.json')
    if (Test-Path -LiteralPath $cache) { Remove-Item -LiteralPath $cache -Force }

    # -Skip only exists once paging lands; passing it unconditionally would make this script
    # unrunnable against an older Sessions.ps1, which is the one thing a baseline must survive.
    $extra = @{}
    if ((Get-Command Get-ClaudeSessions).Parameters.ContainsKey('Skip')) { $extra['Skip'] = $Skip }

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $cold = @(Get-ClaudeSessions -ProjectsRoot $Root -Limit $Limit -CachePath $cache @extra)
    $sw.Stop(); $coldMs = $sw.Elapsed.TotalMilliseconds

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $warm = @(Get-ClaudeSessions -ProjectsRoot $Root -Limit $Limit -CachePath $cache @extra)
    $sw.Stop(); $warmMs = $sw.Elapsed.TotalMilliseconds

    Remove-Item -LiteralPath $cache -Force -ErrorAction SilentlyContinue
    return [pscustomobject]@{ Cold = $coldMs; Warm = $warmMs; Rows = $cold.Count; WarmRows = $warm.Count }
}

if ($Child) {
    $r = Invoke-OneRun -Root $ProjectsRoot -Limit $Limit -Skip $Skip
    Write-Output ("RESULT {0} {1} {2} {3}" -f $r.Cold, $r.Warm, $r.Rows, $r.WarmRows)
    exit 0
}

if (-not (Test-Path -LiteralPath $ProjectsRoot)) {
    Write-Host "projects root not found: $ProjectsRoot"; exit 2
}

# The launcher's own cache file for this root. Read its stamp before and after: this script must
# leave the owner's cache exactly as it found it.
$realCache = Join-Path (Split-Path -Path $ProjectsRoot) 'claude-auto-sessions.json'
$stampBefore = if (Test-Path -LiteralPath $realCache) { (Get-Item -LiteralPath $realCache).LastWriteTimeUtc.Ticks } else { 'absent' }

$colds = @(); $warms = @(); $rows = 0
for ($i = 0; $i -lt $Runs; $i++) {
    $out = & pwsh -NoProfile -File $PSCommandPath -Child -ProjectsRoot $ProjectsRoot -Limit $Limit -Skip $Skip 2>&1
    $line = @($out | Where-Object { "$_" -match '^RESULT ' })
    if ($line.Count -ne 1) {
        Write-Host "run $i produced no RESULT line:"; $out | ForEach-Object { Write-Host "  $_" }; exit 2
    }
    $parts = "$($line[0])" -split ' '
    $colds += [double]$parts[1]; $warms += [double]$parts[2]; $rows = [int]$parts[3]
}

$stampAfter = if (Test-Path -LiteralPath $realCache) { (Get-Item -LiteralPath $realCache).LastWriteTimeUtc.Ticks } else { 'absent' }
if ("$stampBefore" -ne "$stampAfter") {
    Write-Host "the real cache file changed during the measurement ($realCache) - refusing to report a number"; exit 1
}

function Get-Median { param([double[]]$Values) $s = @($Values | Sort-Object); return $s[[int][Math]::Floor($s.Count / 2)] }
$coldMed = Get-Median -Values $colds
$warmMed = Get-Median -Values $warms

Write-Host ("{0,-28} | {1,8} | {2,8} | rows" -f 'step', 'cold ms', 'warm ms')
Write-Host ("{0,-28} | {1,8:N0} | {2,8:N0} | {3}" -f $Step, $coldMed, $warmMed, $rows)
Write-Host ("  cold runs: " + (($colds | ForEach-Object { '{0:N0}' -f $_ }) -join ', ') +
            "   warm runs: " + (($warms | ForEach-Object { '{0:N0}' -f $_ }) -join ', '))
exit 0
