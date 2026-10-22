# The launch modes: one row per value of the project screen's action field. Exports
# $script:LaunchModes, Get-LaunchModes, Get-LaunchMode and Get-LaunchModeTableErrors. Every reader
# goes through this table: the field and its footer buttons (Screens.ps1 Get-ProjectFrame), the
# field's hotkeys (Ui.ps1 Invoke-ProjectScreen), the claude arguments (Screens.ps1 Get-LaunchArgs),
# the launcher loop's per-mode steps and the remote gate (claude-auto.ps1). A new mode = one row
# here, plus its Cyrillic letter in Input.ps1 $script:HotkeyLayoutChars when it has a new hotkey
# (Test-LaunchModes fails until it is there).
#
# Columns:
#   Name           the value the state carries and the word the field and the footer draw.
#   Hotkey         the project screen's one-key shortcut, drawn as a footer button. '' = none.
#   Flag           the claude flag the mode puts on the line ('' = none). Also how the remote gate
#                  recognises the mode on a passthrough command line (`claude-auto -c`).
#   AttachesNewest the mode attaches to the project's newest session (`claude -c`), so the loop asks
#                  first when another claude.exe holds that session.
#   PicksSession   the mode needs a session id: the loop opens the session picker before launch, and
#                  the flag is followed by the id (and --fork-session on a fork); no id, no flag.
#   RemoteOff      why the mode cannot launch through crc (remote is switched off for it, with this
#                  line printed). '' = crc can launch it.
#
# Row order is the field's order (Left/Right step through it) and the footer's button order. The
# first row is the fallback a value outside the table canonicalises to (Step-ProjectAction).

$script:LaunchModes = @(
    [pscustomobject]@{ Name = 'new';      Hotkey = '';  Flag = '';         AttachesNewest = $false; PicksSession = $false
                       RemoteOff = '' }
    [pscustomobject]@{ Name = 'continue'; Hotkey = 'c'; Flag = '-c';       AttachesNewest = $true;  PicksSession = $false
                       RemoteOff = 'crc births a new session, it cannot attach to an existing one' }
    [pscustomobject]@{ Name = 'resume';   Hotkey = 'r'; Flag = '--resume'; AttachesNewest = $false; PicksSession = $true
                       RemoteOff = 'crc births a new session, it cannot attach to an existing one' }
    # -w CREATES a session (the "cannot attach" wording is wrong for it). Whether `claude --bg -w`
    # births correctly through crc is NOT verified, so it still leaves the crc path.
    [pscustomobject]@{ Name = 'worktree'; Hotkey = 't'; Flag = '-w';       AttachesNewest = $false; PicksSession = $false
                       RemoteOff = 'a worktree launch is not routed through crc (unverified interaction)' }
)

# Keys the project screen keeps for itself (Ui.ps1 Invoke-ScreenLoop / Invoke-ProjectScreen): '/'
# opens the filter, w/s move the cursor, a/d step the action field. A mode hotkey on one of them
# would silently replace the filter or disable that navigation alias.
$script:ProjectScreenOwnKeys = @('/', 'w', 'a', 's', 'd')

function Get-LaunchModes { return $script:LaunchModes }

function Get-LaunchMode {
    # The row for one action value, or $null. -eq, so the lookup is case-insensitive like the switch
    # it replaced: a hand-typed 'RESUME' still finds the resume row.
    param([string]$Name)
    if (-not $Name) { return $null }
    return @($script:LaunchModes | Where-Object { $_.Name -eq $Name })[0]
}

function Get-LaunchModeTableErrors {
    # Every hotkey clash in a mode table, one line each, naming the row and the key. Empty = valid.
    param([Parameter(Mandatory)][object[]]$Modes, [string[]]$Reserved = $script:ProjectScreenOwnKeys)
    $errors = [Collections.Generic.List[string]]::new()
    $seen = @{}
    foreach ($mode in $Modes) {
        $key = "$($mode.Hotkey)"
        if (-not $key) { continue }
        if ($key -cin $Reserved) { $errors.Add("mode '$($mode.Name)': hotkey '$key' is a project-screen key (/ filter, w/s move, a/d action)") }
        if ($seen.ContainsKey($key)) { $errors.Add("mode '$($mode.Name)': hotkey '$key' is already mode '$($seen[$key])'s") }
        else { $seen[$key] = $mode.Name }
    }
    return $errors.ToArray()
}

$launchModeErrors = @(Get-LaunchModeTableErrors -Modes $script:LaunchModes)
if ($launchModeErrors.Count -gt 0) { throw "LaunchModes.ps1: $($launchModeErrors -join '; ')" }
