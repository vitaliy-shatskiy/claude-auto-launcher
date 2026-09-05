# Launcher configuration: ~/.claude/claude-auto.json (CLAUDE_AUTO_CONFIG overrides). No file = defaults.
# Every validation failure falls back to the default for THAT key and adds a warning; nothing throws.
$script:AllowedTints = @('Green', 'Magenta', 'Cyan', 'Blue', 'Yellow', 'Red')
$script:AllowedRiderModes = @('auto', 'on', 'off')

function Get-LauncherConfigPath {
    if ($env:CLAUDE_AUTO_CONFIG) { return (Expand-LauncherPath $env:CLAUDE_AUTO_CONFIG) }
    return (Join-Path $HOME '.claude\claude-auto.json')
}

function Expand-LauncherPath {
    param([string]$Path)
    if (-not $Path) { return $Path }
    if ($Path -eq '~') { $Path = $HOME }
    elseif ($Path -match '^~[\\/]') { $Path = Join-Path $HOME $Path.Substring(2) }
    try { return [IO.Path]::GetFullPath($Path) } catch { return $Path }
}

function Get-LauncherDefaults {
    return [pscustomobject]@{
        Accounts = @([pscustomobject]@{ Key = 'work'; Root = (Join-Path $HOME '.claude'); Label = 'work account'; Tint = 'Green'; Hidden = $false; Canonical = $true; Rooted = $true })
        Sharing = $false; Remote = $false; RiderMcp = 'auto'
        ExtraMcpConfigs = @((Join-Path $HOME '.claude\mcp-shared.json'))
        # The credential store: one file per environment variable, tiers shared -> org -> project
        # (Import-ProjectSecrets in Env.ps1). A directory beside the profile, never inside it.
        SecretsRoot = (Join-Path $HOME '.claude-secrets')
        LaunchHooks = @(); MaintenanceActions = @(); Warnings = @(); Path = (Get-LauncherConfigPath)
    }
}

function ConvertTo-LauncherRoster {
    # Returns @{ Accounts = ...; Warnings = ... }. Any structural failure returns the default roster.
    param($Raw)
    $warnings = @()
    $canonicalRoot = Join-Path $HOME '.claude'
    $accounts = @()
    foreach ($a in @($Raw)) {
        $key = "$($a.key)".Trim()
        $rawRoot = "$($a.root)"
        # Judged BEFORE expansion: GetFullPath makes every path absolute, so the check would never fire after it.
        $rooted = ($rawRoot -match '^~([\\/]|$)') -or [IO.Path]::IsPathRooted($rawRoot)
        $root = Expand-LauncherPath $rawRoot
        $tint = if ($a.tint) { "$($a.tint)" } else { 'Green' }
        if ($tint -notin $script:AllowedTints) { $warnings += "account '$key': tint '$tint' is not one of $($script:AllowedTints -join ', '); using Green"; $tint = 'Green' }
        $accounts += [pscustomobject]@{
            Key = $key; Root = $root
            Label = if ($a.label) { "$($a.label)" } else { "$key account" }
            Tint = $tint; Hidden = [bool]$a.hidden
            Canonical = ($root -eq $canonicalRoot); Rooted = $rooted
        }
    }
    $fatal = $null
    if ($accounts.Count -eq 0) { $fatal = 'accounts is empty' }
    elseif (@($accounts | Where-Object { -not $_.Key -or $_.Key.Length -gt 8 }).Count) { $fatal = 'every account key must be 1 to 8 characters' }
    elseif (@($accounts.Key | Sort-Object -Unique).Count -ne $accounts.Count) { $fatal = 'account keys must be unique' }
    elseif (@($accounts.Key | ForEach-Object { $_.Substring(0, 1).ToLowerInvariant() } | Sort-Object -Unique).Count -ne $accounts.Count) { $fatal = 'account keys must start with different first letters (the fallback prompt is one letter per account)' }
    elseif (@($accounts | Where-Object { -not $_.Rooted }).Count) { $fatal = 'every account root must be an absolute path' }
    elseif (@($accounts | Where-Object Canonical).Count -ne 1) { $fatal = "exactly one account must have root $canonicalRoot (the canonical one)" }
    if ($fatal) {
        $warnings += "accounts: $fatal; using the default roster"
        return @{ Accounts = (Get-LauncherDefaults).Accounts; Warnings = $warnings }
    }
    return @{ Accounts = $accounts; Warnings = $warnings }
}

function ConvertTo-MaintenanceActions {
    param($Raw)
    $out = @(); $warnings = @()
    $reserved = @('u', 'r', 'd', 'm', 'p')
    foreach ($m in @($Raw)) {
        $key = "$($m.key)".Trim().ToLowerInvariant()
        if ($key -notmatch '^[a-z0-9]$' -or $key -in $reserved -or -not $m.script) { $warnings += "maintenanceActions: entry '$key' needs a one-letter key outside $($reserved -join ',') and a script; skipped"; continue }
        $out += [pscustomobject]@{ Key = $key; Label = if ($m.label) { "$($m.label)" } else { $key }; Script = (Expand-LauncherPath "$($m.script)"); ConfirmTwice = [bool]$m.confirmTwice }
    }
    return @{ Actions = $out; Warnings = $warnings }
}

function Read-LauncherConfig {
    param([string]$Path = (Get-LauncherConfigPath))
    $cfg = Get-LauncherDefaults
    $cfg.Path = $Path
    if (-not (Test-Path -LiteralPath $Path)) { return $cfg }
    try { $raw = Get-Content -LiteralPath $Path -Raw -ErrorAction Stop | ConvertFrom-Json }
    catch { $cfg.Warnings = @("config $Path could not be read or parsed ($($_.Exception.Message)); using defaults"); return $cfg }

    if ($null -ne $raw.accounts) { $r = ConvertTo-LauncherRoster $raw.accounts; $cfg.Accounts = @($r.Accounts); $cfg.Warnings += $r.Warnings }
    if ($null -ne $raw.sharing) { $cfg.Sharing = [bool]$raw.sharing }
    if ($null -ne $raw.remote)  { $cfg.Remote = [bool]$raw.remote }
    if ($null -ne $raw.riderMcp) {
        $mode = "$($raw.riderMcp)".ToLowerInvariant()
        if ($mode -in $script:AllowedRiderModes) { $cfg.RiderMcp = $mode } else { $cfg.Warnings += "riderMcp '$($raw.riderMcp)' is not auto/on/off; using auto" }
    }
    if ($null -ne $raw.extraMcpConfigs) { $cfg.ExtraMcpConfigs = @($raw.extraMcpConfigs | ForEach-Object { Expand-LauncherPath "$_" }) }
    if ($null -ne $raw.secretsRoot) {
        # Judged BEFORE expansion, same as the account roster's Rooted check: GetFullPath makes
        # every path absolute, so a relative value would never fail the check after it ran.
        $rawSecrets = "$($raw.secretsRoot)".Trim()
        if (-not $rawSecrets) {
            $cfg.Warnings += "secretsRoot is empty; using $($cfg.SecretsRoot)"
        } elseif (-not (($rawSecrets -match '^~([\\/]|$)') -or [IO.Path]::IsPathRooted($rawSecrets))) {
            $cfg.Warnings += 'secretsRoot must be an absolute path'
        } else {
            $cfg.SecretsRoot = Expand-LauncherPath $rawSecrets
        }
    }
    if ($null -ne $raw.launchHooks) { $cfg.LaunchHooks = @($raw.launchHooks | ForEach-Object { Expand-LauncherPath "$_" }) }
    if ($null -ne $raw.maintenanceActions) { $m = ConvertTo-MaintenanceActions $raw.maintenanceActions; $cfg.MaintenanceActions = @($m.Actions); $cfg.Warnings += $m.Warnings }
    if ($cfg.Sharing -and @($cfg.Accounts).Count -lt 2) { $cfg.Sharing = $false; $cfg.Warnings += 'sharing needs at least two accounts; off' }
    return $cfg
}
