#Requires -Version 7.4
# Demo home for the README recordings: a throwaway USERPROFILE under %TEMP%, the demo roster,
# remembered choices and usage numbers. Dot-source it from the repository root (the tapes do).
# Nothing real is read: no ~/.claude, no prefs, no projects of the recording machine.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3.0

$t = Join-Path ([IO.Path]::GetTempPath()) 'claude-auto-demo'
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

$env:USERPROFILE = $t
$env:CLAUDE_AUTO_CONFIG = (Resolve-Path -LiteralPath 'docs/media/demo-config.json').Path
$env:CLAUDE_AUTO_PREFS = Join-Path $t 'prefs.json'
$env:CLAUDE_AUTO_PROJECTS_ROOT = Join-Path $t 'projects'
$env:CLAUDE_AUTO_RATE_LIMITS_DIR = $limits
$env:CLAUDE_AUTO_NO_UPDATE = '1'
$env:CLAUDE_NO_ROAM = '1'
Remove-Item Env:NO_COLOR, Env:CLAUDE_CONFIG_DIR -ErrorAction SilentlyContinue

$targetDir = $env:CARGO_TARGET_DIR ?? 'target'
$script:demoExe = @('release', 'debug') | ForEach-Object { Join-Path -Path $targetDir -ChildPath $_ -AdditionalChildPath 'claude-auto.exe' } |
    Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
function Invoke-ClaudeAuto { & $script:demoExe @args }
Set-Alias -Name claude-auto -Value Invoke-ClaudeAuto
function prompt { '> ' }
