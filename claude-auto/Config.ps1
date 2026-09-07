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
    # A relative value is resolved against $PWD explicitly: GetFullPath($Path) alone reads
    # [Environment]::CurrentDirectory, which Set-Location never updates, so a relative
    # CLAUDE_AUTO_CONFIG, root, hook or extra could silently resolve somewhere the caller never
    # navigated to. The two-argument overload resolves relative paths against the base given and
    # leaves an already-rooted path untouched.
    try { return [IO.Path]::GetFullPath($Path, $PWD.Path) } catch { return $Path }
}

function ConvertTo-LauncherBool {
    # A JSON boolean survives ConvertFrom-Json as [bool]; a quoted "true"/"false" survives as
    # [string], and PowerShell's [bool] cast treats ANY non-empty string as $true - so
    # `"sharing": "false"` used to turn sharing ON. Real booleans and the two literal strings
    # (case-insensitively) are honoured; anything else keeps the caller's default, reported by
    # naming the field and the value so the failure is never silent.
    #
    # Returns @{ Value; Warning }, never mutates a caller's warning list itself: [ref] on a
    # PROPERTY (e.g. $cfg.Warnings) only wraps a snapshot of its current value in PowerShell, not
    # a live slot, so a callee-side append through it is silently lost. Handing the warning back
    # and letting each caller append it to ITS OWN variable sidesteps that trap entirely.
    param($Value, [bool]$Default, [string]$Name)
    if ($null -eq $Value) { return @{ Value = $Default; Warning = $null } }
    if ($Value -is [bool]) { return @{ Value = $Value; Warning = $null } }
    $s = "$Value"
    if ($s -eq 'true') { return @{ Value = $true; Warning = $null } }
    if ($s -eq 'false') { return @{ Value = $false; Warning = $null } }
    return @{ Value = $Default; Warning = "$Name`: '$s' is not a boolean; using $Default" }
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
        # Normalised to the allowed list's own casing when it matches case-insensitively: -notin
        # below is already case-insensitive, so a lower-case "magenta" passed it and was stored
        # verbatim - any later exact-case lookup keyed on the stored value would then miss it.
        $canonicalTint = $script:AllowedTints | Where-Object { $_ -eq $tint } | Select-Object -First 1
        if ($canonicalTint) { $tint = $canonicalTint }
        else { $warnings += "account '$key': tint '$tint' is not one of $($script:AllowedTints -join ', '); using Green"; $tint = 'Green' }
        $hiddenResult = ConvertTo-LauncherBool -Value $a.hidden -Default $false -Name "account '$key': hidden"
        if ($hiddenResult.Warning) { $warnings += $hiddenResult.Warning }
        $accounts += [pscustomobject]@{
            Key = $key; Root = $root
            Label = if ($a.label) { "$($a.label)" } else { "$key account" }
            Tint = $tint; Hidden = $hiddenResult.Value
            Canonical = ($root -eq $canonicalRoot); Rooted = $rooted
        }
    }
    foreach ($g in @($accounts | Group-Object Tint | Where-Object { $_.Count -gt 1 })) {
        $warnings += "accounts $(($g.Group | ForEach-Object Key) -join ', ') share tint '$($g.Name)'"
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
        if ($out.Key -contains $key) { $warnings += "maintenanceActions: duplicate key '$key'; the first one wins, this entry is skipped"; continue }
        $confirmResult = ConvertTo-LauncherBool -Value $m.confirmTwice -Default $false -Name "maintenanceActions '$key': confirmTwice"
        if ($confirmResult.Warning) { $warnings += $confirmResult.Warning }
        $out += [pscustomobject]@{
            Key = $key; Label = if ($m.label) { "$($m.label)" } else { $key }; Script = (Expand-LauncherPath "$($m.script)")
            ConfirmTwice = $confirmResult.Value
        }
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

    if ($raw.PSObject.Properties.Name -contains 'accounts') {
        if ($null -eq $raw.accounts) { $cfg.Warnings += 'accounts is null; using the default roster' }
        else { $r = ConvertTo-LauncherRoster $raw.accounts; $cfg.Accounts = @($r.Accounts); $cfg.Warnings += $r.Warnings }
    }
    if ($null -ne $raw.sharing) {
        $sharingResult = ConvertTo-LauncherBool -Value $raw.sharing -Default $cfg.Sharing -Name 'sharing'
        $cfg.Sharing = $sharingResult.Value
        if ($sharingResult.Warning) { $cfg.Warnings += $sharingResult.Warning }
    }
    if ($null -ne $raw.remote) {
        $remoteResult = ConvertTo-LauncherBool -Value $raw.remote -Default $cfg.Remote -Name 'remote'
        $cfg.Remote = $remoteResult.Value
        if ($remoteResult.Warning) { $cfg.Warnings += $remoteResult.Warning }
    }
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
