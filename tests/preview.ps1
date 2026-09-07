# Drives claude-auto through its preview seam with scripted keys and prints what WOULD launch.
#
# Why this exists: this was run about ten times by hand during the v2 rebuild, and the preview path
# WRITES ~/.claude/claude-auto-prefs.json - which is how the owner's remembered model/effort/
# permission choices were silently replaced by probe values, and how a crash mid-run (or simply
# forgetting this script exists) could leave them corrupted even with a backup/restore in the way.
# Fixed at the source instead: CLAUDE_AUTO_PREFS (Prefs.ps1's Get-LaunchPrefsPath) redirects the
# whole read/save path to a throwaway file for the run, so the real prefs file is never opened at
# all - not backed up, not restored, not touched on a friend's machine that has none.
#
# CLAUDE_NO_ROAM=1 is always set so a probe can never start the companion server or a real session.

[CmdletBinding()]
param(
    # Comma-separated .NET ConsoleKey names, or bare characters: 'DownArrow,RightArrow,Enter', 'r,Enter'
    [Parameter(Mandatory)][string]$Keys,
    [string]$Launcher = (Join-Path $PSScriptRoot '..\claude-auto.ps1'),
    # Print the launcher's whole output instead of just the summary lines
    [switch]$Full
)

if (-not (Test-Path $Launcher)) { Write-Host "launcher not found: $Launcher" -ForegroundColor Red; exit 2 }

$fakePrefs = Join-Path $env:TEMP ('claude-auto-prefs.probe-' + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.json')

$saved = @{
    CLAUDE_AUTO_PREVIEW      = $env:CLAUDE_AUTO_PREVIEW
    CLAUDE_AUTO_PREVIEW_KEYS = $env:CLAUDE_AUTO_PREVIEW_KEYS
    CLAUDE_NO_ROAM           = $env:CLAUDE_NO_ROAM
    CLAUDE_AUTO_PREFS        = $env:CLAUDE_AUTO_PREFS
}

try {
    $env:CLAUDE_AUTO_PREVIEW = '1'
    $env:CLAUDE_NO_ROAM = '1'
    $env:CLAUDE_AUTO_PREVIEW_KEYS = $Keys
    $env:CLAUDE_AUTO_PREFS = $fakePrefs

    $out = & pwsh -NoProfile -File $Launcher 2>&1
    $code = $LASTEXITCODE

    if ($Full) {
        $out | ForEach-Object { $_ }
    } else {
        $wanted = 'launch args|command\s*:|remote\s*:|account\s*:|picker cancelled|remote off for this session|crc '
        $lines = @($out | Select-String -Pattern $wanted)
        if ($lines) { $lines | ForEach-Object { $_.Line.TrimEnd() } }
        else { Write-Host '  (no summary lines - rerun with -Full to see everything)' -ForegroundColor DarkGray }
    }
    Write-Host "  launcher exit: $code" -ForegroundColor DarkGray
} finally {
    foreach ($k in $saved.Keys) {
        if ($null -eq $saved[$k]) { Remove-Item "Env:$k" -ErrorAction SilentlyContinue }
        else { Set-Item "Env:$k" $saved[$k] }
    }
    Remove-Item -LiteralPath $fakePrefs -Force -ErrorAction SilentlyContinue
}
