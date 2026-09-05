# One-time setup: checks pwsh 7 and claude, writes ~/.claude/claude-auto.json if absent, puts this
# folder on the user PATH so `claude-auto` works from any shell. Idempotent. `-Define` only loads
# the functions (tests).
[CmdletBinding()]
param([switch]$Define)

function Get-InstallRequirements {
    $pwsh = Get-Command pwsh -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $claude = Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    return [ordered]@{
        pwsh   = if ($pwsh -and $PSVersionTable.PSVersion.Major -ge 7) { "ok ($($PSVersionTable.PSVersion))" } else { 'MISSING - install PowerShell 7: winget install Microsoft.PowerShell' }
        claude = if ($claude) { "ok ($($claude.Source))" } else { 'MISSING - install Claude Code first (https://code.claude.com)' }
    }
}
function Write-DefaultLauncherConfig {
    param([Parameter(Mandatory)][string]$Path)
    if (Test-Path -LiteralPath $Path) { return 'kept' }
    New-Item -ItemType Directory -Force (Split-Path $Path -Parent) | Out-Null
    $default = [ordered]@{
        accounts = @([ordered]@{ key = 'work'; root = '~/.claude'; label = 'work account'; tint = 'Green' })
        sharing = $false; remote = $false; riderMcp = 'auto'
        extraMcpConfigs = @('~/.claude/mcp-shared.json'); launchHooks = @(); maintenanceActions = @()
    }
    [IO.File]::WriteAllText($Path, ($default | ConvertTo-Json -Depth 5) + "`n")
    return 'created'
}
function Add-PathEntry {
    param([string]$Current, [Parameter(Mandatory)][string]$Entry)
    $parts = @(($Current -split ';') | Where-Object { $_ })
    if ($parts | Where-Object { $_.TrimEnd('\') -ieq $Entry.TrimEnd('\') }) { return $Current }
    return (@($parts) + $Entry) -join ';'
}
if ($Define) { return }

$req = Get-InstallRequirements
$req.GetEnumerator() | ForEach-Object { Write-Host ("  {0,-7} {1}" -f $_.Key, $_.Value) }
if (($req.Values -join ' ') -match 'MISSING') { Write-Host 'fix the missing requirement and run install.ps1 again' -ForegroundColor Red; exit 1 }
Write-Host "  config  $(Write-DefaultLauncherConfig -Path (Join-Path $HOME '.claude\claude-auto.json')) $(Join-Path $HOME '.claude\claude-auto.json')"
# [Environment]::GetEnvironmentVariable('Path','User') EXPANDS %VAR% tokens on read, so writing
# that back through SetEnvironmentVariable(...,'User') replaces every %USERPROFILE%\... entry
# (the stock Windows user PATH ships one: ...\AppData\Local\Microsoft\WindowsApps) with a
# hardcoded absolute path - permanent, silent PATH damage on a machine this was never asked to
# touch. HKCU\Environment read/written directly, RAW (DoNotExpandEnvironmentNames) and with its
# own value kind preserved, so an untouched entry stays untouched. Anything that fails here - the
# key, the value, or its kind - must not write: fail safe, never guess a kind to write with.
$key = $null
$pathLine = "  PATH    could not be read; add $PSScriptRoot to your PATH by hand"
try {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    if (-not $key) { throw 'could not open HKCU\Environment' }
    $raw = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $kind = $key.GetValueKind('Path')
    $new = Add-PathEntry -Current $raw -Entry $PSScriptRoot
    if ($new -ne $raw) {
        $key.SetValue('Path', $new, $kind)
        # Setting the registry value does not update THIS process's environment or any other
        # already-open terminal - only a new one reads HKCU\Environment again.
        $pathLine = "  PATH    added $PSScriptRoot (only new terminals will see it)"
    } else {
        $pathLine = "  PATH    already there"
    }
} catch { } finally { if ($key) { $key.Close() } }
Write-Host $pathLine
Write-Host "  next    run: claude-auto   (Rider: Settings > Tools > Claude Code > Claude command = $PSScriptRoot\claude-auto.ps1)"
Write-Host "  tests   pwsh -File tests\check-regression.ps1 -Record   (once - the reference is gitignored, so checkpoint.ps1 reports regression 2 DID NOT RUN until this runs)"
exit 0
