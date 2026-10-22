# The launch modes: one row per value of the project screen's action field. Exports
# $script:LaunchModes, Get-LaunchModes and Get-LaunchMode. Every reader goes through this table:
# the field and its footer buttons (Screens.ps1 Get-ProjectFrame), the field's hotkeys (Ui.ps1
# Invoke-ProjectScreen), the claude arguments (Screens.ps1 Get-LaunchArgs) and the launcher loop's
# per-mode steps (claude-auto.ps1). A new mode = one row here; a mode with a new step = one row plus
# a flag the loop reads.
#
# Columns:
#   Name           the value the state carries and the word the field and the footer draw.
#   Hotkey         the project screen's one-key shortcut, drawn as a footer button. '' = none.
#   Args           { param($ResumeId, $Fork) } -> the arguments the mode adds to the claude line.
#   AttachesNewest the mode attaches to the project's newest session (`claude -c`), so the loop asks
#                  first when another claude.exe holds that session.
#   PicksSession   the mode needs a session id, so the loop opens the session picker before launch.
#
# Row order is the field's order (Left/Right step through it) and the footer's button order. The
# first row is the fallback a value outside the table canonicalises to (Step-ProjectAction).

$script:LaunchModes = @(
    [pscustomobject]@{ Name = 'new';      Hotkey = '';  AttachesNewest = $false; PicksSession = $false
                       Args = { param($ResumeId, $Fork) } }
    [pscustomobject]@{ Name = 'continue'; Hotkey = 'c'; AttachesNewest = $true;  PicksSession = $false
                       Args = { param($ResumeId, $Fork) '-c' } }
    [pscustomobject]@{ Name = 'resume';   Hotkey = 'r'; AttachesNewest = $false; PicksSession = $true
                       Args = { param($ResumeId, $Fork) if ($ResumeId) { '--resume'; $ResumeId; if ($Fork) { '--fork-session' } } } }
    [pscustomobject]@{ Name = 'worktree'; Hotkey = 't'; AttachesNewest = $false; PicksSession = $false
                       Args = { param($ResumeId, $Fork) '-w' } }
)

function Get-LaunchModes { return $script:LaunchModes }

function Get-LaunchMode {
    # The row for one action value, or $null. -eq, so the lookup is case-insensitive like the switch
    # it replaced: a hand-typed 'RESUME' still finds the resume row.
    param([string]$Name)
    if (-not $Name) { return $null }
    return @($script:LaunchModes | Where-Object { $_.Name -eq $Name })[0]
}
