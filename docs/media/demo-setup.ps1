#Requires -Version 7.4
# Demo home for the README recordings: a throwaway USERPROFILE under %TEMP%, the demo roster,
# remembered choices and usage numbers. Dot-source it from the repository root (the tapes do).
# Nothing real is read: no ~/.claude, no prefs, no projects of the recording machine.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0

$t = Join-Path ([IO.Path]::GetTempPath()) 'claude-auto-demo'
# Drives an earlier run substituted for the demo projects are dropped first.
foreach ($line in @(subst)) {
    if ($line -like "*=> $t\*") { subst $line.Substring(0, 2) /D }
}
if (Test-Path -LiteralPath $t) { Remove-Item -LiteralPath $t -Recurse -Force }
$limits = Join-Path $t 'rate-limits'
New-Item -ItemType Directory -Force -Path (Join-Path $t '.claude'), $limits | Out-Null
$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()

# Remembered choices, saved two hours ago (the screen prints their age).
$prefs = Get-Content -Raw -LiteralPath 'docs/media/demo-prefs.json' | ConvertFrom-Json -AsHashtable
$prefs.SavedAtMs = $now - 2 * 3600000
foreach ($p in $prefs.Profiles.Values) { $p.SavedAtMs = $prefs.SavedAtMs }
[IO.File]::WriteAllText((Join-Path $t 'prefs.json'), ($prefs | ConvertTo-Json -Depth 5))

# Usage records as the statusline writes them (five-hour %, seven-day %), stamped a minute ago.
$at = $now - 60000
$usage = [ordered]@{ main = @(34, 61); team = @(12, 27); night = @(78, 45) }
foreach ($e in $usage.GetEnumerator()) {
    $body = [ordered]@{ fiveHour = $e.Value[0]; sevenDay = $e.Value[1]; atMs = $at } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText((Join-Path $limits "$($e.Key).json"), $body)
}

# The usage widget's record: older than the statusline's, carrying the per-model weekly bucket.
$widget = [ordered]@{ main = 12; night = 41 }
foreach ($e in $widget.GetEnumerator()) {
    $u = $usage[$e.Key]
    $body = [ordered]@{ fiveHour = $u[0]; sevenDay = $u[1]; atMs = $at - 120000; modelLabel = 'FABLE'; modelSevenDay = $e.Value } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText((Join-Path $limits "$($e.Key).widget.json"), $body)
}

# Demo projects on a substituted drive: the screens show short invented paths, and no project sits
# under the temp folder (the project screen hides scratch directories there).
$work = Join-Path $t 'work'
New-Item -ItemType Directory -Force -Path $work | Out-Null
$letter = @('P', 'Q', 'R', 'S', 'T', 'V', 'W', 'Y') | Where-Object { -not (Test-Path -LiteralPath "${_}:\") } | Select-Object -First 1
if (-not $letter) { throw 'demo-setup: no free drive letter for the demo projects' }
subst "${letter}:" $work
if ($LASTEXITCODE -ne 0) { throw "demo-setup: subst ${letter}: failed with $LASTEXITCODE" }
$env:CLAUDE_AUTO_DEMO_DRIVE = "${letter}:"

# Invented sessions from demo-sessions.json, each dated its minutes ago.
# The main account's own root: sharing is off in the demo roster, so its picker reads here.
$projectsRoot = Join-Path $t '.claude' -AdditionalChildPath 'projects'
$n = 0
foreach ($s in (Get-Content -Raw -LiteralPath 'docs/media/demo-sessions.json' | ConvertFrom-Json)) {
    $n++
    $cwd = "${letter}:\$($s.project)"
    $dir = Join-Path $projectsRoot "$letter--$($s.project)"
    New-Item -ItemType Directory -Force -Path $cwd, $dir | Out-Null
    $id = '{0:x8}-0000-4000-8000-{1:x12}' -f (0x5e551000 + $n), $n
    $when = [DateTimeOffset]::UtcNow.AddMinutes(-[double]$s.minutesAgo)
    $lines = foreach ($turn in $s.turns) {
        $role = $turn[0]
        $content = if ($role -eq 'user') { $turn[1] } else { , @([ordered]@{ type = 'text'; text = $turn[1] }) }
        [ordered]@{
            type = $role; cwd = $cwd; sessionId = $id; entrypoint = 'cli'; timestamp = $when.ToString('o')
            message = [ordered]@{ role = $role; content = $content }
        } | ConvertTo-Json -Compress -Depth 6
    }
    $file = Join-Path $dir "$id.jsonl"
    [IO.File]::WriteAllText($file, ($lines -join "`n") + "`n")
    (Get-Item -LiteralPath $file).LastWriteTimeUtc = $when.UtcDateTime
}

$env:USERPROFILE = $t
# A copy: the walk's row edits (ctrl+x, the o editor) save to the config.
$env:CLAUDE_AUTO_CONFIG = Join-Path $t 'config.json'
Copy-Item -LiteralPath 'docs/media/demo-config.json' -Destination $env:CLAUDE_AUTO_CONFIG
$env:CLAUDE_AUTO_PREFS = Join-Path $t 'prefs.json'
$env:CLAUDE_AUTO_PROJECTS_ROOT = $projectsRoot
$env:CLAUDE_AUTO_RATE_LIMITS_DIR = $limits
$env:CLAUDE_AUTO_NO_UPDATE = '1'
$env:CLAUDE_NO_ROAM = '1'
Remove-Item Env:NO_COLOR, Env:CLAUDE_CONFIG_DIR -ErrorAction SilentlyContinue

$targetDir = $env:CARGO_TARGET_DIR ?? 'target'
$script:demoExe = @('release', 'debug') | ForEach-Object { Join-Path -Path $targetDir -ChildPath $_ -AdditionalChildPath 'claude-auto.exe' } |
    Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1 | ForEach-Object { (Resolve-Path -LiteralPath $_).Path }
function Invoke-ClaudeAuto { & $script:demoExe @args }
Set-Alias -Name claude-auto -Value Invoke-ClaudeAuto
function prompt { '> ' }
