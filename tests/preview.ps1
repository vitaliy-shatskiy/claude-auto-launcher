# Drives claude-auto through its preview seam with scripted keys and prints what WOULD launch.
#
# Why this exists: this was run about ten times by hand during the v2 rebuild, and the preview path
# WRITES ~/.claude/claude-auto-prefs.json - which is how the owner's remembered model/effort/
# permission choices were silently replaced by probe values. The backup here is a file COPY restored
# by copy: re-saving through the launcher's own serializer does not reproduce the bytes, because it
# writes an unordered hashtable whose JSON key order varies between processes.
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

$prefs = Join-Path $HOME '.claude\claude-auto-prefs.json'
$backup = Join-Path $env:TEMP "claude-auto-prefs.probe-backup.json"
$had = Test-Path $prefs
if ($had) { Copy-Item $prefs $backup -Force }

$saved = @{
    CLAUDE_AUTO_PREVIEW      = $env:CLAUDE_AUTO_PREVIEW
    CLAUDE_AUTO_PREVIEW_KEYS = $env:CLAUDE_AUTO_PREVIEW_KEYS
    CLAUDE_NO_ROAM           = $env:CLAUDE_NO_ROAM
}

try {
    $env:CLAUDE_AUTO_PREVIEW = '1'
    $env:CLAUDE_NO_ROAM = '1'
    $env:CLAUDE_AUTO_PREVIEW_KEYS = $Keys

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
    # Restore by copy, never by re-saving through the launcher - see the header.
    if ($had) {
        Copy-Item $backup $prefs -Force
        $same = (Get-FileHash $prefs).Hash -eq (Get-FileHash $backup).Hash
        if (-not $same) { Write-Host '  WARNING: prefs restore did not match its backup' -ForegroundColor Red }
    } elseif (Test-Path $prefs) {
        # The probe created a prefs file where the owner had none. Leave it, but say so - deleting
        # something the owner may now want is worse than one line of output.
        Write-Host "  note: this probe CREATED $prefs (there was none before)" -ForegroundColor Yellow
    }
}
