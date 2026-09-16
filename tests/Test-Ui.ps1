# Assertions for Ui.ps1. Run: pwsh -File Test-Ui.ps1
# Every screen is driven through the injectable key reader, so none of this needs a terminal.
$env:CLAUDE_AUTO_CONFIG = "$PSScriptRoot\fixtures\config-four.json"   # BEFORE the dot-sources
try {
    . "$PSScriptRoot\..\claude-auto\Theme.ps1"
    . "$PSScriptRoot\..\claude-auto\Layout.ps1"
    . "$PSScriptRoot\..\claude-auto\Sessions.ps1"   # Get-PickerFrame calls Format-RelativeAge at render time
    . "$PSScriptRoot\..\claude-auto\Projects.ps1"   # Get-ProjectFrame calls Select-ProjectMatch at render time
    . "$PSScriptRoot\..\claude-auto\Screens.ps1"
    . "$PSScriptRoot\..\claude-auto\Prefs.ps1"   # Invoke-LaunchScreen calls Switch-LaunchAccount / Reset-LaunchTab
    . "$PSScriptRoot\..\claude-auto\Input.ps1"   # Invoke-SessionPicker maps a click through Get-ClaudeMouseRow
    . "$PSScriptRoot\..\claude-auto\Ui.ps1"
    . "$PSScriptRoot\..\claude-auto\Maintenance.ps1"   # Invoke-MaintenanceScreen calls Get-ClaudeInstallInfo internally
    . "$PSScriptRoot\..\claude-auto\Env.ps1"   # Get-FriendlyModelName / Get-DefaultModelLabel - the model-row label shortener
    Set-LaunchRoster -Accounts (Read-LauncherConfig).Accounts -Remote
} catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }

# Env.ps1 is dot-sourced above (for the model-label helpers) so its REAL Write-LauncherLog is in
# scope too, and Expand-SessionPage's error-logging catch (Ui.ps1) now calls it on every IO-failure
# fixture in this file, including the ones that predate that catch. Defined here, before any test
# runs, so nothing in this file - old or new - appends to the owner's real launcher log; function
# lookup is last-wins in this scope, so this simply shadows Env.ps1's definition for the whole file.
$script:loggedCalls = @()
function Write-LauncherLog {
    param([string]$Stage, [hashtable]$Data = @{}, [string]$RunId, [string]$Root)
    $script:loggedCalls += [pscustomobject]@{ Stage = $Stage; Data = $Data }
}

$script:Failed = 0
$script:Ran = 0
function Assert-Equal {
    param($Expected, $Actual, [string]$Because)
    $script:Ran++
    if ("$Expected" -ne "$Actual") {
        Write-Host "FAIL  $Because"
        Write-Host "      expected: $Expected"
        Write-Host "      actual:   $Actual"
        $script:Failed++
    } else {
        Write-Host "ok    $Because"
    }
}
function Assert-True {
    param([bool]$Actual, [string]$Because)
    $script:Ran++
    if (-not $Actual) {
        Write-Host "FAIL  $Because"
        $script:Failed++
    } else {
        Write-Host "ok    $Because"
    }
}

# Fix 1 (Ctrl+C): New-ScriptedKeyReader has no way to express a modifier, so a mixed reader
# combines its string-driven keys with a pre-built Ctrl+C ConsoleKeyInfo. The last three
# constructor booleans are shift, alt, control.
$CtrlC = [System.ConsoleKeyInfo]::new([char]3, [System.ConsoleKey]::C, $false, $false, $true)
function New-MixedKeyReader {
    param([Parameter(Mandatory)][array]$Keys)
    $queue = [System.Collections.Queue]::new()
    foreach ($k in $Keys) {
        if ($k -is [System.ConsoleKeyInfo]) { $queue.Enqueue($k); continue }
        if ("$k".Length -eq 1) { $queue.Enqueue([System.ConsoleKeyInfo]::new([char]"$k", [System.ConsoleKey]0, $false, $false, $false)) }
        else {
            $parsed = [System.ConsoleKey]::A
            if (-not [enum]::TryParse([System.ConsoleKey], "$k", [ref]$parsed)) { throw "unknown key name '$k'" }
            $queue.Enqueue([System.ConsoleKeyInfo]::new([char]0, $parsed, $false, $false, $false))
        }
    }
    return { if ($queue.Count -eq 0) { throw 'scripted keys exhausted' }; $queue.Dequeue() }.GetNewClosure()
}

# Row order changed 2026-08-11 (account, remote, action, model, effort, permission, mode) and
# will change again - every test that navigates by counting DownArrow presses looks the row's
# index up here rather than hardcoding a count, so a future reorder points a stale assertion at
# the wrong row loudly (wrong result) instead of quietly (right result, wrong reason).
function Get-RowIndex {
    param([string]$Name)
    return [Array]::IndexOf(@((Get-LaunchRows) | ForEach-Object { $_.Name }), $Name)
}

# The navigation tests below all compute their DownArrow counts from Get-RowIndex, so they would
# keep passing under ANY row order - they prove navigation works, not that the order is the one
# the owner asked for. This is the one assertion that actually pins the order.
Assert-Equal 'Account,Remote,Model,Effort,Advisor,Permission,Mode' (((Get-LaunchRows) | ForEach-Object { $_.Name }) -join ',') 'the row order is account, remote, model, effort, advisor, permission, mode - action left this screen for the project screen (Task 9)'

# --- screen 1 -----------------------------------------------------------------------------

# Enter alone must reproduce today's launcher exactly: work account, remote on, new session.
$r = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('Enter')) -Draw {}
Assert-Equal 'work'    $r.Account 'Enter alone keeps the work account'
Assert-Equal 'on'      $r.Remote  'Enter alone keeps remote on'
Assert-Equal 'new'     $r.Action  'Enter alone starts a new session'
Assert-Equal 'default' $r.Model   'Enter alone leaves the model unset'
Assert-Equal 'default' $r.Effort  'Enter alone leaves the effort unset'
Assert-Equal 'normal'  $r.Mode    'Enter alone does not enable safe mode'

# THE invariant: the default state adds no arguments at all.
Assert-Equal 0 (@(Get-LaunchArgs -State (New-LaunchState))).Count 'a default state produces no launch arguments'
# ProjectSlug (Task 9): derived, never a habit - a fresh state carries none until Resolve-StartProject
# or the project screen fills it in.
Assert-Equal '' (New-LaunchState).ProjectSlug 'a fresh state carries no project slug'

$s = New-LaunchState; $s.Model = 'fable'
Assert-Equal '--model fable' (@(Get-LaunchArgs -State $s) -join ' ') 'fable becomes --model fable'
$s = New-LaunchState; $s.Model = 'opus1m'
Assert-Equal '--model opus[1m]' (@(Get-LaunchArgs -State $s) -join ' ') 'the opus 1M-context option resolves its internal key to the --model alias'
$s = New-LaunchState; $s.Model = 'sonnet1m'
Assert-Equal '--model sonnet[1m]' (@(Get-LaunchArgs -State $s) -join ' ') 'the sonnet 1M-context option resolves its internal key to the --model alias'
$s = New-LaunchState; $s.Model = 'haiku'
Assert-Equal '--model haiku' (@(Get-LaunchArgs -State $s) -join ' ') 'haiku becomes --model haiku'
$s = New-LaunchState; $s.Effort = 'high'
Assert-Equal '--effort high' (@(Get-LaunchArgs -State $s) -join ' ') 'effort becomes --effort'
$s = New-LaunchState; $s.Permission = 'bypass'
Assert-Equal '--permission-mode bypassPermissions' (@(Get-LaunchArgs -State $s) -join ' ') 'bypass expands to the full mode name'
$s = New-LaunchState; $s.Permission = 'plan'
Assert-Equal '--permission-mode plan' (@(Get-LaunchArgs -State $s) -join ' ') 'plan passes through unchanged'
$s = New-LaunchState; $s.Mode = 'safe'
Assert-Equal '--safe-mode' (@(Get-LaunchArgs -State $s) -join ' ') 'safe mode becomes --safe-mode'
$s = New-LaunchState; $s.Action = 'continue'
Assert-Equal '-c' (@(Get-LaunchArgs -State $s) -join ' ') 'continue becomes -c'
$s = New-LaunchState; $s.Action = 'worktree'
Assert-Equal '-w' (@(Get-LaunchArgs -State $s) -join ' ') 'worktree becomes -w'
$s = New-LaunchState; $s.Action = 'resume'
Assert-Equal '--resume abc123' (@(Get-LaunchArgs -State $s -ResumeId 'abc123') -join ' ') 'resume carries the session id'
Assert-Equal '--resume abc123 --fork-session' (@(Get-LaunchArgs -State $s -ResumeId 'abc123' -Fork) -join ' ') 'fork adds --fork-session'

$s = New-LaunchState; $s.Model = 'opus1m'; $s.Effort = 'max'; $s.Mode = 'safe'
Assert-Equal '--model opus[1m] --effort max --safe-mode' (@(Get-LaunchArgs -State $s) -join ' ') 'flags combine in a stable order'

# --- effort ultracode and the advisor row (owner ask 2026-09-04) ----------------------------
# 'ultracode' is offered because 2.1.260 ACCEPTS it: a bogus --effort value warns on stderr, this
# one does not. The row is the CLI's list, not a guess - an option here reads as a recommendation.
$effortRow = (Get-LaunchRows) | Where-Object { $_.Name -eq 'Effort' }
Assert-Equal 'default,low,medium,high,xhigh,max,ultracode' (($effortRow.Values) -join ',') 'the effort row offers ultracode after max'
$s = New-LaunchState; $s.Effort = 'ultracode'
Assert-Equal '--effort ultracode' (@(Get-LaunchArgs -State $s) -join ' ') 'ultracode is passed through like every other effort'

# --advisor <model> per code.claude.com/docs/en/advisor.md. 'off' is NOT a flag - the CLI has none -
# it is the CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1 environment variable the launcher sets before exec.
# Both 'default' and 'off' must therefore add nothing here, for opposite reasons, and the difference
# is only visible in the environment: assert both, or one of them silently becomes the other.
$advisorRow = (Get-LaunchRows) | Where-Object { $_.Name -eq 'Advisor' }
Assert-Equal 'default,fable,opus,off' (($advisorRow.Values) -join ',') 'the advisor row offers default, fable, opus and off'
Assert-Equal 'default' (New-LaunchState).Advisor 'a fresh state leaves the advisor at default'
$s = New-LaunchState; $s.Advisor = 'opus'
Assert-Equal '--advisor opus' (@(Get-LaunchArgs -State $s) -join ' ') 'an advisor model becomes --advisor'
$s = New-LaunchState; $s.Advisor = 'fable'
Assert-Equal '--advisor fable' (@(Get-LaunchArgs -State $s) -join ' ') 'the other advisor model does too'
$s = New-LaunchState; $s.Advisor = 'off'
Assert-Equal 0 (@(Get-LaunchArgs -State $s)).Count 'advisor off adds no flag - it is an environment variable, not an argument'
$s = New-LaunchState; $s.Advisor = 'default'
Assert-Equal 0 (@(Get-LaunchArgs -State $s)).Count 'advisor default adds no flag either'
$s = New-LaunchState; $s.Model = 'opus1m'; $s.Effort = 'ultracode'; $s.Advisor = 'fable'; $s.Permission = 'plan'
Assert-Equal '--model opus[1m] --effort ultracode --advisor fable --permission-mode plan' (@(Get-LaunchArgs -State $s) -join ' ') 'the advisor flag sits between effort and permission'

# 'default' on this row means whatever settings.json's advisorModel says, resolved by the caller
# exactly as the model row's default is - the row definition cannot know it.
Assert-Equal 'default (fable)' (Get-RowOptionText -Row $advisorRow -Key 'default' -DefaultAdvisorLabel 'default (fable)') 'the advisor default defers to the caller-resolved label'
Assert-Equal 'opus' (Get-RowOptionText -Row $advisorRow -Key 'opus' -DefaultAdvisorLabel 'default (fable)') 'a chosen advisor model renders as its own key'
$advFrame = @(Get-LaunchFrame -State (New-LaunchState) -Width 100 -Height 30 -DefaultAdvisorLabel 'default (fable)')
Assert-Equal 1 (@($advFrame | Where-Object { $_ -match [regex]::Escape('default (fable)') }).Count) 'the launch frame renders the resolved advisor default'

# The row LABELS are dimmed as a group; a new row left out of that list reads as the only emphasised
# word on the screen.
# No \b in the filter: on a PAINTED line the label is preceded by the 'm' that ends its own escape
# sequence ("ESC[2madvisor"), so \badvisor is two word characters and never matches. The uncoloured
# assertions above can use \b; anything reading a coloured line must not.
$dimEsc = "$([char]27)[2m"
$advColored = @(Get-LaunchFrame -State (New-LaunchState) -Width 100 -Height 30 -Color | Where-Object { $_ -match 'advisor' })
Assert-Equal 1 $advColored.Count 'exactly one advisor row in the coloured frame'
Assert-Equal $true ($advColored[0] -match ([regex]::Escape($dimEsc) + 'advisor')) 'the advisor label is dimmed like every other row label'

# Navigation reaches every row, and values wrap in both directions. Downs are computed from the
# row's own index (see Get-RowIndex above), not a hardcoded count.
$lastRow = (Get-LaunchRows).Count - 1
$down = @('DownArrow') * $lastRow + @('RightArrow', 'Enter')
$r = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys $down) -Draw {}
Assert-Equal 'safe' $r.Mode "$lastRow downs reach the last row (mode)"

$remoteDowns = @('DownArrow') * (Get-RowIndex -Name 'Remote')
$r = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys ($remoteDowns + @('LeftArrow', 'Enter'))) -Draw {}
Assert-Equal 'stop server' $r.Remote 'left wraps to the last value of the remote row'

$r = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw {}
Assert-Equal '' "$r" 'escape abandons the launch'

$r = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-MixedKeyReader -Keys @($CtrlC)) -Draw {}
Assert-Equal '' "$r" 'ctrl+c abandons the launch screen exactly like escape'

# Ctrl+R resets the state but must not move the cursor, or the owner loses their place.
#
# It was a bare `r` until 2026-08-23, and it is not in the footer hints - so nothing on screen said
# that one keystroke flattens all seven rows at once. It did exactly that during a launch on that
# date (launcher-logs 12:57:02: account=work, remote=on, no state flags in argv), and the flattened
# state was then saved, which is how "the settings reset themselves" was reported. Prefs no longer
# persist the reset (Prefs.ps1), and a modifier now stands between a stray keystroke and the reset.
# The owner chose the modifier over advertising the bare key (2026-08-23).
$CtrlR = [System.ConsoleKeyInfo]::new([char]18, [System.ConsoleKey]::R, $false, $false, $true)
$modelDowns = @('DownArrow') * (Get-RowIndex -Name 'Model')
$r = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-MixedKeyReader -Keys ($modelDowns + @('RightArrow', $CtrlR, 'Enter'))) -Draw {}
Assert-Equal 'default' $r.Model 'ctrl+r resets a changed value back to its default'
Assert-Equal (Get-RowIndex -Name 'Model') $r.Row 'ctrl+r leaves the cursor row where it was'

# A bare 'r' must now be inert: it is an ordinary character on a screen that has no text entry, and
# treating it as a reset is what made a typo indistinguishable from a deliberate wipe.
$r = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-MixedKeyReader -Keys ($modelDowns + @('RightArrow', 'r', 'Enter'))) -Draw {}
Assert-Equal 'fable' $r.Model 'a bare r no longer resets anything'
Assert-Equal (Get-RowIndex -Name 'Model') $r.Row 'a bare r leaves the cursor row where it was'

# --- model row: content survives every width, and the label/value split holds ------------
$modelRow = (Get-LaunchRows) | Where-Object { $_.Name -eq 'Model' }
Assert-Equal 'Fable 5.1' (Get-RowOptionText -Row $modelRow -Key 'fable') 'a non-default model key renders its static label'
Assert-Equal 'shown'   (Get-RowOptionText -Row $modelRow -Key 'default' -DefaultModelLabel 'shown') 'the default key defers to the caller-resolved label'
$accountRow = (Get-LaunchRows) | Where-Object { $_.Name -eq 'Account' }
Assert-Equal 'work' (Get-RowOptionText -Row $accountRow -Key 'work') 'a row with no Labels table uses its key as the label'

# --- accounts: the third account (2026-08-20) is HIDDEN since 2026-09-02 (inactive), the fourth
# (2026-08-26) stays. Un-hiding is one value back in the row; this assertion is what says so. ---
Assert-Equal 'work,personal,low' (($accountRow.Values) -join ',') 'the account row offers work, personal and low - shared is hidden'
# The short keys are the whole reason the row still fits: a 12-character key would push it into the
# collapsed form at 100 columns, which is the measured width crisis Env.ps1 records. Assert the
# uncollapsed row instead of trusting the choice - and assert it for the LONGEST selection, since
# the selected cell is the one that grows by two bracket characters.
foreach ($w in @(50, 60, 100, 120)) {
    foreach ($acc in @('low')) {
        $sel = New-LaunchState; $sel.Account = $acc
        $f = Get-LaunchFrame -State $sel -Width $w -Height 24
        Assert-Equal 0 (@($f | Where-Object { $_.Length -gt $w }).Count) "no line exceeds width $w with the $acc account selected"
        $accLine = @($f | Where-Object { $_ -match '\baccount\b' })
        Assert-Equal 1 $accLine.Count "width ${w}: exactly one account row ($acc selected)"
        Assert-Equal $false ($accLine[0] -match '\bshared\b') "width ${w}: the hidden shared account is not drawn"
        if ($w -ge 100) {
            Assert-Equal $true ($accLine[0] -match "\[$acc\]" -and $accLine[0] -match 'personal' -and $accLine[0] -match 'work') "width ${w}: the account row shows every account without collapsing ($acc selected)"
        }
    }
}
# Its own tint per account: sharing green with 'work' is how a session gets spent on the wrong
# account's limit, and two accounts sharing a colour is the same failure one step later.
$cyan = "$([char]27)[36m"; $green = "$([char]27)[32m"; $blue = "$([char]27)[34m"
$selLow = New-LaunchState; $selLow.Account = 'low'
$colouredLow = @(Get-LaunchFrame -State $selLow -Width 120 -Height 24 -Color | Where-Object { $_ -match '\[low\]' })
Assert-Equal 1 $colouredLow.Count 'the coloured frame has exactly one low line'
Assert-Equal $true ($colouredLow[0] -match [regex]::Escape($blue)) 'the selected low account is painted blue'
Assert-Equal $false ($colouredLow[0] -match [regex]::Escape($cyan + '[low]')) 'low is not painted like the hidden account'

# --- the account row is a TAB STRIP (owner ask 2026-09-04) -------------------------------------
# Switching accounts is the habit, so every account's own five-hour percentage is on the row before
# the switch, not after it. The active tab is the bracketed one; the bars below belong to it alone.
$tabGlyphs = Get-Glyphs
$tabLimits = @{
    work     = [pscustomobject]@{ FiveHour = 40; SevenDay = 48; AgeText = '3 min ago'; Model = 15;    ModelLabel = 'FABLE' }
    personal = [pscustomobject]@{ FiveHour = 17; SevenDay = 14; AgeText = '1 h ago';   Model = $null; ModelLabel = $null }
    low      = [pscustomobject]@{ FiveHour = 43; SevenDay = 22; AgeText = 'just now';  Model = $null; ModelLabel = $null }
}
# 'fable' alone is useless as a probe: it is a VALUE on the advisor row and, case-insensitively, the
# model row's 'Fable 5.1' too. The third bar is identified by its shape - the label, eight bar cells
# and a percentage - which is the only thing that distinguishes it from an option cell.
$barCells = "[$($tabGlyphs.BarFull)$($tabGlyphs.BarEmpty)]{8}"
$modelBarRx = "fable $barCells\s+\d+%"

$tf = @(Get-LaunchFrame -State (New-LaunchState) -Width 100 -Height 30 -Limits $tabLimits)
$tabLine = @($tf | Where-Object { $_ -match '\baccount\b' })
Assert-Equal 1 $tabLine.Count 'the tab strip is one line'
Assert-Equal $true ($tabLine[0] -match [regex]::Escape('[work 40%]')) 'the active account is bracketed and carries its own five-hour percentage'
Assert-Equal $true ($tabLine[0] -match 'personal 17%') 'an inactive tab shows its account''s percentage too'
Assert-Equal $true ($tabLine[0] -match 'low 43%') 'and so does the third'
Assert-Equal $false ($tabLine[0] -match "5h $barCells") 'the five-hour BAR no longer shares the account row'

# Wide: all three bars on one line, with the age of the record they came from. The age travels with
# them because a three-day-old number that reads as current is the failure this line exists to stop.
$barLine = @($tf | Where-Object { $_ -match "5h $barCells" })
Assert-Equal 1 $barLine.Count 'at 100 columns the bars share one line'
Assert-Equal $true ($barLine[0] -match "7d $barCells") 'the seven-day bar is on it'
Assert-Equal $true ($barLine[0] -match $modelBarRx) 'and the model bucket, labelled with the record''s own model'
Assert-Equal $true ($barLine[0] -match '15%') 'the model percentage is the bucket, not one of the account limits'
Assert-Equal $true ($barLine[0] -match '3 min ago') 'the age of that record closes the line'

# No model bucket in the record (statusline.js writes none) - no third bar. Not "a bar reading 0".
$noModel = @{ work = [pscustomobject]@{ FiveHour = 40; SevenDay = 48; AgeText = '3 min ago'; Model = $null; ModelLabel = $null } }
$nf = @(Get-LaunchFrame -State (New-LaunchState) -Width 100 -Height 30 -Limits $noModel)
Assert-Equal 0 (@($nf | Where-Object { $_ -match $modelBarRx }).Count) 'a record without a model bucket draws no third bar'
Assert-Equal 1 (@($nf | Where-Object { $_ -match "5h $barCells" }).Count) 'while the two account bars are still drawn'

# Percentages are dropped only when the line with them does not fit - the arithmetic, not a width
# literal, because a wider label or a fourth account moves the breakpoint and a literal would not.
# The whole line is pinned rather than probed for '40%': asserting only the percentage cannot tell
# "dropped the percentages" from "collapsed to the selected tab", and the collapsed form has no
# percentage either - so a mutation that skipped the drop step passed by falling into the collapse.
$sep = " $($tabGlyphs.HintSep) "
function New-ExpectedTabLine {
    param([switch]$WithPercent, [hashtable]$Limits, [string]$Sep, $Glyphs)
    $cells = @('work', 'personal', 'low') | ForEach-Object {
        $l = $Limits[$_]
        $t = if ($WithPercent -and $l -and $null -ne $l.FiveHour) { "$_ $($l.FiveHour)%" } else { "$_" }
        if ($_ -eq 'work') { "[$t]" } else { $t }
    }
    # The cursor sits on row 0 (account) in a fresh state, so the prefix is the cursor glyph, not
    # three blanks - both are three columns wide, which is what keeps the cell spans stable.
    return (" $($Glyphs.Cursor) " + 'account     ' + ($cells -join $Sep))
}
foreach ($w in @(50, 60, 78, 100, 120)) {
    $inner = [Math]::Min($w, 100) - 4
    $withPct   = New-ExpectedTabLine -WithPercent -Limits $tabLimits -Sep $sep -Glyphs $tabGlyphs
    $namesOnly = New-ExpectedTabLine              -Limits $tabLimits -Sep $sep -Glyphs $tabGlyphs
    # Percentages first, then names alone, then the collapsed form - each step taken only because
    # the one before it did not fit.
    $expected =
        if ($withPct.Length -le $inner) { $withPct }
        elseif ($namesOnly.Length -le $inner) { $namesOnly }
        else { " $($tabGlyphs.Cursor) " + 'account     ' + "$($tabGlyphs.LAngle) work $($tabGlyphs.RAngle)" }
    $f = @(Get-LaunchFrame -State (New-LaunchState) -Width $w -Height 30 -Limits $tabLimits)
    $line = @($f | Where-Object { $_ -match '\baccount\b' })[0]
    Assert-Equal $expected $line "width ${w}: the account row is exactly what fits ($($withPct.Length) with percentages, $($namesOnly.Length) without, $inner available)"
    Assert-Equal 0 (@($f | Where-Object { $_.Length -gt $w }).Count) "width ${w}: no line exceeds the terminal with three tabs and three bars"
}

# Narrow: one bar per line, the age on the last of them - squeezing three onto one line would
# truncate the numbers themselves.
$narrow = @(Get-LaunchFrame -State (New-LaunchState) -Width 50 -Height 40 -Limits $tabLimits)
$narrowBars = @($narrow | Where-Object { $_ -match "(5h|7d|fable)\s+$barCells" })
Assert-Equal 3 $narrowBars.Count 'at 50 columns each bar gets its own line'
Assert-Equal $true ($narrowBars[2] -match '3 min ago') 'the age sits on the last of them'
Assert-Equal 0 (@($narrowBars | Where-Object { $_ -match "5h\s+$barCells.*7d\s+$barCells" }).Count) 'and no line carries two bars'
# Stacked bars line up: the labels are padded to the widest ('fable'), so the three bar runs start
# in one column instead of stepping right with the label's length (owner, 2026-09-04).
$barStarts = @($narrowBars | ForEach-Object { [regex]::Match($_, $barCells).Index } | Select-Object -Unique)
Assert-Equal 1 $barStarts.Count 'at 50 columns the three bars start in the same column'

# The tabs are click cells: each one carries its own account key, so a click walks the same stepper
# the arrow keys do. Without this the strip would be a picture of a menu.
$tmap = $null
$null = Get-LaunchFrame -State (New-LaunchState) -Width 100 -Height 30 -Limits $tabLimits -RowMap ([ref]$tmap)
$tabRow = @($tmap.Rows | Where-Object { $_.Name -eq 'Account' })[0]
Assert-Equal 'work,personal,low' ((@($tabRow.Cells) | ForEach-Object { $_.Value }) -join ',') 'every tab is a click cell carrying its account key'
Assert-Equal $true ($tabRow.Cells[1].Start -gt $tabRow.Cells[0].End) 'the cells do not overlap'

# The active tab is painted with the account tint, and only the active one. Exactly one Bold escape
# on the line, and it opens the bracketed cell: Assert-Equal cannot see control characters, so the
# escapes are counted with a regex rather than compared as text.
$boldEsc = "$([char]27)[1m"
$tabColored = @(Get-LaunchFrame -State (New-LaunchState) -Width 100 -Height 30 -Limits $tabLimits -Color | Where-Object { $_ -match 'account' })[0]
Assert-Equal 1 ([regex]::Matches($tabColored, [regex]::Escape($boldEsc)).Count) 'exactly one tab is emphasised'
Assert-Equal $true ($tabColored.IndexOf($boldEsc) -lt $tabColored.IndexOf('[work')) 'and it is the active one'
Assert-Equal $true ($tabColored -match [regex]::Escape($green)) 'the work account keeps its green tint'
$selPersonal = New-LaunchState; $selPersonal.Account = 'personal'
$magenta = "$([char]27)[35m"
$tabColoredP = @(Get-LaunchFrame -State $selPersonal -Width 100 -Height 30 -Limits $tabLimits -Color | Where-Object { $_ -match 'account' })[0]
Assert-Equal $true ($tabColoredP.IndexOf($magenta) -lt $tabColoredP.IndexOf('[personal')) 'the personal tint opens the personal tab'
Assert-Equal 1 ([regex]::Matches($tabColoredP, [regex]::Escape($boldEsc)).Count) 'still exactly one emphasised tab'

# A long, fixed default label (this machine's real settings.json value, hardcoded so the test does
# not depend on what the live file currently says) makes the overflow behaviour deterministic.
$longDefaultLabel = 'default (claude-fable-5-1[1m])'
foreach ($w in @(60, 80, 100, 120)) {
    $sel = New-LaunchState; $sel.Model = 'sonnet1m'   # 'Sonnet 5[1M]' is the longest static label
    $f = Get-LaunchFrame -State $sel -Width $w -Height 24 -DefaultModelLabel $longDefaultLabel
    Assert-Equal 0 (@($f | Where-Object { $_.Length -gt $w }).Count) "no line exceeds width $w with Sonnet 5[1M] selected"
    Assert-Equal 1 (@($f | Where-Object { $_ -match [regex]::Escape('Sonnet 5[1M]') }).Count) "width ${w}: the selected model label renders complete, not truncated"
}
foreach ($w in @(60, 80, 100, 120)) {
    $f = Get-LaunchFrame -State (New-LaunchState) -Width $w -Height 24 -DefaultModelLabel $longDefaultLabel
    Assert-Equal 0 (@($f | Where-Object { $_.Length -gt $w }).Count) "no line exceeds width $w with the long resolved default label"
    Assert-Equal 1 (@($f | Where-Object { $_ -match [regex]::Escape($longDefaultLabel) }).Count) "width ${w}: the resolved default label renders complete, not truncated"
}
# The collapse is content-driven, not width-driven alone: short labels at a generous width still
# show every option, proving the compact form is not hardcoded to always fire.
$f = Get-LaunchFrame -State (New-LaunchState) -Width 120 -Height 24 -DefaultModelLabel 'default'
Assert-Equal 1 (@($f | Where-Object { $_ -match [regex]::Escape('Haiku 4.5') }).Count) 'at width 120 with short labels the model row shows every option, not just the selected one'
# And when it DOES collapse, it must actually show the compact ‹ › form rather than silently
# truncating - Limit-Line would truncate too and this width check alone cannot tell them apart.
# Matched against the line carrying the selected label itself, not "any line in the frame" - at
# width 60 the effort row's six values overflow the same way and would otherwise double-count.
foreach ($ascii in @($true, $false)) {
    $glyphs = Get-Glyphs -Ascii:$ascii
    $sel = New-LaunchState; $sel.Model = 'sonnet1m'
    $f = Get-LaunchFrame -State $sel -Width 60 -Height 24 -DefaultModelLabel $longDefaultLabel -Ascii:$ascii
    $modelLine = @($f | Where-Object { $_ -match [regex]::Escape('Sonnet 5[1M]') })
    Assert-Equal 1 $modelLine.Count "width 60 (ascii=${ascii}): the selected model label appears on exactly one line"
    $isCompact = $modelLine.Count -eq 1 -and ($modelLine[0] -match [regex]::Escape($glyphs.LAngle)) -and ($modelLine[0] -match [regex]::Escape($glyphs.RAngle))
    Assert-Equal $true $isCompact "width 60 (ascii=${ascii}): the overflowing model row collapses to the compact ‹ › form, not a silent truncation"
}

# Widths, both glyph sets, every breakpoint.
foreach ($w in @(60, 80, 120, 200)) {
    foreach ($ascii in @($true, $false)) {
        $f = Get-LaunchFrame -State (New-LaunchState) -Width $w -Height 24 -Ascii:$ascii
        Assert-Equal 0 (@($f | Where-Object { $_.Length -gt $w }).Count) "no launch line exceeds width $w (ascii=$ascii)"
    }
}

# The wide-layout branch that puts the rate-limit bars beside the account row only runs at
# -Width 100+ with -Limits set; the loop above never passed -Limits, so that branch's width
# arithmetic went unchecked.
$wideLimits = @{ work = [pscustomobject]@{ FiveHour = 90; SevenDay = 90; AgeText = 'now' } }
$f = Get-LaunchFrame -State (New-LaunchState) -Width 120 -Height 24 -Limits $wideLimits
Assert-Equal 0 (@($f | Where-Object { $_.Length -gt 120 }).Count) 'no launch line exceeds width 120 with the wide rate-limit bars shown'
# The width check above can never fail on its own: Get-LaunchFrame Limit-Lines every line to
# $Width right before returning, so any internal overflow in this branch would be silently cut
# rather than reported. Assert the actual content survives instead - both percentages and the
# age text - which is what a broken bar-width or padding calculation would actually corrupt.
#
# Since 2026-09-04 the bars share ONE line below the tab strip; the tab strip itself carries a
# third '90%' for the active account. Counting '90%' across the frame would score 2 whichever way
# the bars were laid out, so the assertion names the bar line: both bars on it, and the age after
# them. The 5h bar has not shared the account row since the strip replaced it.
$wideBarLine = @($f | Where-Object { $_ -match "5h [$($tabGlyphs.BarFull)$($tabGlyphs.BarEmpty)]{8}" })
Assert-Equal 1 $wideBarLine.Count 'one bar line at width 120 with the wide layout'
Assert-Equal 2 ([regex]::Matches($wideBarLine[0], '90%').Count) 'both rate-limit bars render on it'
Assert-Equal $true ($wideBarLine[0] -match '90%.*now') 'the rate-limit age text is not pushed off the end of it'
Assert-Equal $false (@($f | Where-Object { $_ -match '\baccount\b' })[0] -match "5h [$($tabGlyphs.BarFull)$($tabGlyphs.BarEmpty)]{8}") 'and the account row itself carries no bar'

# Below the minimum the screen says what it needs instead of rendering garbage. The size it names
# comes from the constants, so the message cannot go stale the way a literal here would.
$needed = "need $($script:MinWidth)x$($script:MinHeight)"
$f = Get-LaunchFrame -State (New-LaunchState) -Width 40 -Height 24
Assert-Equal 1 (@($f | Where-Object { $_ -match [regex]::Escape($needed) }).Count) 'a too-narrow terminal states the required size'
$f = Get-LaunchFrame -State (New-LaunchState) -Width 50 -Height ($script:MinHeight - 1)
Assert-Equal 1 (@($f | Where-Object { $_ -match [regex]::Escape($needed) }).Count) 'a too-short terminal states the required size'
$f = Get-LaunchFrame -State (New-LaunchState) -Width 50 -Height $script:MinHeight
Assert-Equal 0 (@($f | Where-Object { $_ -match [regex]::Escape($needed) }).Count) 'and one row taller renders the screen instead of refusing it'

$f = Get-LaunchFrame -State (New-LaunchState) -Width 100 -Height 24 -Version @{ Installed='2.1.226'; Newest='2.1.230' }
Assert-Equal 1 (@($f | Where-Object { $_ -match '2\.1\.230' }).Count) 'an available update is visible on the launch screen'

# Restored rows are marked, and the notice appears only when something was actually restored.
$f = Get-LaunchFrame -State (New-LaunchState) -Width 84 -Height 24 -Restored @('Model') -RestoredAge '5 min'
Assert-Equal 1 (@($f | Where-Object { $_ -match 'model\*' }).Count) 'a restored row is marked'
# Since the marks became per tab they live ON the state, and switching tabs recomputes them. The
# parameters stay for the tests and any caller that wants to override, but the default has to be the
# state - a caller passing its own pre-screen copy would freeze the first tab's marks on every tab.
$stateMarks = New-LaunchState
$stateMarks.Restored = @('Effort'); $stateMarks.RestoredAge = '2 h'
$f = Get-LaunchFrame -State $stateMarks -Width 84 -Height 24
Assert-Equal 1 (@($f | Where-Object { $_ -match 'effort\*' }).Count) 'the frame marks the rows the STATE says were restored'
Assert-Equal 1 (@($f | Where-Object { $_ -match '2 h ago' }).Count) 'and reads the age from the state too'
$f = Get-LaunchFrame -State $stateMarks -Width 84 -Height 24 -Restored @('Model') -RestoredAge '5 min'
Assert-Equal 1 (@($f | Where-Object { $_ -match 'model\*' }).Count) 'an explicit -Restored still wins over the state'
Assert-Equal 0 (@($f | Where-Object { $_ -match 'effort\*' }).Count) 'and replaces it rather than adding to it'
$f = Get-LaunchFrame -State (New-LaunchState) -Width 84 -Height 24
Assert-Equal 0 (@($f | Where-Object { $_ -match 'restored from your last launch' }).Count) 'no restore notice when nothing was restored'

# The too-small screen is itself rendered at the caller's actual (tiny) width and height - it must
# obey the same budget it is warning the user about, not just contain the number 60 somewhere.
foreach ($w in @(10, 20, 30, 50)) {
    foreach ($h in @(2, 4, 10, 24)) {
        $lf = Get-LaunchFrame -State (New-LaunchState) -Width $w -Height $h
        Assert-Equal 0 (@($lf | Where-Object { $_.Length -gt $w }).Count) "too-small launch frame has no line over width $w at height $h"
        Assert-Equal $true ($lf.Count -le $h) "too-small launch frame has no more than $h lines at width $w"
        $pf = Get-PickerFrame -Sessions @() -Index 0 -Filter '' -Width $w -Height $h
        Assert-Equal 0 (@($pf | Where-Object { $_.Length -gt $w }).Count) "too-small picker frame has no line over width $w at height $h"
        Assert-Equal $true ($pf.Count -le $h) "too-small picker frame has no more than $h lines at width $w"
    }
}

# --- screen 2 -----------------------------------------------------------------------------

$now = Get-Date '2026-08-10 20:00'
$fake = @(
    [pscustomobject]@{ SessionId='aaaa1111'; Project='Workbench'; Worktree=$null; Modified=(Get-Date '2026-08-10 17:52'); SizeBytes=1258291; PromptCount=25; Title='launcher menu'; LastUser='add resume to claude-auto and make the preview readable at a glance'; LastAssistant='Read the launcher, 390 lines.' }
    [pscustomobject]@{ SessionId='bbbb2222'; Project='Demo-Service'; Worktree='feature-1'; Modified=(Get-Date '2026-08-09 14:03'); SizeBytes=204800; PromptCount=7; Title='batch run'; LastUser='run the nightly job'; LastAssistant='118 messages, 4 parse errors.' }
)

$sel = Invoke-SessionPicker -Sessions $fake -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow','Enter')) -Draw {}
Assert-Equal 'bbbb2222' $sel.Session.SessionId 'down then enter selects the second row'
Assert-Equal $false     $sel.Fork              'enter opens without forking'

$sel = Invoke-SessionPicker -Sessions $fake -ReadKey (New-ScriptedKeyReader -Keys @('f')) -Draw {}
Assert-Equal 'aaaa1111' $sel.Session.SessionId 'f opens the selected session'
Assert-Equal $true      $sel.Fork              'f marks the launch as a fork'

$sel = Invoke-SessionPicker -Sessions $fake -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw {}
Assert-Equal '' "$sel" 'escape leaves the picker with nothing'

# deferred review finding: Select-ResumableSessions wraps ITS OWN return in @(), but the CALL SITE
# inside Invoke-SessionPicker did not - a function emitting ZERO items unrolls to $null on the
# pipeline regardless of how it built that array internally, so a session list of zero (or, after
# Select-ResumableSessions drops every zero-prompt entry, a list that becomes zero) crashed
# Select-SessionMatch's Mandatory -Sessions with a raw PowerShell binding error instead of showing
# "no sessions found". Found via tests\check-preview.ps1's empty-fixture-account run.
$threwOnEmpty = $false
try { $selEmpty = Invoke-SessionPicker -Sessions @() -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw {} }
catch { $threwOnEmpty = $true }
Assert-Equal $false $threwOnEmpty 'an empty session list does not crash the picker'
Assert-Equal ''      "$selEmpty"  'and escape still leaves it with nothing, exactly like a non-empty list'

Assert-Equal 1 (Select-SessionMatch -Sessions $fake -Filter 'batch').Count 'filter matches the worktree name'
Assert-Equal 1 (Select-SessionMatch -Sessions $fake -Filter 'parse errors').Count 'filter matches assistant text'
Assert-Equal 2 (Select-SessionMatch -Sessions $fake -Filter '').Count 'empty filter keeps everything'

$keys = @('/', 'b', 'a', 't', 'c', 'h', 'Enter', 'Enter')
$sel = Invoke-SessionPicker -Sessions $fake -ReadKey (New-ScriptedKeyReader -Keys $keys) -Draw {}
Assert-Equal 'bbbb2222' $sel.Session.SessionId 'slash filter narrows to one session and Enter opens it'

$keys = @('/', 'b', 'a', 't', 'Escape', 'Enter')
$sel = Invoke-SessionPicker -Sessions $fake -ReadKey (New-ScriptedKeyReader -Keys $keys) -Draw {}
Assert-Equal 'aaaa1111' $sel.Session.SessionId 'escape while typing clears the filter, does not exit'

# Fix 1: Ctrl+C matches Escape at both levels of the picker - leaving it at the list level,
# clearing the filter (not leaving) while typing.
$sel = Invoke-SessionPicker -Sessions $fake -ReadKey (New-MixedKeyReader -Keys @($CtrlC)) -Draw {}
Assert-Equal '' "$sel" 'ctrl+c at the list level leaves the picker with nothing, exactly like escape'

$mixedKeys = @('/', 'b', 'a', 't', $CtrlC, 'Enter')
$sel = Invoke-SessionPicker -Sessions $fake -ReadKey (New-MixedKeyReader -Keys $mixedKeys) -Draw {}
Assert-Equal 'aaaa1111' $sel.Session.SessionId 'ctrl+c while typing clears the filter instead of leaving the picker, exactly like escape'

# Fix 1: Ctrl+C returns from the maintenance screen. If it were not handled, the loop would ask
# the reader for a second key, which is empty and throws - that throw IS the failure signal.
$threw = $false
try { Invoke-MaintenanceScreen -ReadKey (New-MixedKeyReader -Keys @($CtrlC)) -Draw { param($info, $status) } }
catch { $threw = $true }
Assert-Equal $false $threw 'ctrl+c returns from the maintenance screen exactly like escape, without looping for another key'

# Captured child output must reach the layout with no invisible characters: .Length is what every
# width calculation in Layout.ps1 is built on, and an escape sequence lies about it. Claude Code
# emits no colour into a pipe today, but that is the child's choice to change, not this screen's
# to depend on.
$esc = [char]27
$clean = ConvertTo-StatusText "$esc[32mRunning: native (2.1.229)$esc[0m`n$esc[2mPlatform: win32-x64$esc[0m"
Assert-Equal 0 ([regex]::Matches($clean, [regex]::Escape("$esc")).Count) 'no escape byte survives the capture'
Assert-Equal 'Running: native (2.1.229)|Platform: win32-x64' (($clean -split "`r?`n") -join '|') 'the visible text is left exactly as it was'

# A cursor move or an erase is not colour, and Remove-AnsiColor alone would leave both behind.
$clean = ConvertTo-StatusText "$esc[2K$esc[1Gline one$esc[?25h"
Assert-Equal 'line one' $clean 'CSI sequences that are not colour are stripped too'

# Not every invisible byte arrives wrapped in a CSI sequence: a bell, or the ESC and BEL of an OSC
# title-set, count against .Length exactly the same and the CSI regex does not touch either.
#
# Both of these count bytes instead of comparing text, and that is load-bearing. Assert-Equal is
# BLIND to this whole class: PowerShell's -eq is culture-sensitive and .NET collation gives control
# characters zero weight, so `"line<BEL>one" -eq "lineone"` is $true - measured 2026-08-13, lengths
# 8 and 7. Written as an Assert-Equal against 'lineone' this assertion passed even with the
# stripping removed.
Assert-Equal 7 (ConvertTo-StatusText "line$([char]7)one").Length 'a bare control character is stripped, not only CSI sequences'
$osc = ConvertTo-StatusText "$esc]0;window title$([char]7)text"
Assert-Equal 0 ([regex]::Matches($osc, '[^\P{C}\r\n\t]').Count) 'an OSC sequence leaves no invisible byte behind'

# What `& claude doctor 2>&1` actually hands over is a stream of lines, not one string.
$clean = ConvertTo-StatusText @("$esc[1mline one$esc[0m", 'line two')
Assert-Equal 'line one|line two' (($clean -split "`r?`n") -join '|') 'a stream of lines becomes one status text'

# Two panes above the breakpoint, one column below it. The inner divider (border, divider, border
# = three vertical bars on one line) only exists when Join-Panes runs; the outer box border alone,
# which every line has regardless of layout, would leave two. Counting occurrences rather than
# presence is what makes this discriminate a mutation that disables paneing.
$glyphs = Get-Glyphs
$vChar = [string]$glyphs.V
$wideF = Get-PickerFrame -Sessions $fake -Index 0 -Filter '' -Width 120 -Height 24 -Now $now
Assert-Equal $true ((@($wideF | Where-Object { (([regex]::Matches($_, [regex]::Escape($vChar))).Count) -ge 3 }).Count) -gt 0) 'a wide terminal draws an inner divider (three vertical bars on a body line)'
$narrowF = Get-PickerFrame -Sessions $fake -Index 0 -Filter '' -Width 78 -Height 24 -Now $now
Assert-Equal 0 (@($narrowF | Where-Object { (([regex]::Matches($_, [regex]::Escape($vChar))).Count) -ge 3 }).Count) 'a narrow terminal draws no inner divider'
Assert-Equal 1 (@($narrowF | Where-Object { $_ -match '25 msgs' }).Count) 'the narrow layout header shows the selected session''s message count'

# 25 sessions with long messages, so the viewport and wrap paths are actually exercised rather than
# a fixture that happens to be short enough to hide an overflow.
$many = 1..25 | ForEach-Object {
    [pscustomobject]@{
        SessionId     = 'm{0:00}' -f $_
        Project       = "Project$_"
        Worktree      = $null
        Modified      = $now.AddHours(-$_)
        SizeBytes     = 1024 * $_
        PromptCount   = $_
        Title         = "session $_"
        LastUser      = (('investigate the failing build step ' * 6) + "case $_").Trim()
        LastAssistant = (('checked the logs and found the root cause ' * 5) + "answer $_").Trim()
    }
}

# The overflow this task found: a frame taller than the requested height scrolls the top away and
# desynchronises every later redraw. Must hold at every width, height, glyph set and list length -
# not just the one fixture that happened to fit.
foreach ($w in @(60, 78, 100, 120, 200)) {
    foreach ($h in @(16, 20, 26, 40)) {
        foreach ($list in @(@($fake[0]), $fake, $many)) {
            foreach ($ascii in @($true, $false)) {
                $f = Get-PickerFrame -Sessions $list -Index 0 -Filter '' -Width $w -Height $h -Now $now -Ascii:$ascii
                Assert-Equal $true ($f.Count -le $h) "frame fits in $h rows (width $w, $($list.Count) session(s), ascii=$ascii)"
                Assert-Equal 0 (@($f | Where-Object { $_.Length -gt $w }).Count) "no line exceeds width $w (height $h, $($list.Count) session(s), ascii=$ascii)"
            }
        }
    }
}

# Fix 2: the width loop above only proves no line is OVER width - Get-PickerFrame pipes every
# line through Limit-Line before returning, so a genuinely too-long value would just come back
# truncated with an ellipsis and that assertion would still pass. This proves the two-pane detail
# header actually has room for the longest realistic values, by checking their full text survives
# as a contiguous substring - which truncation would break.
$wideSession = [pscustomobject]@{
    SessionId = 'wide0001'; Project = 'Demo-Service'; Worktree = 'feature-widths'
    Modified = $now; SizeBytes = 12456789; PromptCount = 1234; Title = 't'; LastUser = 'u'; LastAssistant = 'a'
}
$expectedSize = '{0:N0}' -f ($wideSession.SizeBytes / 1KB)
foreach ($w in @(120, 100)) {
    $wf = Get-PickerFrame -Sessions @($wideSession) -Index 0 -Filter '' -Width $w -Height 24 -Now $now
    $header = @($wf | Where-Object { $_ -match [regex]::Escape($wideSession.Project) })
    Assert-Equal $true ($header.Count -gt 0 -and ($header | Where-Object { $_ -match [regex]::Escape($wideSession.Worktree) }).Count -gt 0) `
        "width ${w}: the detail header shows the full project AND full worktree name together, untruncated"
    Assert-Equal 1 (@($wf | Where-Object { $_ -match "$($wideSession.PromptCount) msgs" }).Count) `
        "width ${w}: the message count renders complete (four digits), not truncated"
    Assert-Equal 1 (@($wf | Where-Object { $_ -match [regex]::Escape("$expectedSize KB") }).Count) `
        "width ${w}: the size figure renders complete (five-digit KB), not truncated"
}

# The specific collapse this task found: with only 2 sessions the old $room arithmetic pinned the
# detail wrap budget to the list's own row count (2), so the preview got at most 2 lines total no
# matter how tall the terminal was. leftWidth mirrors Get-PickerFrame's own formula so the offset
# into the right pane is exact.
$leftW = [Math]::Max(28, [int]((120 - 2) * 0.4))
$offset = 1 + $leftW + 1
$room40 = Get-PickerFrame -Sessions $fake -Index 0 -Filter '' -Width 120 -Height 40 -Now $now
# Body rows sit between the top border (index 0) and the bottom border + footer (last two). The
# first three detail rows are the fixed header (where, modified/msgs, separator); everything after
# is wrapped message content - a continuation line has no marker (fix round 2), so counting marker
# glyphs alone would undercount. Count non-blank right-pane rows instead.
$body40 = $room40[1..($room40.Count - 3)]
$wrapped = @($body40 | Select-Object -Skip 3 | Where-Object { $_.Length -gt $offset -and ($_.Substring($offset).TrimEnd()) -ne '' })
Assert-Equal $true ($wrapped.Count -gt 2) 'two sessions on a 40-row terminal render more than two wrapped preview lines'

# The detail pane wraps rather than cutting at the edge.
$long = @([pscustomobject]@{ SessionId='cccc3333'; Project='Demo'; Worktree=$null; Modified=$now; SizeBytes=1024; PromptCount=2; Title='t'; LastUser=('word ' * 60).Trim(); LastAssistant='short' })
$f = Get-PickerFrame -Sessions $long -Index 0 -Filter '' -Width 120 -Height 24 -Now $now
Assert-Equal $true ((@($f | Where-Object { $_ -match 'word word' }).Count) -gt 1) 'a long last message wraps onto several detail lines'

$f = Get-PickerFrame -Sessions $fake -Index 0 -Filter '' -Width 120 -Height 24 -Now $now
Assert-Equal 1 (@($f | Where-Object { $_ -match 'feature-1' }).Count) 'the worktree name is visible in the list'

$f = Get-PickerFrame -Sessions @() -Index 0 -Filter 'zz' -Width 120 -Height 24 -Now $now
Assert-Equal 1 (@($f | Where-Object { $_ -match 'nothing' }).Count) 'an empty list says so instead of rendering blank'

# PromptCount=1, not 0: this fixture exercises the title-fallback rendering path
# (Get-SessionExchange), not the empty-session filter below - a genuinely PromptCount=0 session is
# now excluded from the picker entirely (Fix 1) and would never reach this rendering code at all.
$titleless = @([pscustomobject]@{ SessionId='dddd4444'; Project='Workbench'; Worktree=$null; Modified=$now; SizeBytes=10240; PromptCount=1; Title='(no prompt) dddd4444'; LastUser=''; LastAssistant='' })
$f = Get-PickerFrame -Sessions $titleless -Index 0 -Filter '' -Width 120 -Height 24 -Now $now
# 2, not 1: the title fallback now also drives the list snippet (fix round 2), so it appears once
# in the list row and once in the wrapped detail pane for the same session.
Assert-Equal 2 (@($f | Where-Object { $_ -match 'no prompt' }).Count) 'a session with no messages falls back to its title, in both the list and the detail pane'
Assert-Equal 1 (@($f | Where-Object { $_ -match 'no reply' }).Count) 'a session with no assistant output says so'

# --- Fix 1: sessions with zero human messages are not offered ------------------------------

$mixed = @(
    [pscustomobject]@{ SessionId='e0000001'; Project='Empty1'; Worktree=$null; Modified=$now; SizeBytes=1024; PromptCount=0; Title='(no prompt) e0000001'; LastUser=''; LastAssistant='' }
    [pscustomobject]@{ SessionId='r0000001'; Project='Real1'; Worktree=$null; Modified=$now; SizeBytes=1024; PromptCount=3; Title='a real session'; LastUser='do the thing'; LastAssistant='done' }
    [pscustomobject]@{ SessionId='e0000002'; Project='Empty2'; Worktree=$null; Modified=$now; SizeBytes=1024; PromptCount=0; Title='(no prompt) e0000002'; LastUser=''; LastAssistant='' }
)
$f = Get-PickerFrame -Sessions $mixed -Index 0 -Filter '' -Width 120 -Height 24 -Now $now
Assert-Equal 0 (@($f | Where-Object { $_ -match 'Empty1' }).Count) 'a session with zero human messages does not appear in the picker list'
Assert-Equal 0 (@($f | Where-Object { $_ -match 'Empty2' }).Count) 'neither does a second empty session'
Assert-Equal 1 (@($f | Where-Object { $_ -match 'Real1' }).Count) 'a session that was actually used still appears'
Assert-Equal 1 (@($f | Where-Object { $_ -match '1 sessions' }).Count) 'the header count reflects only the one resumable session'
Assert-Equal 1 (@($f | Where-Object { $_ -match '2 empty hidden' }).Count) 'the header states how many empty sessions were hidden, not just a shorter list'

# Navigation must never land on a hidden session: Down from the one visible row stays there
# (there is nothing else to move to), and Enter must return the real session, never an empty one.
$sel = Invoke-SessionPicker -Sessions $mixed -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow', 'Enter')) -Draw {}
Assert-Equal 'r0000001' $sel.Session.SessionId 'the picker never selects a hidden empty session, even after arrowing past where it would have sat'

# All sessions empty, no text filter: the "nothing matches" message would be misleading (there is
# no filter), so it must say what actually happened instead.
$allEmpty = @([pscustomobject]@{ SessionId='e0000003'; Project='Empty3'; Worktree=$null; Modified=$now; SizeBytes=1024; PromptCount=0; Title='(no prompt) e0000003'; LastUser=''; LastAssistant='' })
$f = Get-PickerFrame -Sessions $allEmpty -Index 0 -Filter '' -Width 120 -Height 24 -Now $now
Assert-Equal 0 (@($f | Where-Object { $_ -match 'nothing matches that filter' }).Count) 'an all-empty pool with no filter does not blame a nonexistent filter'
Assert-Equal $true ((@($f | Where-Object { $_ -match 'nothing to resume' }).Count) -gt 0) 'it says the sessions were empty instead'

# A session with real content is never hidden: PromptCount > 0 alone decides visibility.
Assert-Equal 1 (Select-ResumableSessions -Sessions $mixed).Count 'Select-ResumableSessions keeps only sessions with at least one human message'

# --- fix round 2: list snippet, single wrap marker, word-only footer -----------------------

# The bug this task exists to fix: without a snippet, every row in a project reads identically and
# the owner cannot pick a session without arrowing through all of them.
$wideListRows = @($wideF[1..2] | ForEach-Object { $_.Substring(0, [Math]::Min($_.Length, $offset)) })
Assert-Equal $true (@($wideListRows | Where-Object { $_ -match 'add resume' }).Count -ge 1) 'the wide list row for a session with a LastUser contains part of that text'
$narrowListRows = @($narrowF[1..2])
Assert-Equal $true (@($narrowListRows | Where-Object { $_ -match 'add resume' }).Count -ge 1) 'the narrow list row for a session with a LastUser contains part of that text'

# A continuation line is not a new message: exactly one attribution per speaker, however many
# physical lines the message wraps onto. Attribution is the literal word 'you', not a glyph -
# the glyph alone would fail this same test with colour off or in the ASCII set.
$f = Get-PickerFrame -Sessions $long -Index 0 -Filter '' -Width 120 -Height 24 -Now $now
$rightLines = @($f | Where-Object { $_.Length -gt $offset } | ForEach-Object { $_.Substring($offset) })
$youLines = @($rightLines | Where-Object { $_.TrimStart().StartsWith('you') })
Assert-Equal 1 $youLines.Count 'a preview wrapped onto several lines is attributed exactly once per message, not on every continuation line'

# --- detail pane: multiple messages, attribution, ordering, overflow, Cyrillic ------------

$exchange = @(
    [pscustomobject]@{ Speaker = 'user';      Text = 'first question' }
    [pscustomobject]@{ Speaker = 'assistant'; Text = 'first answer' }
    [pscustomobject]@{ Speaker = 'user';      Text = 'second question' }
    [pscustomobject]@{ Speaker = 'assistant'; Text = 'second answer' }
)
$exchangeSession = [pscustomobject]@{
    # LastUser/LastAssistant deliberately do not echo any RecentMessages text: the list pane's own
    # snippet (built from LastUser) would otherwise collide with the ordering search below.
    SessionId = 'exch0001'; Project = 'Exchange'; Worktree = $null; Modified = $now
    SizeBytes = 2048; PromptCount = 4; Title = 't'; LastUser = 'list snippet text'; LastAssistant = 'unused'
    RecentMessages = $exchange
}

function Get-FirstMatchIndex {
    param([string[]]$Lines, [string]$Pattern)
    for ($i = 0; $i -lt $Lines.Count; $i++) { if ($Lines[$i] -match $Pattern) { return $i } }
    return -1
}

# Owner asked for more than the last question and the last answer - all four messages should be
# visible on a pane this tall, oldest at the top, newest at the bottom.
$f = Get-PickerFrame -Sessions @($exchangeSession) -Index 0 -Filter '' -Width 120 -Height 40 -Now $now
$iFQ = Get-FirstMatchIndex -Lines $f -Pattern 'first question'
$iFA = Get-FirstMatchIndex -Lines $f -Pattern 'first answer'
$iSQ = Get-FirstMatchIndex -Lines $f -Pattern 'second question'
$iSA = Get-FirstMatchIndex -Lines $f -Pattern 'second answer'
Assert-Equal $true ($iFQ -ge 0 -and $iFA -gt $iFQ -and $iSQ -gt $iFA -and $iSA -gt $iSQ) 'all four recent messages render, oldest at the top and newest at the bottom'
Assert-Equal $true (($f[$iFQ]) -match 'you') 'a user message carries the literal "you" attribution'
Assert-Equal $true (($f[$iFA]) -match 'claude') 'an assistant message carries the literal "claude" attribution'

# Fix 2: the exact bug this task found - one long message must not swallow the whole pane and push
# older messages off it. The newest message here is by far the longest; the three older ones are
# one line each. On a 30-row pane (the owner's stated target) all three older messages must still
# be visible, not crowded out by the dominant one's uncapped wrap.
$dominantText = ('lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor ' * 30).Trim()
$dominant = @(
    [pscustomobject]@{ Speaker = 'user';      Text = 'first question' }
    [pscustomobject]@{ Speaker = 'assistant'; Text = 'first answer' }
    [pscustomobject]@{ Speaker = 'user';      Text = 'second question' }
    [pscustomobject]@{ Speaker = 'assistant'; Text = $dominantText }
)
$dominantSession = [pscustomobject]@{
    SessionId = 'dom00001'; Project = 'Dominant'; Worktree = $null; Modified = $now
    SizeBytes = 4096; PromptCount = 4; Title = 't'
    RecentMessages = $dominant
}
$fDom = Get-PickerFrame -Sessions @($dominantSession) -Index 0 -Filter '' -Width 120 -Height 30 -Now $now
$dFQ = Get-FirstMatchIndex -Lines $fDom -Pattern 'first question'
$dFA = Get-FirstMatchIndex -Lines $fDom -Pattern 'first answer'
$dSQ = Get-FirstMatchIndex -Lines $fDom -Pattern 'second question'
Assert-Equal $true ($dFQ -ge 0 -and $dFA -gt $dFQ -and $dSQ -gt $dFA) 'a 30-row pane still shows at least three messages when one of them is disproportionately long'
Assert-Equal $true ((@($fDom | Where-Object { $_ -match [regex]::Escape([string][char]0x2026) }).Count) -gt 0) 'the dominant long message is itself cut with a visible ellipsis rather than being dropped'

# Attribution must survive with colour off (true above already, -Color omitted) and in the ASCII
# glyph set - it cannot rely on colour or a non-ASCII glyph alone.
foreach ($ascii in @($true, $false)) {
    $fA = Get-PickerFrame -Sessions @($exchangeSession) -Index 0 -Filter '' -Width 120 -Height 40 -Now $now -Ascii:$ascii
    Assert-Equal $true ((@($fA | Where-Object { $_ -match 'you' }).Count) -gt 0) "ascii=${ascii}: the 'you' attribution survives in this glyph set"
    Assert-Equal $true ((@($fA | Where-Object { $_ -match 'claude' }).Count) -gt 0) "ascii=${ascii}: the 'claude' attribution survives in this glyph set"
}

# A message far too long for the pane is cut at a word boundary with a visible ellipsis, not
# silently dropped and not chopped mid-word.
$hugeSession = [pscustomobject]@{
    SessionId = 'huge0001'; Project = 'Huge'; Worktree = $null; Modified = $now
    SizeBytes = 4096; PromptCount = 1; Title = 't'
    RecentMessages = @([pscustomobject]@{ Speaker = 'user'; Text = (('word ' * 200)).Trim() })
}
# $script:MinHeight, not a literal 20: the minimum is measured from the LAUNCH frame (Screens.ps1)
# and moved to 21 on 2026-09-04, at which point a hardcoded 20 stopped rendering a picker at all and
# started asserting against the too-small notice - a green suite testing the wrong screen.
$f = Get-PickerFrame -Sessions @($hugeSession) -Index 0 -Filter '' -Width 100 -Height $script:MinHeight -Now $now
Assert-Equal $true ($f.Count -le ($script:MinHeight - 1)) 'a message far exceeding the pane height still produces a frame that fits the terminal with the headroom row'
$lastContentLine = @($f | Where-Object { $_ -match 'word' } | Select-Object -Last 1)[0]
# The line carries the box's own trailing border and padding, so the ellipsis is not literally the
# last character of the raw string - it must appear in the content, not necessarily at line-end.
# Indexing [0] to a scalar matters: -match on a one-element ARRAY returns the matching elements,
# not a boolean, and that array would always compare unequal to $true.
Assert-Equal $true ($lastContentLine -match [regex]::Escape([string][char]0x2026)) 'a message that overflows the pane is cut with a visible ellipsis, not silently dropped'

# Cyrillic is common in this data; the width budget and the attribution prefix must not break on it.
$cyrText = 'привет как дела сегодня что нового расскажи подробнее пожалуйста, это длинное сообщение'
$cyrSession = [pscustomobject]@{
    SessionId = 'cyr00001'; Project = 'Cyr'; Worktree = $null; Modified = $now
    SizeBytes = 1024; PromptCount = 1; Title = 't'
    RecentMessages = @([pscustomobject]@{ Speaker = 'user'; Text = $cyrText })
}
foreach ($w in @(100, 120)) {
    $f = Get-PickerFrame -Sessions @($cyrSession) -Index 0 -Filter '' -Width $w -Height 30 -Now $now
    Assert-Equal 0 (@($f | Where-Object { $_.Length -gt $w }).Count) "width ${w}: Cyrillic content does not break the width budget"
    Assert-Equal $true ((@($f | Where-Object { $_ -match 'привет' }).Count) -gt 0) "width ${w}: Cyrillic text renders intact"
}

# A session summary with no RecentMessages (an old cache entry, or a hand-built fixture) still
# renders through the same code path via the LastUser/LastAssistant fallback - not a crash, not a
# blank pane.
$noRecent = [pscustomobject]@{
    SessionId = 'norc0001'; Project = 'NoRecent'; Worktree = $null; Modified = $now
    SizeBytes = 512; PromptCount = 2; Title = 't'; LastUser = 'a question with no recent list'; LastAssistant = 'an answer with no recent list'
}
$f = Get-PickerFrame -Sessions @($noRecent) -Index 0 -Filter '' -Width 120 -Height 30 -Now $now
Assert-Equal $true ((@($f | Where-Object { $_ -match 'a question with no recent list' }).Count) -gt 0) 'a session without RecentMessages still shows its last question, via the fallback'
Assert-Equal $true ((@($f | Where-Object { $_ -match 'an answer with no recent list' }).Count) -gt 0) 'a session without RecentMessages still shows its last answer, via the fallback'

# Doubling a glyph reads as a typo, not an instruction ("move with up/down").
$doubledCursor = [string]$glyphs.Cursor + [string]$glyphs.Cursor
$pickerFooter = @(Get-PickerFrame -Sessions $fake -Index 0 -Filter '' -Width 120 -Height 24 -Now $now)[-1]
Assert-Equal $false ($pickerFooter -match [regex]::Escape($doubledCursor)) 'the picker footer contains no doubled cursor glyph'
$launchFooter = @(Get-LaunchFrame -State (New-LaunchState) -Width 120 -Height 24)[-1]
Assert-Equal $false ($launchFooter -match [regex]::Escape($doubledCursor)) 'the launch footer contains no doubled cursor glyph'

# --- Task 8: the session picker scoped to the chosen project -------------------------------

# Correction 2: Select-SessionMatch was `-like` with no escaping, exactly the defect
# Select-ProjectMatch (Projects.ps1) already carries the fix for - a bare '[' in the filter is an
# unmatched character class and raises a terminating WildcardPatternException through the render
# loop. Proven the same way check-hooks-fire proves a gate: the call must not throw AND must
# return zero matches (no session text here contains a literal '[').
$bracketThrew = $false
try { $bracketMatches = @(Select-SessionMatch -Sessions $fake -Filter '[') }
catch { $bracketThrew = $true }
Assert-Equal $false $bracketThrew 'Select-SessionMatch does not throw on a filter of a single ['
Assert-Equal 0 $bracketMatches.Count 'and a lone [ matches nothing, since no fixture session contains one literally'

# Correction 1: two sessions can share a Project NAME (not unique) while living in different
# repositories - only Slug (the transcript directory name) is exact. Scoping by slug must show
# only the matching one, never fall back to name matching just because a name collided.
$sharedNameA = [pscustomobject]@{ SessionId='sn000001'; Slug='SlugA'; Project='Shared'; Worktree=$null; Modified=$now; SizeBytes=10; PromptCount=1; Title='a'; LastUser='session a text'; LastAssistant='reply a' }
$sharedNameB = [pscustomobject]@{ SessionId='sn000002'; Slug='SlugB'; Project='Shared'; Worktree=$null; Modified=$now; SizeBytes=10; PromptCount=1; Title='b'; LastUser='session b text'; LastAssistant='reply b' }
$sharedName = @($sharedNameA, $sharedNameB)

$slugSel = Invoke-SessionPicker -Sessions $sharedName -ProjectSlug 'SlugA' -ReadKey (New-ScriptedKeyReader -Keys @('Enter')) -Draw {}
Assert-Equal 'sn000001' $slugSel.Session.SessionId 'scoping by slug shows only the matching session, even though both share a Project name'

# Tab widens from the scoped project to every session, and back.
$slugTabSel = Invoke-SessionPicker -Sessions $sharedName -ProjectSlug 'SlugA' -ReadKey (New-ScriptedKeyReader -Keys @('Tab', 'DownArrow', 'Enter')) -Draw {}
Assert-Equal 'sn000002' $slugTabSel.Session.SessionId 'Tab widens the scope to all projects, so the second (same-name, different-slug) session becomes reachable'

# With no slug known at all (an unrecognised cwd), scoping falls back to Project by name.
$nameFallbackSel = Invoke-SessionPicker -Sessions $fake -ProjectName 'Workbench' -ReadKey (New-ScriptedKeyReader -Keys @('Enter')) -Draw {}
Assert-Equal 'aaaa1111' $nameFallbackSel.Session.SessionId 'with no slug known, scoping falls back to matching Project by name'

# Fix round 1, MINOR: the Tab key is built the way a real terminal actually sends it (KeyChar TAB,
# ConsoleKey.Tab), not New-ScriptedKeyReader's synthetic zero-char stand-in for a multi-letter key
# name - so this assertion exercises the real character path through the $typing branch's KeyChar
# guard, not merely a $key.Key match. Tab inside typing mode is not a toggle (global constraint):
# scoped to SlugA (one session visible); if Tab secretly widened the scope while typing, DownArrow
# after Escape would have a second session to move to and Enter would return it instead.
$RealTabKey = [System.ConsoleKeyInfo]::new([char]9, [System.ConsoleKey]::Tab, $false, $false, $false)
$typingTabSel = Invoke-SessionPicker -Sessions $sharedName -ProjectSlug 'SlugA' -ReadKey (New-MixedKeyReader -Keys @('/', $RealTabKey, 'Escape', 'DownArrow', 'Enter')) -Draw {}
Assert-Equal 'sn000001' $typingTabSel.Session.SessionId 'a real Tab keystroke while typing a filter does not toggle the scope - DownArrow afterwards has nothing else to move to'

# With no slug AND no name at all, there is no "other" scope - Tab does nothing (proven by the
# session count never changing: only one of the two sessions is reachable either way is wrong here,
# so instead this proves Tab is simply inert by getting the same, unscoped result before and after).
$noScopeSel = Invoke-SessionPicker -Sessions $sharedName -ReadKey (New-ScriptedKeyReader -Keys @('Tab', 'Enter')) -Draw {}
Assert-Equal 'sn000001' $noScopeSel.Session.SessionId 'with neither -ProjectSlug nor -ProjectName, Tab does nothing and the picker behaves exactly as before this task'

# Fix round 1, IMPORTANT 4a: with BOTH -ProjectSlug and -ProjectName given, slug alone decides the
# scope - a session that matches only by NAME under a FOREIGN slug must stay excluded, or an
# implementation that silently ORs the two together (`Slug -eq X -or (Name -and Project -eq Name)`)
# would pass every assertion above (none of them supply both parameters at once). The scoped pool
# must be exactly one session (sn000001), proven by two DownArrows still landing on it.
$foreignSlugSameName = [pscustomobject]@{ SessionId='sn000003'; Slug='SlugC'; Project='Shared'; Worktree=$null; Modified=$now; SizeBytes=10; PromptCount=1; Title='c'; LastUser='session c text'; LastAssistant='reply c' }
$bothParamsSessions = @($sharedNameA, $sharedNameB, $foreignSlugSameName)
$bothParamsSel = Invoke-SessionPicker -Sessions $bothParamsSessions -ProjectSlug 'SlugA' -ProjectName 'Shared' -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow', 'DownArrow', 'Enter')) -Draw {}
Assert-Equal 'sn000001' $bothParamsSel.Session.SessionId 'with both -ProjectSlug and -ProjectName given, slug alone decides scope - a session matching only by name under a foreign slug is excluded, so two DownArrows past the one visible session still land on it'

# Fix round 1, IMPORTANT 4b: the name-FALLBACK branch (no slug at all) is an EXACT match, not a
# substring - a session whose Project merely CONTAINS the name ('api-legacy' contains 'api') must
# stay excluded, or a `-like "*$ProjectName*"` mutant would pass every fallback assertion above
# (none of them have a superstring collision).
$exactNameA = [pscustomobject]@{ SessionId='api00001'; Project='api'; Worktree=$null; Modified=$now; SizeBytes=10; PromptCount=1; Title='a'; LastUser='api session text'; LastAssistant='reply' }
$exactNameB = [pscustomobject]@{ SessionId='api00002'; Project='api-legacy'; Worktree=$null; Modified=$now; SizeBytes=10; PromptCount=1; Title='b'; LastUser='legacy session text'; LastAssistant='reply' }
$exactNameSel = Invoke-SessionPicker -Sessions @($exactNameA, $exactNameB) -ProjectName 'api' -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow', 'Enter')) -Draw {}
Assert-Equal 'api00001' $exactNameSel.Session.SessionId 'the name fallback is an exact match - a session whose Project only CONTAINS the name (api-legacy) stays out of the scoped pool, so DownArrow has nowhere else to go'

# Fix round 1, IMPORTANT 5: Tab must reset the index, not merely change the scope - every Tab test
# above starts at index 0, so deleting the reset would survive all of them. Scoped by NAME (both
# sessions are in the initial pool), DownArrow selects the second, Tab widens to 'all' (still both
# sessions, same order): without the reset Enter would return the second (carried-over index); with
# it, the first.
$tabResetSel = Invoke-SessionPicker -Sessions $sharedName -ProjectName 'Shared' -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow', 'Tab', 'Enter')) -Draw {}
Assert-Equal 'sn000001' $tabResetSel.Session.SessionId 'Tab resets the index to 0 - otherwise the second session (selected by DownArrow before the toggle) would still be current after it'

# Fix round 1, IMPORTANT 1: -Scope 'none' (and the default, now 'none' rather than 'all') renders
# NO tab hint at all - Invoke-SessionPicker only ever reaches Tab's $hasScope guard when it has a
# project to scope by, so a hint that always does nothing would be permanently dead weight, and at
# HEAD it was costing a wrapped footer line (2, not the pre-task 1) at 80 columns for every user.
$noneScopeText = (@(Get-PickerFrame -Sessions $sharedName -Index 0 -Width 78 -Height 24) -join "`n")
Assert-True ($noneScopeText -notmatch 'tab') 'the default scope (none) has no tab hint at all'
$noneScopeExplicitText = (@(Get-PickerFrame -Sessions $sharedName -Index 0 -Scope 'none' -Width 78 -Height 24) -join "`n")
Assert-True ($noneScopeExplicitText -notmatch 'tab') 'an explicit -Scope none has no tab hint either'
$noneMap = $null
$null = Get-PickerFrame -Sessions $sharedName -Index 0 -Width 78 -Height 24 -RowMap ([ref]$noneMap)
Assert-Equal 1 $noneMap.FooterLines 'the default (none) scope footer is exactly 1 line at width 78 - the pre-Task-8 count, tab hint or not'
$noneMapExplicit = $null
$null = Get-PickerFrame -Sessions $sharedName -Index 0 -Scope 'none' -Width 78 -Height 24 -RowMap ([ref]$noneMapExplicit)
Assert-Equal 1 $noneMapExplicit.FooterLines 'an explicit -Scope none footer is exactly 1 line at width 78 too'

# Get-PickerFrame -Scope 'project'/'all': the footer advertises the toggle, and its label names the
# OTHER scope. Two separate .Contains-shaped checks rather than one 'tab.*all projects' regex:
# -Plain (colour off, the default here) renders the token bracketed ('[tab]'), not padded ('tab '),
# so a pattern assuming a literal space right after "tab" would fail on the very form this produces.
$fProjectScopeText = (@(Get-PickerFrame -Sessions $sharedName -Index 0 -Scope 'project' -Width 78 -Height 24) -join "`n")
Assert-True ($fProjectScopeText -match 'tab') 'in project scope the footer advertises a tab hint'
Assert-True ($fProjectScopeText -match 'all projects') 'in project scope the tab hint offers to widen to all projects'
$fAllScopeText = (@(Get-PickerFrame -Sessions $sharedName -Index 0 -Scope 'all' -Width 78 -Height 24) -join "`n")
Assert-True ($fAllScopeText -match 'tab') 'in all scope the footer advertises a tab hint'
Assert-True ($fAllScopeText -match 'this project') 'in all scope the tab hint offers to narrow to this project'

# -Index 1 selects session B in every scope-frame check below, so the wide layout's own
# detail/preview header (unchanged by this task - it names ONE selected session, not a repeated
# list column) only ever prints session B's project name - it can never be the source of a
# 'FirstProj' match, which isolates every check to session A's LIST row, the thing this task
# actually changes.
$scopeFixtureA = [pscustomobject]@{ SessionId='scp0001'; Project='FirstProj'; Worktree=$null; Modified=$now; SizeBytes=10; PromptCount=1; Title='a'; LastUser='alpha snippet text'; LastAssistant='reply a' }
$scopeFixtureB = [pscustomobject]@{ SessionId='scp0002'; Project='SecondProj'; Worktree=$null; Modified=$now; SizeBytes=10; PromptCount=1; Title='b'; LastUser='beta snippet text'; LastAssistant='reply b' }
$scopeFixture = @($scopeFixtureA, $scopeFixtureB)

# Fix round 1, IMPORTANT 3: -ProjectName is the display label the comment on Invoke-SessionPicker's
# parameter always claimed it was - it must actually reach the title under -Scope project, and
# never appear at all under -Scope all (where there is no single project to name). A sentinel name
# that matches none of the fixture sessions' own Project fields, so a hit can only come from the
# title itself, never a list row.
$titleFrameProject = @(Get-PickerFrame -Sessions $scopeFixture -Index 1 -Scope 'project' -ProjectName 'MyNamedProject' -Width 120 -Height 24)
Assert-True ($titleFrameProject[0] -match 'MyNamedProject') 'under -Scope project, -ProjectName appears in the title'
$titleFrameAll = @(Get-PickerFrame -Sessions $scopeFixture -Index 1 -Scope 'all' -ProjectName 'MyNamedProject' -Width 120 -Height 24)
Assert-Equal 0 (@($titleFrameAll | Where-Object { $_ -match 'MyNamedProject' }).Count) 'under -Scope all, -ProjectName does not appear anywhere in the frame'

# Get-PickerFrame -Scope 'project' drops the project-name column from the list rows (every row is
# assumed to be the same project); -Scope 'all' (and the default 'none', unchanged from before this
# task) still shows it.
$scopeAllFrame = Get-PickerFrame -Sessions $scopeFixture -Index 1 -Filter '' -Scope 'all' -Width 120 -Height 24 -Now $now
Assert-Equal 1 (@($scopeAllFrame | Where-Object { $_ -match 'FirstProj' }).Count) 'scope all: the non-selected session''s project name is visible in its own list row'
$scopeProjectFrame = Get-PickerFrame -Sessions $scopeFixture -Index 1 -Filter '' -Scope 'project' -Width 120 -Height 24 -Now $now
Assert-Equal 0 (@($scopeProjectFrame | Where-Object { $_ -match 'FirstProj' }).Count) 'scope project: the project-name column is dropped from the list row'
$scopeDefaultFrame = Get-PickerFrame -Sessions $scopeFixture -Index 1 -Filter '' -Width 120 -Height 24 -Now $now
Assert-Equal 1 (@($scopeDefaultFrame | Where-Object { $_ -match 'FirstProj' }).Count) '-Scope defaults to none, unchanged from before this task'

# Fix round 1, IMPORTANT 2: the mutant `Screens.ps1:929` (narrow branch) -> `$where = $s.Project`
# survived because both scope-frame assertions above only ever used -Width 120 (the wide branch).
# Repeat the same FirstProj absence/presence check at 78 and at 50 (the minimum), which force the
# NARROW branch's own, separately-coded `$where` line.
foreach ($narrowWidth in @(78, 50)) {
    $narrowHeight = if ($narrowWidth -eq 50) { 21 } else { 24 }
    $scopeAllNarrow = Get-PickerFrame -Sessions $scopeFixture -Index 1 -Filter '' -Scope 'all' -Width $narrowWidth -Height $narrowHeight -Now $now
    Assert-Equal 1 (@($scopeAllNarrow | Where-Object { $_ -match 'FirstProj' }).Count) "width ${narrowWidth} (narrow branch): scope all shows the non-selected session's project name"
    $scopeProjectNarrow = Get-PickerFrame -Sessions $scopeFixture -Index 1 -Filter '' -Scope 'project' -Width $narrowWidth -Height $narrowHeight -Now $now
    Assert-Equal 0 (@($scopeProjectNarrow | Where-Object { $_ -match 'FirstProj' }).Count) "width ${narrowWidth} (narrow branch): scope project drops the project-name column from the list row"
}

# Fix round 2, item 1: -ProjectName forwarding through the LOOP's own $Draw call (Ui.ps1:501) was
# unpinned end to end - deleting the argument there left every earlier -ProjectName assertion green,
# because they all called Get-PickerFrame directly. A capturing 5-parameter -Draw proves the loop
# itself hands the name through as the 5th positional argument, under -Scope project.
$script:capturedDrawProjectName = 'not called'
$capturingNameDraw = { param($s, $i, $f, $sc, $pn) $script:capturedDrawProjectName = $pn; $null }
$null = Invoke-SessionPicker -Sessions $sharedName -ProjectSlug 'SlugA' -ProjectName 'Shared' -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw $capturingNameDraw
Assert-Equal 'Shared' $script:capturedDrawProjectName 'Invoke-SessionPicker forwards -ProjectName to $Draw as its 5th argument'

# Fix round 2, item 2: the separator reclaim (Screens.ps1, both list branches) was unpinned -
# reverting either branch's `$sep` back to a fixed '  ' left every earlier assertion green, because
# none of them checked WHERE the snippet actually starts, only whether the project name text was
# present or absent. Pinned on the raw measurable: the column index of the snippet's own first word
# ('alpha', from scopeFixtureA's LastUser) within its row. Measured, not guessed: scope all puts it
# at column 15 (1 border + 3 mark + 'FirstProj' (9) + '  ' (2) separator); scope project puts it at
# column 4 (1 border + 3 mark, no separator at all - the whole 11-cell gap this task frees over to
# the snippet). Same numbers hold at both a narrow (78) and a wide, two-pane (120) width, since both
# branches share the identical mark+where+sep composition.
foreach ($sepWidth in @(78, 120)) {
    $allSepRow = @(Get-PickerFrame -Sessions $scopeFixture -Index 1 -Scope 'all' -Width $sepWidth -Height 24 -Now $now | Where-Object { $_ -match 'alpha' })[0]
    Assert-Equal 15 $allSepRow.IndexOf('alpha') "width ${sepWidth}: scope all - the snippet starts after mark + project name + the two-space separator"
    $projectSepRow = @(Get-PickerFrame -Sessions $scopeFixture -Index 1 -Scope 'project' -Width $sepWidth -Height 24 -Now $now | Where-Object { $_ -match 'alpha' })[0]
    Assert-Equal 4 $projectSepRow.IndexOf('alpha') "width ${sepWidth}: scope project - the snippet starts immediately after the mark; the separator itself is reclaimed, not just the project name"
}

# Fix round 2, item 3: -ProjectName in the title was only ever asserted at width 120, where a name
# always fits whole. -ProjectName is appended LAST in the title string, so at the 50-column minimum
# a long (34-character) name may be cut by Limit-Line - what must survive is the session COUNT,
# which sits earlier in the same string and is the one thing an owner glancing at a truncated title
# still needs to see.
$longProjectName34 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ12345678'
Assert-Equal 34 $longProjectName34.Length 'sanity: the fixture name really is 34 characters'
# $script:MinHeight, not a literal 21 (fix round 1, MINOR 2): the minimum dropped to 20 when the
# Action row left the launch screen (Task 9) - a hardcoded 21 would still pass here but would have
# quietly stopped being "the minimum" it claimed to be.
$narrowTitleFrame = @(Get-PickerFrame -Sessions $scopeFixture -Index 1 -Scope 'project' -ProjectName $longProjectName34 -Width 50 -Height $script:MinHeight -Now $now)
Assert-True ($narrowTitleFrame[0] -match '2 sessions') 'at width 50 (the minimum height) with a 34-character -ProjectName, the session count still survives in the (truncated) title'

# Fix round 2, item 4: Get-PickerFrame never clamped -Index to the FILTERED item count the way
# Get-ProjectFrame already clamps its own -Index to -Rows.Count - a caller passing an -Index that a
# -Filter has since made too large (production is shielded by Invoke-SessionPicker's own re-clamp
# every loop iteration; a direct caller, including a future screen or a test, is not) threw "You
# cannot call a method on a null-valued expression" building the narrow branch's single-session
# preview header ($items[$Index].Modified.ToString(...) on a null $items[$Index]).
$clampThrew = $false
try { $clampedFrame = @(Get-PickerFrame -Sessions $scopeFixture -Index 1 -Filter 'FirstProj' -Width 120 -Height 24 -Now $now) }
catch { $clampThrew = $true }
Assert-Equal $false $clampThrew 'an -Index past the filtered item count is clamped, not left to throw'
Assert-True (($clampedFrame -join "`n") -match 'FirstProj') 'and the clamped frame still renders the one session the filter actually matched'

# Global constraint 3: the scrolled Start must be the ACTUAL first visible index, never the
# degenerate 0 a mutant substituting a hardcoded value would leave in place. Measured, not guessed,
# by rendering 30 sessions at the last row (Index 29) and reading the row map back - once for the
# narrow branch at 78x24 and at the 50-column minimum, and once for the wide (two-pane) branch at
# 120x24, since both branches carry their own `Start = $vp.Start` assignment. Re-measured after fix
# round 1 (the default scope's footer lost the always-dead tab hint, changing $bodyRows at 78x24)
# and again after Task 9 fix round 1, MINOR 2 (the minimum dropped 21 -> 20 with the Action row
# gone) - $script:MinHeight throughout, never a literal, so this re-measures itself.
$scrollSessions = 1..30 | ForEach-Object {
    [pscustomobject]@{
        SessionId = 'scr{0:00}' -f $_; Project = "ScrollProject$_"; Worktree = $null
        Modified = $now.AddHours(-$_); SizeBytes = 1024 * $_; PromptCount = $_
        Title = "session $_"; LastUser = "case $_"; LastAssistant = "answer $_"
    }
}
$mapWide78 = $null
$fWide78 = @(Get-PickerFrame -Sessions $scrollSessions -Index 29 -Width 78 -Height 24 -Now $now -RowMap ([ref]$mapWide78))
Assert-Equal 1 $mapWide78.FirstRowY 'measured at 78x24, index 29 of 30 (narrow branch): FirstRowY'
Assert-Equal 14 $mapWide78.RowCount 'measured at 78x24, index 29 of 30 (narrow branch): RowCount'
Assert-Equal 16 $mapWide78.Start 'measured at 78x24, index 29 of 30 (narrow branch): the scrolled Start is the actual first visible index, not the degenerate 0 a mutant would substitute'
Assert-Equal $true ($fWide78.Count -le 24) 'the 78x24 scrolled frame still fits the terminal'

$mapMin50 = $null
$fMin50 = @(Get-PickerFrame -Sessions $scrollSessions -Index 29 -Width 50 -Height $script:MinHeight -Now $now -RowMap ([ref]$mapMin50))
Assert-Equal 1 $mapMin50.FirstRowY 'measured at 50 columns, the minimum height, index 29 of 30 (narrow branch): FirstRowY'
Assert-Equal 9 $mapMin50.RowCount 'measured at the minimum size, index 29 of 30 (narrow branch): RowCount'
Assert-Equal 21 $mapMin50.Start 'measured at the minimum size, index 29 of 30 (narrow branch): the scrolled Start is the actual first visible index, not the degenerate 0 a mutant would substitute'
Assert-Equal $true ($fMin50.Count -le ($script:MinHeight - 1)) 'the 50-column scrolled frame still fits the minimum terminal size'

$mapPane120 = $null
$fPane120 = @(Get-PickerFrame -Sessions $scrollSessions -Index 29 -Width 120 -Height 24 -Now $now -RowMap ([ref]$mapPane120))
Assert-Equal 1 $mapPane120.FirstRowY 'measured at 120x24, index 29 of 30 (wide branch): FirstRowY'
Assert-Equal 20 $mapPane120.RowCount 'measured at 120x24, index 29 of 30 (wide branch): RowCount'
Assert-Equal 10 $mapPane120.Start 'measured at 120x24, index 29 of 30 (wide branch): the scrolled Start is the actual first visible index, not the degenerate 0 a mutant would substitute'
Assert-Equal $true ($fPane120.Count -le 24) 'the 120x24 scrolled frame still fits the terminal'

# Colour identity for the picker, at both layouts. The footer's key caps are the one deliberate
# exception (Task 4): bracketed with colour off, padded with it on - same WIDTH either way, never
# the same characters - so those lines are compared normalised (brackets -> padding) rather than
# byte for byte.
foreach ($w in @(80, 120)) {
    $pMapPlain = $null
    $plainP   = Get-PickerFrame -Sessions $fake -Index 0 -Filter '' -Width $w -Height 24 -Now $now -RowMap ([ref]$pMapPlain)
    $coloredP = Get-PickerFrame -Sessions $fake -Index 0 -Filter '' -Width $w -Height 24 -Now $now -Color
    $footerFrom = $pMapPlain.FooterY
    $footerTo = $footerFrom + $pMapPlain.FooterLines - 1
    # Pin the window before trusting it: an unpinned FooterY/FooterLines that ever drifted WIDE
    # would make the skip loop swallow the whole frame and index $plainP[$i] past the end, where
    # $null -replace ... is '' and Remove-AnsiColor $null is '' - both loops below would then pass
    # on an empty comparison instead of a real one.
    # Measured at width $w: one footer line. This call passes no -Scope, so Get-PickerFrame's
    # default ('none' - fix round 1, IMPORTANT 1) renders exactly the pre-Task-8 five hints; the
    # tab hint only ever appears under -Scope project/all, which is covered separately above.
    Assert-Equal 1 $pMapPlain.FooterLines "width ${w}: the picker footer is exactly one line"
    Assert-Equal $true ($footerTo -lt $plainP.Count) "width ${w}: the footer window stays inside the frame"
    $mismatch = 0
    for ($i = 0; $i -lt $plainP.Count; $i++) {
        if ($i -ge $footerFrom -and $i -le $footerTo) { continue }
        if ((Remove-AnsiColor -Text $coloredP[$i]) -ne $plainP[$i]) { $mismatch++ }
    }
    Assert-Equal 0 $mismatch "stripping colour returns the plain picker frame exactly at width $w, outside the footer's key caps"
    $footerMismatch = 0
    for ($i = $footerFrom; $i -le $footerTo; $i++) {
        $normalizedPlain = (($plainP[$i] -replace '\[', ' ') -replace '\]', ' ')
        if ($normalizedPlain -ne (Remove-AnsiColor -Text $coloredP[$i])) { $footerMismatch++ }
    }
    Assert-Equal 0 $footerMismatch "stripping colour and normalising brackets-to-padding matches the plain picker footer at width $w"
}

# --- colour ------------------------------------------------------------------------------
# The whole point of painting after layout: stripping the escapes must give back exactly the
# plain frame. If this ever fails, colour has started corrupting the width arithmetic.

$lMapPlain = $null
$plain   = Get-LaunchFrame -State (New-LaunchState) -Width 84 -Height 24 -Limits @{ work = [pscustomobject]@{ FiveHour = 15; SevenDay = 97; AgeText = 'just now' } } -RowMap ([ref]$lMapPlain)
$colored = Get-LaunchFrame -State (New-LaunchState) -Width 84 -Height 24 -Limits @{ work = [pscustomobject]@{ FiveHour = 15; SevenDay = 97; AgeText = 'just now' } } -Color
Assert-Equal $plain.Count $colored.Count 'colouring does not change the number of lines'
# Same exception as the picker above: the footer's key caps are bracketed here (colour off) and
# padded there (colour on) - same width, deliberately different characters - so it is normalised
# rather than compared byte for byte.
$footerFrom = $lMapPlain.FooterY
$footerTo = $footerFrom + $lMapPlain.FooterLines - 1
# Same pin as the picker above: an unpinned window that drifted wide would make both comparison
# loops below pass vacuously (an empty or past-the-end comparison), on a frame that never actually
# got compared.
# w/s and a/d (2026-09-09) are shorter than up/down and left/right - the footer that used to wrap
# to two lines at width 84 now fits on one. Re-measured, not guessed.
Assert-Equal 1 $lMapPlain.FooterLines 'the launch footer at width 84 is exactly one line now that the arrow hints are w/s and a/d'
Assert-Equal $true ($footerTo -lt $plain.Count) 'the footer window stays inside the frame'
$mismatch = 0
for ($i = 0; $i -lt $plain.Count; $i++) {
    if ($i -ge $footerFrom -and $i -le $footerTo) { continue }
    if ((Remove-AnsiColor -Text $colored[$i]) -ne $plain[$i]) { $mismatch++ }
}
Assert-Equal 0 $mismatch 'stripping colour returns the plain launch frame exactly, outside the footer key caps'
$footerMismatch = 0
for ($i = $footerFrom; $i -le $footerTo; $i++) {
    $normalizedPlain = (($plain[$i] -replace '\[', ' ') -replace '\]', ' ')
    if ($normalizedPlain -ne (Remove-AnsiColor -Text $colored[$i])) { $footerMismatch++ }
}
Assert-Equal 0 $footerMismatch 'stripping colour and normalising brackets-to-padding matches the plain launch footer'
Assert-Equal $true ([bool](@($colored | Where-Object { $_ -match "$([char]27)\[" }).Count -gt 0)) 'colour actually emitted escapes'

# Percentages carry meaning, not decoration: a 97% week must not look like a 15% one.
Assert-Equal "$([char]27)[31m" (Get-PercentColor -Percent 97) '97% is red'
Assert-Equal "$([char]27)[33m" (Get-PercentColor -Percent 72) '72% is yellow'
Assert-Equal "$([char]27)[32m" (Get-PercentColor -Percent 15) '15% is green'

# Painting must be skippable for terminals and users that do not want it.
$noColor = Get-LaunchFrame -State (New-LaunchState) -Width 84 -Height 24 -Limits @{}
Assert-Equal 0 (@($noColor | Where-Object { $_ -match "$([char]27)\[" }).Count) 'no escapes unless colour is requested'

# --- Get-FriendlyModelName / Get-DefaultModelLabel (Env.ps1) -----------------------------------
# Direct unit coverage for the label shortener itself - the width/collapse assertions further
# below exercise Get-LaunchFrame with an already-shortened label passed in directly, so on their
# own they cannot tell a working shortener from a mutated one that always returns its input
# unchanged. This block is what makes that mutation visible.
Assert-Equal 'Fable 5.1[1M]' (Get-FriendlyModelName -Raw 'claude-fable-5-1[1m]') 'friendly name: alias-in-full-id form, case-insensitive, plus the [1M] suffix'
Assert-Equal 'Fable 5.1' (Get-FriendlyModelName -Raw 'fable') 'friendly name: the bare fable alias (what /model writes) is the current Fable release'
Assert-Equal 'Opus 5[1M]' (Get-FriendlyModelName -Raw 'opus[1m]') 'friendly name: bare alias form also resolves'
Assert-Equal 'Sonnet 5' (Get-FriendlyModelName -Raw 'claude-sonnet-4-6') 'friendly name: a non-1M full id has no [1M] suffix appended'
Assert-Equal 'Haiku 4.5' (Get-FriendlyModelName -Raw 'haiku') 'friendly name: haiku maps too'
$longUnknown = 'some-unknown-model-id-xyz-12345678'
$unknownResult = Get-FriendlyModelName -Raw $longUnknown
Assert-Equal $true ($unknownResult.Length -le 24) 'friendly name: an unrecognised family is truncated, never left to blow up the row width'
Assert-Equal $true ($longUnknown.StartsWith($unknownResult.TrimEnd([char]0x2026))) 'friendly name: the truncated-unknown fallback is a real prefix of the raw id, not a summary'

$defaultModelFixture = Join-Path $env:TEMP ('claude-auto-model-label-' + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.json')
try {
    '{"model": "claude-fable-5-1[1m]"}' | Set-Content -LiteralPath $defaultModelFixture -Encoding utf8
    Assert-Equal 'default (Fable 5.1[1M])' (Get-DefaultModelLabel -Path $defaultModelFixture) 'default model label: resolves through the friendly-name shortener, not the raw id'

    '{"includeCoAuthoredBy": false}' | Set-Content -LiteralPath $defaultModelFixture -Encoding utf8
    Assert-Equal 'default (account default)' (Get-DefaultModelLabel -Path $defaultModelFixture) 'default model label: no model key falls back to account default'

    'not valid json {{{' | Set-Content -LiteralPath $defaultModelFixture -Encoding utf8
    Assert-Equal 'default' (Get-DefaultModelLabel -Path $defaultModelFixture) 'default model label: unparseable settings.json degrades to plain default, never throws'
} finally {
    Remove-Item -LiteralPath $defaultModelFixture -ErrorAction SilentlyContinue
}
Assert-Equal 'default' (Get-DefaultModelLabel -Path (Join-Path $env:TEMP ('claude-auto-model-label-missing-' + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.json'))) 'default model label: a missing settings.json degrades to plain default'

# --- model row: shortened default label fits at 100/120, still collapses at 60/80 --------------
# The model row's five options must all render in full form once the resolved default label is
# shortened to a friendly name - at width 100/120 the box caps at 100 columns (inner 96), and the
# old raw '--model' id alone ('claude-fable-5[1m]', 28 chars) left no room for all five before
# Get-LaunchFrame's own overflow guard collapsed the row to the compact '‹ selected ›' form. That
# guard is intentionally kept - width 60/80 must still collapse - only the label length changed.
$widthState = New-LaunchState
$widthState.Row = (Get-RowIndex -Name 'Model')
foreach ($w in @(100, 120)) {
    $frame = Get-LaunchFrame -State $widthState -Width $w -Height 24 -DefaultModelLabel 'default (Fable 5.1[1M])'
    $line = @($frame | Where-Object { $_ -match 'model' })[0]
    $hasAllFive = ($line -match [regex]::Escape('default (Fable 5.1[1M])')) -and ($line -match [regex]::Escape(' Fable 5.1 ')) -and
                  ($line -match [regex]::Escape('Opus 5[1M]')) -and ($line -match [regex]::Escape('Sonnet 5[1M]')) -and
                  ($line -match [regex]::Escape('Haiku 4.5')) -and ($line -notmatch [regex]::Escape($glyphs.LAngle))
    Assert-Equal $true $hasAllFive "at width $w the model row shows all five options in full form, not collapsed"
}
foreach ($w in @(60, 80)) {
    $frame = Get-LaunchFrame -State $widthState -Width $w -Height 24 -DefaultModelLabel 'default (Fable 5.1[1M])'
    $line = @($frame | Where-Object { $_ -match 'model' })[0]
    Assert-Equal $true ($line -match [regex]::Escape($glyphs.LAngle)) "at width $w the model row falls back to the compact single-selection form"
}

# --- nested-bracket highlight (model row: labels can carry their own literal brackets) ---------
# A selected label like 'Opus 5[1M]' becomes the cell "<On-glyph> [Opus 5[1M]]" - one bracket
# nested inside another. A highlight regex that matches ANY '[...]' pair, not anchored to the
# selection marker, lands on the wrong span: the INNER '[1M]' instead of the whole selected cell,
# and it goes on to paint an UNselected cell's own literal bracket text ('Sonnet 5[1M]', shown but
# not chosen) as if it were selected too. The pre-existing strip-colour round trip alone cannot
# catch either failure - stripping ANSI gives back the same plain text either way, since only the
# highlight PLACEMENT is wrong, not the underlying characters. Both properties below must hold.
$nestedState = New-LaunchState
$nestedState.Model = 'opus1m'
$nestedState.Row = (Get-RowIndex -Name 'Model')
$nestedPlain = Get-LaunchFrame -State $nestedState -Width 100 -Height 24 -DefaultModelLabel 'default (Fable 5.1[1M])'
$nestedColored = Get-LaunchFrame -State $nestedState -Width 100 -Height 24 -DefaultModelLabel 'default (Fable 5.1[1M])' -Color
$nestedModelPlain = @($nestedPlain | Where-Object { $_ -match 'model' })[0]
$nestedModelColored = @($nestedColored | Where-Object { $_ -match 'model' })[0]

# Property 1 (baseline, kept): the round trip still holds - painting must never touch the text.
Assert-Equal $nestedModelPlain (Remove-AnsiColor -Text $nestedModelColored) 'nested-bracket model row: stripping colour returns the plain row exactly'

# Property 2: the highlight covers EXACTLY the selected label, brackets included, and nothing else.
$expectedHighlight = $script:C.Bold + $script:C.Green + '[Opus 5[1M]]' + $script:C.Reset
Assert-Equal $true ($nestedModelColored.Contains($expectedHighlight)) 'nested-bracket model row: the highlight wraps the whole selected label, brackets included'

# The specific old-bug shape: only the inner '[1M]' of the selected cell gets its own wrapper.
$innerOnlyHighlight = $script:C.Bold + $script:C.Green + '[1M]' + $script:C.Reset
Assert-Equal $false ($nestedModelColored.Contains($innerOnlyHighlight)) 'nested-bracket model row: the inner [1M] alone is not separately highlighted'

# The other old-bug shape: an unselected cell's own literal bracket text painted as if selected.
$oldBugArtifact = 'Sonnet 5' + $script:C.Bold + $script:C.Green + '[1M]' + $script:C.Reset
Assert-Equal $false ($nestedModelColored.Contains($oldBugArtifact)) 'nested-bracket model row: the unselected Sonnet 5[1M] cell is not painted as if it were selected'

# Property 3: the specific failure named in review - a bracket-matching character class that
# allows '[' can, in a LATER pass, swallow half of an earlier-emitted escape sequence and leave a
# bare digits+'m' fragment in the rendered text (stripped-colour still differs from plain in that
# case, which property 1 already guards - this asserts the mechanism directly). Every 'digits+m'
# run in the coloured line must be the tail of a real 'ESC[...m' sequence; -cmatch (case-sensitive)
# because label text on this row legitimately contains 'digit+M' (uppercase, e.g. '[1M]').
Assert-Equal $false ($nestedModelColored -cmatch '(?<!\x1b\[[0-9;]*)\d+m') 'nested-bracket model row: no dangling escape fragment (bare digits+m) survives outside a real ESC[...m sequence'

# --- render driver ------------------------------------------------------------------------
# The driver is testable because both the key source AND the size source are injected. Without
# the second one, "did it notice the resize" could only be answered by resizing a real window.

$sizes = [System.Collections.Queue]::new()
$sizes.Enqueue(@(80, 24)); $sizes.Enqueue(@(120, 40))
$getSize = { if ($sizes.Count -gt 0) { $sizes.Dequeue() } else { @(120, 40) } }
$never = { throw 'ReadKey must not be called when the size changed first' }
$r = Wait-KeyOrResize -ReadKey $never -Width 80 -Height 24 -GetSize $getSize -KeyAvailable { $false } -MaxLoops 3
Assert-Equal 'resize' "$r" 'a size change is reported before any key is read'

$key = [System.ConsoleKeyInfo]::new([char]0, [System.ConsoleKey]::Enter, $false, $false, $false)
$r = Wait-KeyOrResize -ReadKey { $key } -Width 80 -Height 24 -GetSize { @(80, 24) } -KeyAvailable { $true } -MaxLoops 3
Assert-Equal 'Enter' "$($r.Key)" 'a keypress is returned when the size is unchanged'

# Preview mode MUST take the linear path: the alternate buffer is invisible to a captured run, so a
# preview that entered it would render into a screen nobody can read back.
$savedPreview = $env:CLAUDE_AUTO_PREVIEW
try {
    $env:CLAUDE_AUTO_PREVIEW = '1'
    Assert-Equal $false (Test-AltBufferSupported) 'preview mode never enters the alternate buffer'
} finally {
    if ($null -eq $savedPreview) { Remove-Item Env:CLAUDE_AUTO_PREVIEW -ErrorAction SilentlyContinue }
    else { $env:CLAUDE_AUTO_PREVIEW = $savedPreview }
}

# --- Speaker colour in the preview (reported 2026-08-15: you could not tell whose message was
# whose). The words were already there; what was missing was colour. These pin BOTH halves: the
# colour appears, AND the literal attribution survives, because NO_COLOR and dumb terminals are
# real and the words are what has to work when the colour does not.
$uni = Get-Glyphs
$ascii = Get-Glyphs -Ascii
$esc = [char]27

$userLine = "you $($uni.RAngle) what changed here"
$claudeLine = "claude $($uni.RAngle) three files did"
$paintedUser = Add-PickerColor -Line $userLine -Enabled -Glyphs $uni
$paintedClaude = Add-PickerColor -Line $claudeLine -Enabled -Glyphs $uni

# .Contains, never -like: an SGR sequence contains '[', which -like reads as an unterminated
# character class. It throws WildcardPatternException, the error does not increment $script:Failed,
# and the suite prints "all passed" anyway - a check that cannot fail. Caught here 2026-08-15 while
# writing these very assertions.
$cyan = [string][char]27 + '[96m'
$accent = [string][char]27 + '[38;5;209m'
$brightYellow = [string][char]27 + '[93m'
Assert-Equal $true ($paintedUser.Contains($cyan)) 'a user message is tinted BrightCyan'
Assert-Equal $true ($paintedClaude.Contains($accent)) 'an assistant message is tinted with the Claude accent'
Assert-Equal $false ($paintedUser.Contains($accent)) 'the two speakers do not share one colour, or the colour says nothing'
Assert-Equal $userLine ($paintedUser -replace "$esc\[[0-9;]*m", '') 'stripping the colour off a user line returns it unchanged'
Assert-Equal $claudeLine ($paintedClaude -replace "$esc\[[0-9;]*m", '') 'stripping the colour off an assistant line returns it unchanged'
Assert-Equal $userLine (Add-PickerColor -Line $userLine -Glyphs $uni) 'colour disabled leaves the line exactly as it was'

# ASCII mode is where this could go wrong silently: Cursor and RAngle are both '>' there, so the
# cursor rule used to repaint the '>' that belongs to the attribution. The sentinel must keep the
# two rules apart - and the round trip is what proves it, since a mangled escape sequence would
# survive a naive "does it contain a colour" check.
$asciiUser = "you $($ascii.RAngle) ascii mode"
$paintedAscii = Add-PickerColor -Line $asciiUser -Enabled -Glyphs $ascii
Assert-Equal $asciiUser ($paintedAscii -replace "$esc\[[0-9;]*m", '') 'in ASCII mode the attribution survives the cursor rule intact'
Assert-Equal $true ($paintedAscii.Contains($cyan)) 'in ASCII mode the attribution is still tinted'

# A '>' that is a genuine cursor must still be painted - the sentinel must not swallow the rule
# it was introduced to avoid colliding with.
$cursorLine = "$($ascii.Cursor) some project"
Assert-Equal $true ((Add-PickerColor -Line $cursorLine -Enabled -Glyphs $ascii).Contains($brightYellow)) 'the cursor glyph is still painted in ASCII mode'

# The word must not be coloured when it is ordinary prose rather than an attribution.
$prose = 'claude and you both agree'
Assert-Equal $prose (Add-PickerColor -Line $prose -Enabled -Glyphs $uni) 'the words you/claude in message text are left alone'

# --- Mouse in the picker (owner ask 2026-08-15). Driven entirely through the injected seams, so
# none of this needs a console: $Draw hands back a row map exactly as the real one does, $Wait
# yields scripted events, and $GetWindowTop is fixed. What is asserted is the DECISION - which
# session a click selects - not the plumbing, which Test-Input covers. ---
$mouseSessions = 1..5 | ForEach-Object {
    [pscustomobject]@{
        SessionId = "s$_"; Project = "P$_"; Worktree = $null
        Modified = (Get-Date).AddMinutes(-$_); SizeBytes = 100; PromptCount = 2
        Title = "t$_"; LastUser = "u$_"; LastAssistant = "a$_"; RecentMessages = @()
    }
}
# Rows start at screen line 4 and five of them are visible, scrolled to the top.
$mouseMap = [pscustomobject]@{ FirstRowY = 4; RowCount = 5; Start = 0 }
$mapDraw = { param($s, $i, $f) $mouseMap }.GetNewClosure()
$noMapDraw = { param($s, $i, $f) }

function New-MouseEvent {
    param([int]$X = 0, [int]$Y = 0, [switch]$Left, [switch]$Move, [switch]$Double, [int]$Wheel = 0)
    [pscustomobject]@{
        Kind = 'mouse'; X = $X; Y = $Y; Buttons = 0
        Left = [bool]$Left; Right = $false
        IsMove = [bool]$Move; IsDoubleClick = [bool]$Double
        Wheel = $Wheel; WheelUp = ($Wheel -gt 0); WheelDown = ($Wheel -lt 0)
    }
}
function New-EventReader { param([array]$Events) $q = [System.Collections.Queue]::new(); foreach ($e in $Events) { $q.Enqueue($e) }; return { if ($q.Count -eq 0) { throw 'events exhausted' }; $q.Dequeue() }.GetNewClosure() }
$enter = [System.ConsoleKeyInfo]::new([char]0, [System.ConsoleKey]::Enter, $false, $false, $false)

# A click on the third visible row selects the third session - proven by pressing Enter after it
# and seeing WHICH session comes back.
$w = New-EventReader @((New-MouseEvent -Y 6 -Left), $enter)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $mapDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 's3' $picked.Session.SessionId 'a click on the third row selects the third session'

# A single click must NOT open anything on its own: a stray click that launched a session is the
# mistake nobody forgives.
$w = New-EventReader @((New-MouseEvent -Y 5 -Left), (New-MouseEvent -Y 5), $enter)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $mapDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 's2' $picked.Session.SessionId 'a click alone only moves the selection; the release that follows it does nothing'

# A double click opens immediately - and returns without needing the Enter that follows it, which
# is what the un-consumed event proves.
$w = New-EventReader @((New-MouseEvent -Y 4 -Left -Double), $enter)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $mapDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 's1' $picked.Session.SessionId 'a double click opens the row under the pointer'
Assert-Equal $false $picked.Fork 'and opens it rather than forking it'

# Scrolling moves the selection, in both directions.
$w = New-EventReader @((New-MouseEvent -Wheel -128), (New-MouseEvent -Wheel -128), $enter)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $mapDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 's3' $picked.Session.SessionId 'two notches down move the selection down two'
$w = New-EventReader @((New-MouseEvent -Wheel -128), (New-MouseEvent -Wheel 128), $enter)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $mapDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 's1' $picked.Session.SessionId 'down then up comes back'

# Everything that must be IGNORED. Each of these would be a bug that only shows up in a real
# terminal, where the events arrive whether or not the code expects them.
$w = New-EventReader @((New-MouseEvent -Y 99 -Left), $enter)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $mapDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 's1' $picked.Session.SessionId 'a click below the rows changes nothing'
$w = New-EventReader @((New-MouseEvent -Y 2 -Left), $enter)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $mapDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 's1' $picked.Session.SessionId 'a click on the box border changes nothing'
$w = New-EventReader @((New-MouseEvent -Y 6 -Left -Move), $enter)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $mapDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 's1' $picked.Session.SessionId 'dragging across rows does not select - only a press does'

# The window has scrolled: the same session now sits at a higher BUFFER row, and forgetting that
# is how a click lands rows away from the pointer.
$w = New-EventReader @((New-MouseEvent -Y 106 -Left), $enter)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $mapDraw -Wait $w -GetWindowTop { 100 }
Assert-Equal 's3' $picked.Session.SessionId 'the window top is applied, so a scrolled buffer still hits the right row'

# A Draw that returns no map - which is every pre-existing caller and every other test in this file
# - must leave the mouse completely inert rather than throwing.
$w = New-EventReader @((New-MouseEvent -Y 6 -Left), $enter)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $noMapDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 's1' $picked.Session.SessionId 'without a row map a click is inert, not fatal'

# --- Mouse on the launch screen. Same seams, and the assertion that matters most is that a click
# on an option CELL selects that value - the difference between a menu and a picture of one. ---
$launchState = New-LaunchState
$lmap = $null
$null = Get-LaunchFrame -State $launchState -Width 100 -Height 30 -RowMap ([ref]$lmap)
Assert-Equal $true ($lmap.Rows.Count -gt 2) 'the launch frame reports a row map'

$rowsDef = @(Get-LaunchRows)
$modelIdx = [Array]::FindIndex($rowsDef, [Predicate[object]]{ param($r) $r.Name -eq 'Model' })
$modelRowMap = $lmap.Rows[$modelIdx]
$fableCell = @($modelRowMap.Cells | Where-Object { $_.Value -eq 'fable' })[0]
Assert-Equal $true ($null -ne $fableCell) 'the Model row exposes a clickable cell for every option'

$ldraw = { param($s) $lmap }.GetNewClosure()
$esc = [System.ConsoleKeyInfo]::new([char]0, [System.ConsoleKey]::Escape, $false, $false, $false)
$enterKey = [System.ConsoleKeyInfo]::new([char]0, [System.ConsoleKey]::Enter, $false, $false, $false)

# A click on the row selects it, and nothing else changes.
$w = New-EventReader @((New-MouseEvent -Y $modelRowMap.Y -X 1 -Left), $enterKey)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal $modelIdx $out.Row 'a click on a row selects that row'
Assert-Equal 'default' $out.Model 'and, away from the option cells, leaves the value alone'

# A click ON the 'fable' cell selects the row AND the value.
$w = New-EventReader @((New-MouseEvent -Y $modelRowMap.Y -X $fableCell.Start -Left), $enterKey)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 'fable' $out.Model 'a click on an option cell selects that option'
Assert-Equal $modelIdx $out.Row 'and selects its row too'
# The far edge of the span must hit as well - an off-by-one there makes the last option unclickable.
$w = New-EventReader @((New-MouseEvent -Y $modelRowMap.Y -X $fableCell.End -Left), $enterKey)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 'fable' $out.Model 'the last column of an option cell still hits it'
# One column past it must NOT.
$w = New-EventReader @((New-MouseEvent -Y $modelRowMap.Y -X ($fableCell.End + 1) -Left), $enterKey)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal $false ($out.Model -eq 'fable') 'one column past the cell does not select it'

# Nothing a mouse does may START a session: only Enter returns the state.
$w = New-EventReader @((New-MouseEvent -Y $modelRowMap.Y -X $fableCell.Start -Left -Double), $esc)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal '' "$out" 'even a double click does not start a session - Escape still cancels'

# The wheel walks the rows.
$w = New-EventReader @((New-MouseEvent -Wheel -128), (New-MouseEvent -Wheel -128), $enterKey)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 2 $out.Row 'two notches down move two rows down'

# --- Clickable footer hints (owner ask 2026-08-15). A click becomes the KEY the hint advertises and
# takes the ordinary keyboard path, so what is asserted here is that the SAME outcome arrives. ---
function Get-HintSpan { param($Map, [string]$Key, [string]$Char)
    @($Map.Footer | Where-Object { ($Key -and $_.Key -eq $Key) -or ($Char -and $_.Char -eq $Char) })[0]
}

Assert-Equal $true ($lmap.Footer.Count -ge 3) 'the launch footer reports its clickable hints'
Assert-Equal $true ($null -ne $lmap.FooterY) 'and where the footer line is'

$hEnter = Get-HintSpan -Map $lmap -Key 'Enter'
$hEsc = Get-HintSpan -Map $lmap -Key 'Escape'
$hU = Get-HintSpan -Map $lmap -Char 'u'

$w = New-EventReader @((New-MouseEvent -Y $lmap.FooterY -X $hEnter.Start -Left))
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal $true ($null -ne $out) 'clicking "enter start" starts, exactly as pressing Enter does'
# The far end of the span is the LABEL, not the key word - clicking "start" must work too, or the
# hint is a puzzle about which four characters are live.
$w = New-EventReader @((New-MouseEvent -Y $lmap.FooterY -X $hEnter.End -Left))
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal $true ($null -ne $out) 'clicking the LABEL half of a hint works too'

$w = New-EventReader @((New-MouseEvent -Y $lmap.FooterY -X $hEsc.Start -Left))
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal '' "$out" 'clicking "esc quit" quits'

# 'u maintenance' must reach $OnKey - the hook the launcher uses to open the maintenance screen -
# rather than being special-cased anywhere in the mouse code.
$script:sawU = $false
$w = New-EventReader @((New-MouseEvent -Y $lmap.FooterY -X $hU.Start -Left), $esc)
$null = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 } -OnKey {
    param($k) if ("$($k.KeyChar)" -eq 'u') { $script:sawU = $true; return $true }; return $false
}
Assert-Equal $true $script:sawU 'clicking "u maintenance" arrives at OnKey as the letter u'

# A click on the separator between hints, and on a hint that is not clickable, must do nothing.
$w = New-EventReader @((New-MouseEvent -Y $lmap.FooterY -X ($hEnter.Start - 2) -Left), $esc)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal '' "$out" 'a click on the separator does nothing and the screen stays up'
$w = New-EventReader @((New-MouseEvent -Y $lmap.FooterY -X 3 -Left), $esc)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal '' "$out" 'the arrow hint is not clickable - it names two directions and a click cannot mean one'

# --- switching tabs carries the rows with it (owner ask 2026-09-04) ---------------------------
# The five habit rows are per account. Stepping the account row parks the current answers under the
# account being LEFT and loads the arriving one's - otherwise switching tabs merely to read another
# account's five-hour percentage would silently discard the choices already made on this one, and
# the launch would spend the wrong limit with the wrong model.
$effortIdx = Get-RowIndex -Name 'Effort'
$toEffort  = @('DownArrow') * $effortIdx
$backUp    = @('UpArrow') * $effortIdx
$effortMax = @('RightArrow') * ([Array]::IndexOf(((Get-LaunchRows) | Where-Object { $_.Name -eq 'Effort' }).Values, 'max'))

$keys = @('RightArrow') + $toEffort + $effortMax + @('Enter')
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys $keys) -Draw {}
Assert-Equal 'personal' $out.Account 'right on the account row moves to the next tab'
Assert-Equal 'max' $out.Effort 'and the rows edited afterwards belong to that tab'

$keys = @('RightArrow') + $toEffort + $effortMax + $backUp + @('LeftArrow', 'Enter')
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys $keys) -Draw {}
Assert-Equal 'work' $out.Account 'left comes back to the previous tab'
Assert-Equal 'default' $out.Effort 'and the tab it returns to still has ITS own answers, not the other tab''s'

$keys = @('RightArrow') + $toEffort + $effortMax + $backUp + @('LeftArrow', 'RightArrow', 'Enter')
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys $keys) -Draw {}
Assert-Equal 'personal' $out.Account 'and going back again returns to the edited tab'
Assert-Equal 'max' $out.Effort 'with the edit still on it - the stash, not the file'

# ctrl+r resets the ACTIVE tab only. A stray reset must not erase another account's habit; that is
# the whole reason it stopped being New-LaunchState.
$keys = @('RightArrow') + $toEffort + $effortMax + @($CtrlR, 'Enter')
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-MixedKeyReader -Keys $keys) -Draw {}
Assert-Equal 'personal' $out.Account 'ctrl+r does not change the account'
Assert-Equal 'default' $out.Effort 'ctrl+r resets the active tab''s rows'
Assert-Equal $effortIdx $out.Row 'and leaves the cursor row where it was'
$keys = @('RightArrow') + $toEffort + $effortMax + @($CtrlR) + $backUp + @('LeftArrow', 'RightArrow', 'Enter')
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-MixedKeyReader -Keys $keys) -Draw {}
Assert-Equal 'default' $out.Effort 'and the reset survives a trip to another tab and back - it is not the stale stash that returns'
# The other tab's stash is untouched by the reset.
$keys = $toEffort + $effortMax + $backUp + @('RightArrow', $CtrlR, 'LeftArrow', 'Enter')
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-MixedKeyReader -Keys $keys) -Draw {}
Assert-Equal 'work' $out.Account 'back on the first tab'
Assert-Equal 'max' $out.Effort 'a reset on another tab left this one alone'

# Action is deliberately outside the reset: since Task 9 it is not even a launch-screen row any
# more (the project screen decides it) - Reset-LaunchTab never touches it regardless of how it got
# set. Proven directly on the state, since there is no row left to navigate to and edit it through.
$actionState = New-LaunchState; $actionState.Action = 'continue'
$out = Invoke-LaunchScreen -State $actionState -ReadKey (New-MixedKeyReader -Keys @($CtrlR, 'Enter')) -Draw {}
Assert-Equal 'continue' $out.Action 'ctrl+r leaves the action alone - it describes this launch, not a habit, and is not a row Reset-LaunchTab touches'

# A tab with no stash falls back to the FILE, and the rows it restores are marked so a stale choice
# is visible rather than silent.
$tabPrefs = @{ Version = 2; Account = 'work'
               Profiles = @{ low = @{ Model = 'haiku'; Effort = 'high'; SavedAtMs = 1000 } } }
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('RightArrow', 'RightArrow', 'Enter')) -Draw {} -Prefs $tabPrefs
Assert-Equal 'low' $out.Account 'two rights reach the third tab'
Assert-Equal 'haiku' $out.Model 'a tab with no stash loads its remembered profile from the file'
Assert-Equal 'high' $out.Effort 'every remembered row of it'
Assert-Equal $true ('Model' -in @($out.Restored)) 'and they are marked, so a stale choice is visible'

# A click on a tab is the same intent as arrowing to it, and must walk the same stepper - a direct
# assignment would skip the stash and the two paths would drift apart silently.
$cmap = $null
$null = Get-LaunchFrame -State (New-LaunchState) -Width 100 -Height 30 -RowMap ([ref]$cmap)
$cdraw = { param($s) $cmap }.GetNewClosure()
$accountRowMap = @($cmap.Rows | Where-Object { $_.Name -eq 'Account' })[0]
$lowCell = @($accountRowMap.Cells | Where-Object { $_.Value -eq 'low' })[0]
$w = New-EventReader @((New-MouseEvent -Y $accountRowMap.Y -X $lowCell.Start -Left), $enterKey)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $cdraw -Wait $w -GetWindowTop { 0 } -Prefs $tabPrefs
Assert-Equal 'low' $out.Account 'a click on a tab selects that account'
Assert-Equal 'haiku' $out.Model 'and brings its remembered profile with it, exactly as arrowing there does'

# Picker footer: every action reachable by click, and each producing the SAME result as its key.
$pmap = $null
$null = Get-PickerFrame -Sessions $mouseSessions -Index 0 -Width 100 -Height 24 -RowMap ([ref]$pmap)
$pDraw = { param($s, $i, $f) $pmap }.GetNewClosure()
Assert-Equal $true ($pmap.Footer.Count -ge 4) 'the picker footer reports its clickable hints'

$w = New-EventReader @((New-MouseEvent -Y $pmap.FooterY -X (Get-HintSpan -Map $pmap -Key 'Enter').Start -Left))
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $pDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 's1' $picked.Session.SessionId 'clicking "enter open" opens the selected session'
Assert-Equal $false $picked.Fork 'and does not fork it'

$w = New-EventReader @((New-MouseEvent -Y $pmap.FooterY -X (Get-HintSpan -Map $pmap -Char 'f').Start -Left))
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $pDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal $true $picked.Fork 'clicking "f fork" forks'

$w = New-EventReader @((New-MouseEvent -Y $pmap.FooterY -X (Get-HintSpan -Map $pmap -Key 'Escape').Start -Left))
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $pDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal '' "$picked" 'clicking "esc back" leaves the picker'

# '/' enters filter mode, which is only observable through what happens NEXT: typed letters become
# a filter instead of commands.
$slash = Get-HintSpan -Map $pmap -Char '/'
$typed = [System.ConsoleKeyInfo]::new([char]'f', [System.ConsoleKey]0, $false, $false, $false)
# TWO escapes on purpose: inside filter mode Esc clears the filter and leaves typing, it does not
# leave the picker - so a single one would exhaust the queue and look like a hang. Had the click on
# '/' done nothing, the f would have FORKED and returned immediately, which is what this
# distinguishes.
$w = New-EventReader @((New-MouseEvent -Y $pmap.FooterY -X $slash.Start -Left), $typed, $esc, $esc)
$picked = Invoke-SessionPicker -Sessions $mouseSessions -ReadKey $w -Draw $pDraw -Wait $w -GetWindowTop { 0 }
Assert-Equal '' "$picked" 'clicking "/ filter" starts typing, so the f that follows filters instead of forking'

# Colour: applied by column span, and reversible. A footer that cannot be stripped back to its
# plain text would break every width calculation downstream.
$plainFooter = (New-HintFooter -Glyphs (Get-Glyphs) -Hints @(
    @{ Token = 'enter'; Label = 'start'; Clickable = $true; Key = 'Enter'; Char = '' }
    @{ Token = 'esc';   Label = 'quit';  Clickable = $true; Key = 'Escape'; Char = '' }))
$paintedFooter = Add-HintColor -Line $plainFooter.Text -Spans $plainFooter.Spans -Enabled
Assert-Equal $plainFooter.Text ($paintedFooter -replace "$([char]27)\[[0-9;]*m", '') 'stripping the colour off the footer returns it unchanged'
# The literal escape, not $script:C.Reverse: .Contains($script:C.Reverse) would pass vacuously if
# Reverse were ever present-and-EMPTY ('' is contained in anything) - only a missing key (Contains
# $null -> False) would have caught that trap, not an empty one.
Assert-Equal $true ($paintedFooter.Contains("$([char]27)[7m")) 'the key caps are painted reverse video'
Assert-Equal $plainFooter.Text (Add-HintColor -Line $plainFooter.Text -Spans $plainFooter.Spans) 'colour disabled leaves the footer exactly as it was'

# --- footer hotkeys render as BUTTONS (Task 4). A clickable token gets a cell of its own -
# padded with a space either side when colour will paint it, bracketed when it will not - so the
# structure survives NO_COLOR. Assert-Equal here is (Expected, Actual) - the house order; the
# brief's own sample had it backwards. ---
$g4 = Get-Glyphs
$f4 = New-HintFooter -Glyphs $g4 -Hints @(
    @{ Token = 'up/down'; Label = 'row';   Clickable = $false }
    @{ Token = 'enter';   Label = 'start'; Clickable = $true; Key = 'Enter'; Char = '' }
)
Assert-True ($f4.Text -match ' enter  start') 'a clickable token is padded into a key cap'
$click4 = @($f4.Spans | Where-Object { $_.Key -eq 'Enter' })[0]
Assert-Equal ' enter ' $f4.Text.Substring($click4.KeyStart, $click4.KeyEnd - $click4.KeyStart + 1) 'the cap span includes the padding'
Assert-True ($f4.Text.Substring($click4.Start, $click4.End - $click4.Start + 1) -match 'start') 'the click span still covers the label'

$plain4 = New-HintFooter -Glyphs $g4 -Plain -Hints @(
    @{ Token = 'enter'; Label = 'start'; Clickable = $true; Key = 'Enter'; Char = '' }
)
Assert-True ($plain4.Text -match '\[enter\] start') 'without colour the cap is bracketed'

$painted4 = Add-HintColor -Line $f4.Text -Spans $f4.Spans -Enabled
# Not [regex]::Escape($script:C.Reverse) -match ... : Escape($null) silently returns '', and an
# empty pattern matches ANY string - a missing Reverse code would pass this check by accident.
Assert-True ((-not [string]::IsNullOrEmpty($script:C.Reverse)) -and $painted4.Contains($script:C.Reverse)) 'the cap is painted reverse video'
Assert-Equal $f4.Text (Remove-AnsiColor $painted4) 'painting stays reversible'
# Task 6 correction 4: nothing above pins WHERE the reverse-video escape lands - moving the tint
# (Add-HintColor, Screens.ps1) off the key cap onto the preceding gap text survives every assertion
# above (both still find the escape and both still strip back to plain text). Pin it directly: the
# Reverse+Bold escape must be followed immediately by the cap text itself (' enter '), not the gap.
Assert-True ($painted4.Contains($script:C.Reverse + $script:C.Bold + ' enter ' + $script:C.Reset)) 'the reverse-video escape paints the key cap itself, not the gap before it'

# The width fact the padded and bracketed forms share (1 + token + 1, either way): they cannot
# wrap to a different number of lines, so $script:MinHeight cannot diverge between colour and
# no-colour. If this ever fails, something is wrong with the -Plain implementation.
$widthHints = @(
    @{ Token = 'up/down'; Label = 'row';   Clickable = $false }
    @{ Token = 'enter';   Label = 'start'; Clickable = $true; Key = 'Enter'; Char = '' }
    @{ Token = 'u';       Label = 'maintenance'; Clickable = $true; Key = ''; Char = 'u' }
    @{ Token = 'esc';     Label = 'quit';  Clickable = $true; Key = 'Escape'; Char = '' }
)
$paddedWrap = New-HintFooter -Glyphs $g4 -Width 30 -Hints $widthHints
$bracketWrap = New-HintFooter -Glyphs $g4 -Width 30 -Plain -Hints $widthHints
Assert-Equal @($paddedWrap.Lines).Count @($bracketWrap.Lines).Count 'the padded and bracketed forms wrap to the same number of lines'

# --- Maintenance screen. It has no rows to select, so the mouse does exactly one thing there. The
# assertion works by EVENT BUDGET: the reader holds a single event, so a click that is honoured
# leaves on the first pass, and one that is ignored asks for a second event and throws. That is what
# makes both directions falsifiable without inspecting internal state. ---
# Any EXISTING file: the fake -Runner never executes it, but the existence check runs regardless of the runner.
$cfgActions = @([pscustomobject]@{ Key = 'i'; Label = 'full reindex'; Script = (Join-Path $PSScriptRoot 'fixtures\config-four.json'); ConfirmTwice = $true })
$fakeInfo = [pscustomobject]@{ Matches = $true; NewestVersion = '2.1.233'; InstalledHash = 'A'; NewestHash = 'A'; VersionCount = 3; BinPath = 'x' }
$mmap = $null
$null = Get-MaintenanceFrame -Info $fakeInfo -Width 100 -Height 24 -RowMap ([ref]$mmap) -Actions $cfgActions
$mDraw = { param($i, $s) $mmap }.GetNewClosure()
# Asserting the SET rather than a count: a count says nothing about which hint went missing, and
# adding one should not make an unrelated assertion fail with a number.
$mChars = @($mmap.Footer | ForEach-Object { if ($_.Char) { $_.Char } else { $_.Key } }) -join ','
Assert-Equal 'u,r,d,m,p,i,Escape' $mChars 'every maintenance action is clickable, reindex included'

$escSpan = Get-HintSpan -Map $mmap -Key 'Escape'
# +$escSpan.Line: the padded key caps push this footer to wrap at width 100, so 'esc' now sits on
# the SECOND footer line - the same offset every other wrapped-hint click in this file already uses.
$w = New-EventReader @((New-MouseEvent -Y ($mmap.FooterY + $escSpan.Line) -X $escSpan.Start -Left))
$threw = $false
try { Invoke-MaintenanceScreen -ReadKey $w -Draw $mDraw -Wait $w -GetWindowTop { 0 } } catch { $threw = $true }
Assert-Equal $false $threw 'clicking "esc back" leaves the maintenance screen on the first event'

# A click one column past a span must NOT leave - proving the hit test is what decided it, not the
# mere arrival of a mouse event. 'esc' no longer works for this: it is now ALONE on the wrapped
# second footer line, so one column past its end is merely past end-of-line, not a boundary against
# an ADJACENT hint. 'd doctor' sits between 'r rename swap' and 'm mcp list' on the FIRST footer
# line, so one column past its end lands in the separator before 'm' - the adjacency this was
# written to prove.
$dSpan = Get-HintSpan -Map $mmap -Char 'd'
Assert-Equal 0 $dSpan.Line 'doctor sits on the first, unwrapped footer line'
$w = New-EventReader @((New-MouseEvent -Y ($mmap.FooterY + $dSpan.Line) -X ($dSpan.End + 1) -Left))
$threw = $false
try { Invoke-MaintenanceScreen -ReadKey $w -Draw $mDraw -Wait $w -GetWindowTop { 0 } } catch { $threw = $true }
Assert-Equal $true $threw 'a click one column past the hint does not leave — the span decided, not the event'

# --- CBM full reindex from the maintenance screen (owner ask 2026-08-15). It is confirmed first
# because it is SLOW, not because it is destructive, and the assertions pin exactly that: one press
# only warns, the second runs. The runner is injected, so nothing here reindexes anything. ---
$script:reindexRuns = 0
$fakeRunner = { param($p) $script:reindexRuns++; return [pscustomobject]@{ Output = @('fleet: 48/48 ok', 'artifacts refreshed'); ExitCode = 0 } }
$iKey = [System.ConsoleKeyInfo]::new([char]'i', [System.ConsoleKey]0, $false, $false, $false)

$script:lastStatus = ''
$script:drawMap = $mmap
# NOT .GetNewClosure() here, unlike the read-only draws above: GetNewClosure binds the scriptblock
# to a NEW module scope, so a `$script:` assignment inside it lands somewhere this file cannot read
# and the captured status silently stays empty. Reading a captured variable is fine; writing one out
# is not.
$statusDraw = { param($i, $s) $script:lastStatus = $s; $script:drawMap }

$w = New-EventReader @($iKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -Actions $cfgActions -Runner $fakeRunner -GetWindowTop { 0 }
Assert-Equal 0 $script:reindexRuns 'one press of i does NOT start a five-minute reindex'
Assert-Equal $true ($script:lastStatus -match '^confirm: press i') 'it asks for confirmation and says how long it takes'

$script:reindexRuns = 0
$w = New-EventReader @($iKey, $iKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -Actions $cfgActions -Runner $fakeRunner -GetWindowTop { 0 }
Assert-Equal 1 $script:reindexRuns 'the second press runs it, exactly once'
Assert-Equal $true ($script:lastStatus -match 'green') 'and reports the verdict'

# A click on "i full reindex" must behave identically to the key - it becomes that key.
$script:reindexRuns = 0
$iSpan = Get-HintSpan -Map $mmap -Char 'i'
$w = New-EventReader @((New-MouseEvent -Y $mmap.FooterY -X $iSpan.Start -Left), (New-MouseEvent -Y $mmap.FooterY -X $iSpan.Start -Left), $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -Actions $cfgActions -Runner $fakeRunner -GetWindowTop { 0 }
Assert-Equal 1 $script:reindexRuns 'clicking the hint twice runs it once, exactly as pressing i twice does'

# Exit 2 is "could not run" across this whole toolchain and must never read as success.
$w = New-EventReader @($iKey, $iKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner { param($p) [pscustomobject]@{ Output = @('nothing'); ExitCode = 2 } }
Assert-Equal $true ($script:lastStatus -match 'COULD NOT RUN') 'exit 2 is reported as could-not-run, never as a pass'
$w = New-EventReader @($iKey, $iKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner { param($p) [pscustomobject]@{ Output = @('boom'); ExitCode = 1 } }
Assert-Equal $true ($script:lastStatus -match 'FAILED') 'a non-zero exit is reported as a failure'

# A missing script must say so rather than silently doing nothing.
$missing = Invoke-MaintenanceScript -ScriptPath 'C:\nope\does-not-exist.ps1' -Label 'full reindex'
Assert-Equal $false $missing.Ran 'a missing reindex script is reported, not silently skipped'
Assert-Equal $true ($missing.Message -match 'not found') 'and the message names the problem'
$missingInjected = Invoke-MaintenanceScript -ScriptPath 'C:\nope\x.ps1' -Label 'q' -Runner { throw 'must not run' }
Assert-Equal $false $missingInjected.Ran 'the existence check runs before any runner, injected or not'
Assert-Equal $true ($missingInjected.Message -match 'not found') 'and says not found without calling the runner'

# --- A hover must never press a menu key (caught live 2026-08-25) ------------------------------
# `claude mcp list` fired over and over in a Rider terminal tab while the owner merely HOVERED over
# the window - initiator proven by process capture: claude.exe mcp list as a direct child of
# claude-auto.ps1. PowerShell's -eq is case-INSENSITIVE, so the uppercase letter that terminates an
# SGR mouse report (ESC [ < b ; x ; y M) presses the same footer key as the lowercase hint: M ran
# 'mcp list', and U / R / D / P / I sit one hover away from the update, the rename swap and a
# five-minute fleet reindex. Only the reindex branch has an injectable runner, so it is the one
# asserted here - the defect is the comparison, and every branch shares it.
$script:reindexRuns = 0
$IKeyUpper = [System.ConsoleKeyInfo]::new([char]'I', [System.ConsoleKey]0, $false, $false, $false)
$w = New-EventReader @($IKeyUpper, $IKeyUpper, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -Actions $cfgActions -Runner $fakeRunner -GetWindowTop { 0 }
Assert-Equal 0 $script:reindexRuns 'an uppercase I never runs the fleet reindex - maintenance key matching is case-sensitive'

# The double-click record carries the button down with MOUSE_MOVED clear, so it walked straight
# through the press guard and counted as a SECOND press - which is enough to get past a confirm.
$script:reindexRuns = 0
$iSpanDbl = Get-HintSpan -Map $mmap -Char 'i'
$w = New-EventReader @((New-MouseEvent -Y $mmap.FooterY -X $iSpanDbl.Start -Left),
                       (New-MouseEvent -Y $mmap.FooterY -X $iSpanDbl.Start -Left -Double), $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -Actions $cfgActions -Runner $fakeRunner -GetWindowTop { 0 }
Assert-Equal 0 $script:reindexRuns 'a double-click is not a second press - it cannot confirm what one click only warned about'

# Whatever the terminal queued WHILE a child command owned the screen is noise behind a process
# that took ten seconds, not an instruction to this menu. Replaying it is what turned one hover
# into a run every few seconds, so the screen drains the queue after every shell-out.
$script:drains = 0
$script:reindexRuns = 0
$w = New-EventReader @($iKey, $iKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -Actions $cfgActions -Runner $fakeRunner -GetWindowTop { 0 } -Drain { $script:drains++ }
Assert-Equal 1 $script:reindexRuns 'the drain does not change what the second press does'
Assert-Equal 1 $script:drains 'the input queue is drained after a command that owned the screen'

# The picker shares the defect: an uppercase F forks a session, which starts one.
$pickSessions = @([pscustomobject]@{ Id = 'aaa'; Path = 'C:\x'; Modified = (Get-Date); PromptCount = 3; Summary = 'one' })
$FKeyUpper = [System.ConsoleKeyInfo]::new([char]'F', [System.ConsoleKey]0, $false, $false, $false)
$w = New-EventReader @($FKeyUpper, $esc)
$picked = Invoke-SessionPicker -Sessions $pickSessions -ReadKey $w -Draw { param($s, $i, $f) $null } -Wait $w -GetWindowTop { 0 }
Assert-True ($null -eq $picked) 'an uppercase F does not fork a session - picker key matching is case-sensitive'

# --- Hotkeys on any keyboard layout (owner ask 2026-09-02: RDP from a phone, Russian and Ukrainian
# layouts). A real console reports the VIRTUAL key of the physical key whatever the layout paints
# on it; VK_PACKET input (a soft keyboard) carries only the Cyrillic character. Both must press the
# key, and the hover guards (uppercase, modifiers) must survive the new paths. ---
function New-VkKey { param([char]$Char, [System.ConsoleKey]$Vk, [switch]$Shift, [switch]$Ctrl)
    [System.ConsoleKeyInfo]::new($Char, $Vk, [bool]$Shift, $false, [bool]$Ctrl) }
$ghe = [char]0x0433   # the letter on the U key of the Cyrillic layouts
$ruU = New-VkKey -Char $ghe -Vk ([System.ConsoleKey]::U)
$packetU = New-VkKey -Char $ghe -Vk ([System.ConsoleKey]::Packet)
$shiftU = New-VkKey -Char ([char]0x0413) -Vk ([System.ConsoleKey]::U) -Shift
$sgrM = New-VkKey -Char 'M' -Vk ([System.ConsoleKey]::M) -Shift
$bareVkM = New-VkKey -Char 'M' -Vk ([System.ConsoleKey]::M)
$ctrlU = New-VkKey -Char ([char]21) -Vk ([System.ConsoleKey]::U) -Ctrl
Assert-Equal $true  (Test-ClaudeHotkey -Key $ruU -Char 'u') 'the U key on the Russian layout (vk=U, char ghe) is the u hotkey'
Assert-Equal $true  (Test-ClaudeHotkey -Key $packetU -Char 'u') 'a VK_PACKET ghe (soft keyboard, no virtual key) is the u hotkey'
Assert-Equal $false (Test-ClaudeHotkey -Key $shiftU -Char 'u') 'Shift on that key is not the hotkey'
Assert-Equal $false (Test-ClaudeHotkey -Key $sgrM -Char 'm') 'the M ending an SGR mouse report (vk=M with Shift) never presses m'
Assert-Equal $false (Test-ClaudeHotkey -Key $bareVkM -Char 'm') 'an uppercase M even without the Shift flag never presses m'
Assert-Equal $false (Test-ClaudeHotkey -Key $ctrlU -Char 'u') 'Ctrl+U is not the hotkey'
Assert-Equal $false (Test-ClaudeHotkey -Key $ruU -Char 'm') 'the ghe key is not some other hotkey'
Assert-Equal $true  (Test-ClaudeHotkey -Key (New-VkKey -Char ([char]0x002E) -Vk ([System.ConsoleKey]::Oem2)) -Char '/') 'the slash key on a Cyrillic layout (Oem2, char .) is the filter hotkey'
Assert-Equal $false (Test-ClaudeHotkey -Key 'resize' -Char 'u') 'a resize is never a hotkey'
Assert-Equal $false (Test-ClaudeHotkey -Key (New-MouseEvent -Y 0 -X 0 -Left) -Char 'u') 'a mouse event is never a hotkey'

# deferred review finding: maintenance action keys may now be [a-z0-9] (Config.ps1), and digits get
# their own virtual-key branch (ConsoleKey.D0..D9) - no Cyrillic table entry is needed for them,
# because both Cyrillic layouts type the same 0-9 characters unshifted, same as Latin.
Assert-Equal $true  (Test-ClaudeHotkey -Key (New-VkKey -Char ([char]0) -Vk ([System.ConsoleKey]::D5)) -Char '5') 'the virtual D5 key with no character (a soft keyboard) is the 5 hotkey'
Assert-Equal $false (Test-ClaudeHotkey -Key (New-VkKey -Char ([char]0) -Vk ([System.ConsoleKey]::D5) -Shift) -Char '5') 'Shift on the D5 key is not the hotkey'
Assert-Equal $false (Test-ClaudeHotkey -Key (New-VkKey -Char ([char]0) -Vk ([System.ConsoleKey]::D6)) -Char '5') 'the D6 key is not the 5 hotkey'
Assert-Equal $true  (Test-ClaudeHotkey -Key (New-VkKey -Char '5' -Vk ([System.ConsoleKey]::D5)) -Char '5') 'typing the digit directly still presses the 5 hotkey'

# Through the screens, not only the matcher: the maintenance reindex confirm and the picker fork.
$script:reindexRuns = 0
$ruI = New-VkKey -Char ([char]0x0448) -Vk ([System.ConsoleKey]::I)
$w = New-EventReader @($ruI, $ruI, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -Actions $cfgActions -Runner $fakeRunner -GetWindowTop { 0 }
Assert-Equal 1 $script:reindexRuns 'sha on the I key (Russian layout) confirms and runs the reindex like i does'
$script:reindexRuns = 0
$w = New-EventReader @($sgrM, $sgrM, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -Actions $cfgActions -Runner $fakeRunner -GetWindowTop { 0 }
Assert-Equal 0 $script:reindexRuns 'two SGR terminators still run nothing'
$ruF = New-VkKey -Char ([char]0x0430) -Vk ([System.ConsoleKey]::F)
$w = New-EventReader @($ruF)
$picked = Invoke-SessionPicker -Sessions $pickSessions -ReadKey $w -Draw { param($s, $i, $f) $null } -Wait $w -GetWindowTop { 0 }
Assert-Equal $true $picked.Fork 'the letter a on the F key (Russian layout) forks like f does'

# --- 50 columns (owner ask 2026-09-02). Content, not clamps: every hint must be READABLE on the
# frame and reachable by click; the frame must fit the terminal with the headroom row. ---
$narrowSessions = @(1..6 | ForEach-Object { [pscustomobject]@{ SessionId = "n$_"; Project = "project-$_"; Worktree = ''; Title = "question $_"; LastUser = ('word ' * 30); LastAssistant = ('reply ' * 30); Modified = (Get-Date).AddMinutes(-$_); PromptCount = $_; SizeBytes = 2048 } })
# A fixed-length synthetic BinPath, not (Join-Path $HOME '.local\bin\claude.exe'): this fixture
# renders at the minimum supported width, so a real $HOME would make the margin depend on how long
# this machine's user name happens to be - unrelated to the code under test.
$narrowInfo = [pscustomobject]@{ Matches = $false; NewestVersion = '2.1.240'; InstalledHash = ('a' * 64); NewestHash = ('b' * 64); VersionCount = 3; VersionsBytes = 900000000; BinPath = 'C:\Users\sample-user\.local\bin\claude.exe' }
# With the model bucket: three bars is what the minimum height is measured against, so the block
# that asserts the minimum has to render the case that produced it.
$narrowLimits = @{ work = [pscustomobject]@{ FiveHour = 41; SevenDay = 63; AgeText = '12 min ago'; Model = 15; ModelLabel = 'FABLE' } }
$longStatus = (1..40 | ForEach-Object { "status line $_ with some words in it" }) -join "`n"

# MinHeight is MEASURED, not chosen: the worst 50-column LAUNCH frame - the one screen with no
# scroll, every row fixed content rather than a list - is rendered at a height the guard cannot
# refuse (200), its lines are counted, and the constant must be that count plus the headroom row
# Write-Frame needs. Written this way the number re-measures itself on every run - a literal would
# go stale the first time a row or a bar is added, which is exactly how the old 16 survived being
# one short. Measured 2026-09-04: 20 lines (3 box + blank + 8 rows + 3 bars + blank + separator +
# restored + 2 wrapped footer lines) -> 21. RE-MEASURED 2026-09-15 (Task 9) when the Action row left
# this screen: 19 lines (one fewer row) -> 20.
#
# The picker and project screens do NOT feed this measurement, and never have since (fix round 1,
# Task 10 review, IMPORTANT 1): a version of this block briefly rendered them at Height 200 too and
# asserted their EXACT count, which does not measure the screen - it measures the FIXTURE. Both
# scroll, so their line count at an unbounded height grows LINEARLY with however many rows the
# fixture happens to have (measured: project 12/13/40 rows -> 19/20/47 lines; picker 6/15/30 rows ->
# 16/25/40 lines) - an exact-count assertion there pins MinHeight to an arbitrary fixture size, and
# bumping the fixture by one row would make the gate demand a taller minimum for no real reason, when
# the screen itself is unchanged. What actually matters is that both screens CLAMP their viewport to
# whatever height they are given, so neither can ever push this constant higher than the launch
# screen's own worst case - proven below with a fixture (40 rows) far past what any real viewport
# shows: it still FITS at MinHeight, and unbound (Height 200) it renders MORE lines than at
# MinHeight, which is the only way to tell a screen that actually clamped from one that merely had a
# short list. They scroll where the launch screen cannot, so re-measure LAUNCH, never these two,
# before ever moving this constant.
$worstCase = @(Get-LaunchFrame -State (New-LaunchState) -Width 50 -Height 200 -Limits $narrowLimits `
    -Restored @('Model') -RestoredAge '12 min' -DefaultModelLabel 'default (Fable 5.1[1M])' -DefaultAdvisorLabel 'default (fable)')
Assert-Equal 19 $worstCase.Count 'the worst 50-column launch frame is 19 lines'
Assert-Equal ($worstCase.Count + 1) $script:MinHeight 'MinHeight is that count plus the headroom row'

# Picker: a 40-session fixture, far past what any viewport shows, must still FIT at MinHeight, and
# must render MORE lines when given the room - proving the viewport actually clamped, not merely
# that the list happened to be short (which is exactly what a naive exact-count assertion could not
# tell apart, per the comment above). The fit assertion alone is stub-blind (fix round 2, coordinator
# review, finding 2): widening the too-small gate (`-lt $script:MinHeight` -> `-lt ($script:MinHeight
# + 3)`) makes the frame at MinHeight return the ~4-line "need 50x20" stub instead of a real render,
# and 4 <= 19 / 4 < (unbounded count) both still pass - a mutant the fit+clamp pair alone cannot see.
# The title line ("resume - N sessions") only appears on a REAL render, never on the stub, so it is
# asserted alongside every fit check below.
$pickerBigSessions = @(1..40 | ForEach-Object { [pscustomobject]@{ SessionId = "big$_"; Project = "project-$_"; Worktree = ''; Title = "question $_"; LastUser = 'x'; LastAssistant = 'y'; Modified = (Get-Date).AddMinutes(-$_); PromptCount = $_; SizeBytes = 2048 } })
$pickerFitPlain = @(Get-PickerFrame -Sessions $pickerBigSessions -Index 0 -Width 50 -Height $script:MinHeight)
Assert-True ($pickerFitPlain.Count -le ($script:MinHeight - 1)) 'a 40-session plain picker still fits at MinHeight - the viewport clamps, not the list length'
Assert-True (($pickerFitPlain -join "`n") -match 'sessions') 'and it is a REAL render at MinHeight, not the too-small stub (a widened size gate cannot fake this)'
$pickerUnboundedPlain = @(Get-PickerFrame -Sessions $pickerBigSessions -Index 0 -Width 50 -Height 200)
Assert-True ($pickerUnboundedPlain.Count -gt $pickerFitPlain.Count) 'and renders more lines when given the room - proving MinHeight is a real clamp on this screen, not an accident of a short fixture'

$pickerFitScoped = @(Get-PickerFrame -Sessions $pickerBigSessions -Index 0 -Scope 'project' -ProjectName $longProjectName34 -Width 50 -Height $script:MinHeight)
Assert-True ($pickerFitScoped.Count -le ($script:MinHeight - 1)) 'a 40-session SCOPED picker still fits at MinHeight too'
Assert-True (($pickerFitScoped -join "`n") -match 'sessions') 'and the scoped fit is a REAL render too, not the stub'
$pickerUnboundedScoped = @(Get-PickerFrame -Sessions $pickerBigSessions -Index 0 -Scope 'project' -ProjectName $longProjectName34 -Width 50 -Height 200)
Assert-True ($pickerUnboundedScoped.Count -gt $pickerFitScoped.Count) 'and the scoped picker clamps the same way'

# The project screen's own worst case at 50 columns: a full registry (more projects than the
# minimum viewport shows) plus the two pinned rows - kept at 12 rows because the hint-readability
# checks further below only need a fixture bigger than one screenful, not proof of clamping.
$projWorstList = 1..12 | ForEach-Object {
    [pscustomobject]@{
        Slug = "wc$_"; Path = "C:\Users\sample-user\Projects\project-name-$_"
        Name = "project-name-$_"; Worktree = $(if ($_ % 3 -eq 0) { 'feature-x' } else { $null })
        LastActivity = (Get-Date).AddHours(-$_)
    }
}

# Project: the same clamp proof as the picker above, with its own 40-row registry - it scrolls where
# the launch screen cannot, so it never pushes this constant higher than the launch screen's own
# worst case (it scrolls; it never pushes this constant higher).
$projBigList = 1..40 | ForEach-Object {
    [pscustomobject]@{
        Slug = "big$_"; Path = "C:\Users\sample-user\Projects\project-name-$_"
        Name = "project-name-$_"; Worktree = $(if ($_ % 3 -eq 0) { 'feature-x' } else { $null })
        LastActivity = (Get-Date).AddHours(-$_)
    }
}
$projFit = @(Get-ProjectFrame -Projects $projBigList -Index 5 -Cwd 'C:\x' -Width 50 -Height $script:MinHeight)
Assert-True ($projFit.Count -le ($script:MinHeight - 1)) 'a 40-project registry still fits at MinHeight - the viewport clamps, not the registry size'
Assert-True (($projFit -join "`n") -match 'known') 'and it is a REAL render at MinHeight, not the too-small stub (a widened size gate cannot fake this)'
$projUnbounded = @(Get-ProjectFrame -Projects $projBigList -Index 5 -Cwd 'C:\x' -Width 50 -Height 200)
Assert-True ($projUnbounded.Count -gt $projFit.Count) 'and renders more lines when given the room - proving MinHeight is a real clamp on this screen too'

foreach ($h in @($script:MinHeight, 50)) {
    $nmap = $null
    $lf = @(Get-LaunchFrame -State (New-LaunchState) -Width 50 -Height $h -Limits $narrowLimits -Restored @('Model') -RestoredAge '12 min' -RowMap ([ref]$nmap))
    $lfText = $lf -join "`n"
    # Clickable tokens are bracketed here (no -Color): '[enter] next', not 'enter next'.
    foreach ($hint in @('[enter] next', '[u] maintenance', '[esc] quit', 'w/s row', 'a/d value')) {
        Assert-Equal $true $lfText.Contains($hint) "50x${h} launch: the hint '$hint' is readable"
    }
    Assert-Equal $true ($lf.Count -le ($h - 1)) "50x${h} launch: $($lf.Count) lines leave the headroom row"
    Assert-Equal $true ($lfText.Contains('ctrl+r resets')) "50x${h} launch: the restored line keeps its reset advice"
    Assert-Equal 3 @($nmap.Footer).Count "50x${h} launch: all three actions are clickable"

    $pf = @(Get-PickerFrame -Sessions $narrowSessions -Index 2 -Width 50 -Height $h)
    $pfText = $pf -join "`n"
    # 'w/s move' pinned alongside the picker's other 50-column hints (fix round 1: it was asserted
    # nowhere, the same way 'w/s row' is pinned for the launch screen above).
    foreach ($hint in @('w/s move', '[/] filter', '[enter] open', '[f] fork', '[esc] back')) {
        Assert-Equal $true $pfText.Contains($hint) "50x${h} picker: the hint '$hint' is readable"
    }
    Assert-Equal $true ($pf.Count -le ($h - 1)) "50x${h} picker: $($pf.Count) lines leave the headroom row"

    # Fix round 1, MINOR 2: the SCOPED picker (Task 8's -Scope project, the tab hint and the
    # ProjectName title suffix added on top of the plain picker above) was never rendered at 50
    # columns at all - only the unscoped picker was. A 34-character -ProjectName is the worst case
    # this took: it wrapped the title further than the plain "resume - N sessions" form.
    $spf = @(Get-PickerFrame -Sessions $narrowSessions -Index 2 -Scope 'project' -ProjectName $longProjectName34 -Width 50 -Height $h)
    Assert-Equal $true ($spf.Count -le ($h - 1)) "50x${h} scoped picker: $($spf.Count) lines leave the headroom row"
    Assert-True (($spf -join "`n") -match 'tab') "50x${h} scoped picker: the tab-widen hint is readable"

    $mf = @(Get-MaintenanceFrame -Info $narrowInfo -Width 50 -Height $h -Status $longStatus -Actions $cfgActions)
    $mfText = $mf -join "`n"
    foreach ($hint in @('[u] update', '[r] rename swap', '[d] doctor', '[m] mcp list', '[p] prune', '[i] full reindex', '[esc] back')) {
        Assert-Equal $true $mfText.Contains($hint) "50x${h} maintenance: the hint '$hint' is readable"
    }
    Assert-Equal $true ($mf.Count -le ($h - 1)) "50x${h} maintenance: $($mf.Count) lines with a long status leave the headroom row"

    $pjf = @(Get-ProjectFrame -Projects $projWorstList -Index 5 -Cwd 'C:\x' -Width 50 -Height $h)
    $pjfText = $pjf -join "`n"
    # '[enter] run', not '[enter] new': Enter runs whatever the action field says, and the arrow
    # hint that names the field has to survive the narrowest terminal like every other one.
    foreach ($hint in @('w/s move', "$($tabGlyphs.LAngle) $($tabGlyphs.RAngle) action", '[enter] run', '[c] continue', '[r] resume', '[t] worktree', '[/] filter', '[esc] back')) {
        Assert-Equal $true $pjfText.Contains($hint) "50x${h} project: the hint '$hint' is readable"
    }
    Assert-Equal $true ($pjf.Count -le ($h - 1)) "50x${h} project: $($pjf.Count) lines leave the headroom row"
}

# A click on a hint that wrapped onto a LATER footer line reaches its key - the span's line offset
# is what the hit test uses, and a map that ignored it would send every click to the first line.
$wmap = $null
$null = Get-MaintenanceFrame -Info $narrowInfo -Width 50 -Height 24 -RowMap ([ref]$wmap) -Actions $cfgActions
Assert-Equal $true ($wmap.FooterLines -ge 2) 'at 50 columns the maintenance footer wraps'
$iWrapped = Get-HintSpan -Map $wmap -Char 'i'
Assert-Equal $true ($iWrapped.Line -ge 1) 'the reindex hint sits on a wrapped line'
$wDraw = { param($i, $s) $wmap }.GetNewClosure()
$script:reindexRuns = 0
$w = New-EventReader @((New-MouseEvent -Y ($wmap.FooterY + $iWrapped.Line) -X $iWrapped.Start -Left), (New-MouseEvent -Y ($wmap.FooterY + $iWrapped.Line) -X $iWrapped.Start -Left), $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $wDraw -Wait $w -Actions $cfgActions -Runner $fakeRunner -GetWindowTop { 0 }
Assert-Equal 1 $script:reindexRuns 'clicking a hint on a wrapped footer line presses its key'
$script:reindexRuns = 0
$w = New-EventReader @((New-MouseEvent -Y $wmap.FooterY -X $iWrapped.Start -Left), (New-MouseEvent -Y $wmap.FooterY -X $iWrapped.Start -Left), $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $wDraw -Wait $w -Actions $cfgActions -Runner $fakeRunner -GetWindowTop { 0 }
Assert-Equal 0 $script:reindexRuns 'the same column on the FIRST footer line is not that hint'

# --- Maintenance actions come from the config: no action, no hint; one hint per action; a key with
# no action does nothing; ConfirmTwice=false runs on the first press. ---
$noActions = Get-MaintenanceFrame -Info $narrowInfo -Width 78 -Height 24
Assert-Equal $false (($noActions | ForEach-Object { Remove-AnsiColor $_ }) -join "`n" -match 'full reindex') 'no action, no hint'
$withAction = Get-MaintenanceFrame -Info $narrowInfo -Width 78 -Height 24 -Actions $cfgActions
Assert-Equal $true (($withAction | ForEach-Object { Remove-AnsiColor $_ }) -join "`n" -match '\[i\]\s+full reindex') 'a configured action is a footer hint'
$w = New-EventReader @($iKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions @() -Runner { throw 'must not run' }
Assert-Equal $false ($script:lastStatus -match 'confirm') 'a key with no action does nothing'
$oneShot = @([pscustomobject]@{ Key = 'x'; Label = 'quick'; Script = (Join-Path $PSScriptRoot 'fixtures\config-four.json'); ConfirmTwice = $false })
$xKey = [System.ConsoleKeyInfo]::new([char]'x', [System.ConsoleKey]::X, $false, $false, $false)   # same shape as $iKey
$w = New-EventReader @($xKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $oneShot -Runner { param($p) [pscustomobject]@{ Output = @('done'); ExitCode = 0 } }
Assert-Equal $true ($script:lastStatus -match 'quick finished green') 'confirmTwice=false runs on the first press'

# --- The confirm is STATE, not a parse of the status text (review, fix round 1). ---
# (a) an unmapped key with actions configured: nothing happens, the runner is never called.
$zKey = [System.ConsoleKeyInfo]::new([char]'z', [System.ConsoleKey]::Z, $false, $false, $false)
$script:lastStatus = 'x'
$w = New-EventReader @($zKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner { throw 'must not run' }
Assert-Equal '' $script:lastStatus 'an unmapped key with actions configured leaves the status empty'
# (a2) an armed confirm cancelled by an unrelated key: the "press i again" text must go with it,
# or the screen keeps promising a confirm that the next i would only re-arm.
$script:reindexRuns = 0
$w = New-EventReader @($iKey, $zKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner $fakeRunner
Assert-Equal 0 $script:reindexRuns 'i then z runs nothing'
Assert-Equal '' $script:lastStatus 'and the stale confirm text is cleared by the unrelated key'
# (b) i then p: the action's confirm does not arm prune - prune shows its OWN confirm and runs nothing.
$script:pruneRuns = 0
function Remove-OldClaudeVersions { param($Keep) $script:pruneRuns++; [pscustomobject]@{ Deleted = @(); FreedBytes = 0 } }
$pKey = [System.ConsoleKeyInfo]::new([char]'p', [System.ConsoleKey]::P, $false, $false, $false)
$script:reindexRuns = 0
$w = New-EventReader @($iKey, $pKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner $fakeRunner
Assert-Equal 0 $script:pruneRuns 'i then p does not prune - one confirm never arms another key'
Assert-Equal 0 $script:reindexRuns 'and p did not run the action either'
Assert-Equal $true ($script:lastStatus -match '^confirm: press p again') 'p shows its own confirm text'
# (c) two ConfirmTwice actions: i, x, x runs x exactly once and never i.
$script:actionRuns = @()
$twoActions = @($cfgActions[0], [pscustomobject]@{ Key = 'x'; Label = 'quick'; Script = (Join-Path $PSScriptRoot 'fixtures\config-four.json'); ConfirmTwice = $true })
$w = New-EventReader @($iKey, $xKey, $xKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $twoActions -Runner { param($p) $script:actionRuns += $p; [pscustomobject]@{ Output = @('ok'); ExitCode = 0 } }
Assert-Equal 1 @($script:actionRuns).Count 'i, x, x runs exactly one action'
Assert-Equal $true ($script:lastStatus -match 'quick finished green') 'and it is x, not i'

# --- W5: a PASTE must not satisfy "press it twice" -------------------------------------------
# This screen acts on every character it is handed, and ConfirmTwice was the only brake - so any
# pasted string containing `pp` deleted builds and `ii` started a five-minute fleet reindex, with
# no keypress at all. The brake now also needs the confirming character to have ARRIVED more than
# ConfirmMinMs after the arming one; two characters of one paste arrive microseconds apart.
# The clock is injected because the screen's OWN elapsed time is the wrong instrument: a redraw
# plus Get-ClaudeInstallInfo between two presses can outlast any threshold worth setting, which is
# why the real stamp comes from the input record (asserted in Test-Input.ps1).
$script:pasteClock = 1000
$burstClock = { $script:pasteClock += 10; return $script:pasteClock }
$typedClock = { $script:pasteClock += 400; return $script:pasteClock }

$script:pruneRuns = 0
$script:pasteClock = 1000
$w = New-EventReader @($pKey, $pKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner $fakeRunner -RecordTime $burstClock
Assert-Equal 0 $script:pruneRuns 'two p characters 10 ms apart are a paste, not a confirmed prune'
Assert-Equal $true ($script:lastStatus -match '^confirm: press p again') 'and the screen still stands on its confirm rather than reporting a deletion'

$script:pruneRuns = 0
$script:pasteClock = 1000
$w = New-EventReader @($pKey, $pKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner $fakeRunner -RecordTime $typedClock
Assert-Equal 1 $script:pruneRuns 'two p presses 400 ms apart still prune - the brake is a paste guard, not a lockout'

$script:reindexRuns = 0
$script:pasteClock = 1000
$w = New-EventReader @($iKey, $iKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner $fakeRunner -RecordTime $burstClock
Assert-Equal 0 $script:reindexRuns 'a pasted "ii" does not start the five-minute fleet reindex either'
$script:reindexRuns = 0
$script:pasteClock = 1000
$w = New-EventReader @($iKey, $iKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner $fakeRunner -RecordTime $typedClock
Assert-Equal 1 $script:reindexRuns 'and two real presses still run it'

# A third pasted character must not confirm what the second one re-armed: the guard re-arms on the
# too-fast press, so a long paste of the same letter is a stream of re-arms and never an action.
$script:pruneRuns = 0
$script:pasteClock = 1000
$w = New-EventReader @($pKey, $pKey, $pKey, $pKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner $fakeRunner -RecordTime $burstClock
Assert-Equal 0 $script:pruneRuns 'four pasted p characters still prune nothing'

# With no arrival stamp to offer - the keyboard-only [Console]::ReadKey path, and every test above
# that injects no clock - the gate stays inert rather than inventing a timestamp and blocking a
# confirm the owner really did press twice.
$script:pruneRuns = 0
$w = New-EventReader @($pKey, $pKey, $esc)
Invoke-MaintenanceScreen -ReadKey $w -Draw $statusDraw -Wait $w -GetWindowTop { 0 } -Actions $cfgActions -Runner $fakeRunner -RecordTime { $null }
Assert-Equal 1 $script:pruneRuns 'with no arrival stamp available the confirm behaves exactly as it did before'

# Without -Width the footer is one line, as every caller that never wraps expects.
$oneLine = New-HintFooter -Glyphs (Get-Glyphs) -Hints @(
    @{ Token = 'u'; Label = 'update'; Clickable = $true; Key = ''; Char = 'u' }
    @{ Token = 'r'; Label = 'rename swap'; Clickable = $true; Key = ''; Char = 'r' }
    @{ Token = 'i'; Label = 'full reindex'; Clickable = $true; Key = ''; Char = 'i' })
Assert-Equal 1 @($oneLine.Lines).Count 'no width: one footer line'
$wrapped = New-HintFooter -Glyphs (Get-Glyphs) -Width 20 -Hints @(
    @{ Token = 'u'; Label = 'update'; Clickable = $true; Key = ''; Char = 'u' }
    @{ Token = 'r'; Label = 'rename swap'; Clickable = $true; Key = ''; Char = 'r' }
    @{ Token = 'i'; Label = 'full reindex'; Clickable = $true; Key = ''; Char = 'i' })
Assert-Equal 3 @($wrapped.Lines).Count 'width 20: three padded hints of 10-16 characters take three lines'
Assert-Equal 0 (@($wrapped.Lines | Where-Object { $_.Text.Length -gt 20 }).Count) 'width 20: no footer line exceeds the width'
Assert-Equal '   r  rename swap' $wrapped.Lines[1].Text 'each wrapped line is indented like the first'

# --- roster is configurable; the Remote row is optional ------------------------------------------
# Only .Count is trustworthy off this capture: Get-LaunchRows returns the SAME row hashtables
# Set-LaunchRoster below mutates in place, so the objects behind this variable can still change
# shape even though the variable itself never gets reassigned.
$rowCountBefore = @(Get-LaunchRows).Count
Assert-Equal 'work,personal,low' ((@(Get-LaunchRows) | Where-Object Name -eq 'Account').Values -join ',') 'hidden accounts stay off the row'
Assert-Equal $true ((Get-LaunchRows | ForEach-Object Name) -contains 'Remote') 'remote on: the row exists'
Set-LaunchRoster -Accounts @([pscustomobject]@{ Key = 'me'; Root = 'C:\x'; Label = 'me'; Tint = 'Yellow'; Hidden = $false; Canonical = $true })
Assert-Equal $false ((Get-LaunchRows | ForEach-Object Name) -contains 'Remote') 'remote off: no row'
Assert-Equal 'me' (New-LaunchState).Account 'the default account is the first visible key'
Assert-Equal $script:C.Yellow (Get-AccountTint -Account 'me') 'tint comes from the roster'
Assert-Equal $script:C.Green (Get-AccountTint -Account 'nobody') 'unknown account falls back to Green'
$frame = Get-LaunchFrame -State (New-LaunchState) -Width 78 -Height 24
Assert-Equal 0 (@($frame | Where-Object { (Remove-AnsiColor $_) -match '^\s*.\s*remote' }).Count) 'no remote line is rendered when the row is off'
# One definition of the default account: the canonical key when it is visible, whatever its
# position in the roster; the first visible key only when it is not.
$accB = [pscustomobject]@{ Key = 'b'; Root = 'C:\b'; Label = 'b'; Tint = 'Cyan'; Hidden = $false; Canonical = $false }
$accA = [pscustomobject]@{ Key = 'a'; Root = 'C:\a'; Label = 'a'; Tint = 'Green'; Hidden = $false; Canonical = $true }
Set-LaunchRoster -Accounts @($accB, $accA) -Default 'a'
Assert-Equal 'a' (New-LaunchState).Account 'the canonical account listed second is still the default'
$accA.Hidden = $true
Set-LaunchRoster -Accounts @($accB, $accA) -Default 'a'
Assert-Equal 'b' (New-LaunchState).Account 'a hidden canonical account yields to the first visible key'
Assert-Equal 'b' (Get-LaunchDefaultAccount) 'Get-LaunchDefaultAccount agrees with the UI default'
Set-LaunchRoster -Accounts (Read-LauncherConfig).Accounts -Remote
Assert-Equal $rowCountBefore @(Get-LaunchRows).Count 'restoring the fixture roster restores the row count'

# --- deferred review finding: Set-LaunchRoster's Remote-row splice duplicates row 0 when the
# non-Remote roster is down to ONE row. `$rows[1..($rows.Count - 1)]` counts BACKWARDS when
# Count is 1 (the range is 1..0), yielding $rows[0] again instead of an empty tail. Seven rows
# ship today, so this is reached here by shrinking $script:Rows first - the same script-scope
# variable Screens.ps1 sets, reachable because this file dot-sources it into its own scope. ---
$savedScriptRows = $script:Rows
try {
    $script:Rows = @(@{ Name = 'Account'; Label = 'account'; Values = @('work') })
    Set-LaunchRoster -Accounts @([pscustomobject]@{ Key = 'me'; Root = 'C:\x'; Label = 'me'; Tint = 'Green'; Hidden = $false; Canonical = $true }) -Remote
    $oneRowResult = @(Get-LaunchRows)
    Assert-Equal 2 $oneRowResult.Count 'a one-row roster plus Remote yields two rows, not a duplicated Account row'
    Assert-Equal 'Remote' $oneRowResult[1].Name 'the second row is Remote'
} finally {
    $script:Rows = $savedScriptRows
    Set-LaunchRoster -Accounts (Read-LauncherConfig).Accounts -Remote
}

# --- Task 5: WASD navigates alongside the arrows, on every screen with a cursor ------------------
# Assert-Equal here is (Expected, Actual) - the house order; a written brief's own sample assertions
# had it backwards (see the ruling this task was dispatched with).
$sDown = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('s', 's', 'Enter')) -Draw {}
Assert-Equal 2 $sDown.Row 's moves the launch cursor down twice'
$sBack = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('s', 'w', 'Enter')) -Draw {}
Assert-Equal 0 $sBack.Row 'w moves it back up'

# The modifier guard is not weakened by adding a letter: a SHIFTED W must not navigate. Fix round 1
# (the first version was vacuous both ways: New-LaunchState starts at Row 0, and the w branch is
# `if ($State.Row -gt 0) { $State.Row-- }`, so a shifted W that DID navigate would ALSO leave Row at
# 0). Move first with a plain 's' to Row 1, then the shifted W: guard intact leaves it at 1; a guard
# that let the shifted key through would pull it back to 0.
$ShiftW = [System.ConsoleKeyInfo]::new('W', [System.ConsoleKey]::W, $true, $false, $false)
$sShift = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-MixedKeyReader -Keys @('s', $ShiftW, 'Enter')) -Draw {}
Assert-Equal 1 $sShift.Row 'a shifted W does not navigate the launch screen back up'

# a/d step the selected row's value, exactly like left/right - reuse the same "reach the last row,
# then wrap the value" shape the arrow-key assertions above already use.
$aBack = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys ($remoteDowns + @('a', 'Enter'))) -Draw {}
Assert-Equal 'stop server' $aBack.Remote 'a wraps to the last value of the remote row, like left'
$dForward = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('d', 'Enter')) -Draw {}
Assert-Equal ((Get-LaunchRows | Where-Object Name -eq 'Account').Values | Select-Object -Skip 1 -First 1) $dForward.Account 'd steps the account row forward, like right'

# The session picker: reuse the small two-session fixture from screen 2's own arrow-key assertions.
$sPick = Invoke-SessionPicker -Sessions $fake -ReadKey (New-ScriptedKeyReader -Keys @('s', 'Enter')) -Draw {}
Assert-Equal 'bbbb2222' $sPick.Session.SessionId 's moves the picker selection down, like DownArrow'
$wPick = Invoke-SessionPicker -Sessions $fake -ReadKey (New-ScriptedKeyReader -Keys @('s', 'w', 'Enter')) -Draw {}
Assert-Equal 'aaaa1111' $wPick.Session.SessionId 'w moves it back up'

# Typed filter text must not navigate: 's' and 'w' inside an open filter are characters, not moves.
# Fix round 1: the final session id alone cannot tell filtering from navigating here (s and w cancel
# each other, and Escape resets the index to 0 anyway - both paths land on the same session). $Draw
# already receives (s, i, f) every redraw, so capture what it is actually called with instead.
$script:typedDrawCalls = [System.Collections.Generic.List[object]]::new()
$typedDraw = { param($s, $i, $f) $script:typedDrawCalls.Add([pscustomobject]@{ Index = $i; Filter = $f }); $null }
$typedKeys = @('/', 's', 'w', 'Escape', 'Enter')
$typedPick = Invoke-SessionPicker -Sessions $fake -ReadKey (New-ScriptedKeyReader -Keys $typedKeys) -Draw $typedDraw
$sw = @($script:typedDrawCalls | Where-Object { $_.Filter -eq 'sw' })
Assert-Equal 1 $sw.Count 'w and s typed into an open filter reach it as characters - the filter becomes "sw"'
Assert-Equal 0 $sw[0].Index 'and the index never moved while those letters were being typed'

# --- Task 6: the project screen frame ------------------------------------------------------
$projs6 = @(
    [pscustomobject]@{ Slug = 'A'; Path = 'C:\w\alpha'; Name = 'alpha'; Worktree = $null; LastActivity = (Get-Date).AddMinutes(-2) }
    [pscustomobject]@{ Slug = 'B'; Path = 'C:\w\beta';  Name = 'beta';  Worktree = $null; LastActivity = (Get-Date).AddDays(-1) }
)
$map6 = $null
$f6 = @(Get-ProjectFrame -Projects $projs6 -Index 0 -Cwd 'C:\somewhere' -Width 78 -Height 24 -RowMap ([ref]$map6))
$f6Text = $f6 -join "`n"
Assert-True ($f6Text -match 'alpha') 'the newest project is listed'
Assert-True ($f6Text -match 'current directory') 'the pinned cwd row is there'
Assert-True ($f6Text -match 'enter a path') 'the pinned free-path row is there'
Assert-Equal 4 $map6.RowCount 'two projects plus two pinned rows are hit-testable'
# "Hit-testable like a project row" means the SAME map fields a project row would get, not merely
# a count: FirstRowY is always 1 (one row below the box's own top border, whatever the body holds)
# and Start is the viewport's first visible index - both computed once and shared by every row,
# pinned or not.
Assert-Equal 1 $map6.FirstRowY "the row map's FirstRowY sits just under the box top border"
Assert-Equal 0 $map6.Start 'the row map start is the first visible row'
# Fix round 2 (reviewer correction): with only 2 projects the list never scrolls, so Start is
# always the degenerate 0 above - a mutant that hardcodes Start to 0 survives every assertion in
# this file. Start is what the next task adds to a clicked row's offset, and a scrolled list is
# exactly where that bites. 30 projects, selecting the LAST one, forces the viewport to scroll.
$scroll30 = @(1..30 | ForEach-Object { [pscustomobject]@{ Slug = "S$_"; Path = "C:\w\s$_"; Name = "scrollproj-$_"; Worktree = $null; LastActivity = (Get-Date).AddMinutes(-$_) } })
$mapScroll = $null
$null = @(Get-ProjectFrame -Projects $scroll30 -Index 29 -Cwd 'C:\somewhere' -Width 78 -Height 24 -RowMap ([ref]$mapScroll))
Assert-Equal 1 $mapScroll.FirstRowY 'the scrolled row map still sits just under the box top border'
# 18 and 14, not 19 and 13, since the action field joined the box (2026-09-16): the field's row comes
# out of the LIST's viewport, which is the whole point of putting it inside the box - the frame still
# fits the same terminal, one list row further down.
Assert-Equal 18 $mapScroll.RowCount 'the scrolled row map still reports how many rows are visible'
Assert-Equal 14 $mapScroll.Start 'the scrolled row map start is the actual first visible index, not the degenerate 0'
Assert-True ($f6Text -match 'continue') 'the footer advertises continue'
# Plain (no -Color) hints are bracketed like every other screen's footer - '[t] worktree',
# never ' t worktree' - see the picker's '[f] fork' etc. at 50 columns. The brief's own sample
# checked for ' t ', which only the -Color form of New-HintFooter ever renders; against the -Plain
# contract this call actually takes, that sample is wrong, so the check is against the bracketed
# form - which is exactly what also proves the hotkey is 't' and not 'w': a reverted 'w' prints
# '[w] worktree' here instead.
Assert-True ($f6Text.Contains('[t] worktree')) 'worktree is on t, not w'
# Fix round 1, owner ruling: the current-directory row must show WHICH directory it means, exactly
# like a project row shows its path - '-Cwd' is otherwise accepted and never reaches the page.
Assert-True ($f6Text.Contains('C:\somewhere')) 'the current-directory row shows the actual cwd path'

# Fix round 1, mutation coverage (Select-ProjectMatch -> @($Projects), the filter silently ignored):
# filtering to one match must both shrink the row count (one match + the two pinned rows, not
# both projects + two pinned) and show in the title - the title's own count word ('1 known') is
# what a filter-bypassing mutant cannot fake, because it is driven by $items.Count, not by $Filter.
$mapF6 = $null
$fF6 = @(Get-ProjectFrame -Projects $projs6 -Index 0 -Filter 'alpha' -Cwd 'C:\somewhere' -Width 78 -Height 24 -RowMap ([ref]$mapF6))
$fF6Text = $fF6 -join "`n"
Assert-Equal 3 $mapF6.RowCount 'filtering to one match still offers both pinned rows (the match plus current directory plus enter a path)'
Assert-True ($fF6Text -match '1 known') 'the title counts only the matched project'
Assert-True ($fF6Text -match 'filter: alpha') 'the title states the active filter'

# Fix round 1, CRITICAL 1 (reviewer correction): the previous version of this check rendered at
# Width 50 x (MinHeight-1) - one row BELOW the minimum, which hits the too-small gate
# (Screens.ps1) and returns the 4-line stub, not the real frame; the "Count -le 20" comparison was
# then 4 -le 20, true no matter what the real layout math does. Split into the two things that
# check actually differently distinguishes:
#
# 1) the too-small gate itself, still worth its own assertion, same contract as every other builder.
$f6TooSmall = Get-ProjectFrame -Projects $projs6 -Cwd 'C:\somewhere' -Width 50 -Height ($script:MinHeight - 1)
Assert-Equal 1 (@($f6TooSmall | Where-Object { $_ -match [regex]::Escape("need $($script:MinWidth)x$($script:MinHeight)") }).Count) 'a too-short terminal states the required size'
#
# 2) the real overflow check, rendered AT $script:MinHeight (not one row short of it) with a FULL
# registry: with only two projects the viewport is capped at 4 rows (rows.Count) regardless of
# bodyRows, so a bodyRows miscalculation ($Height - 3 mutated to $Height - 1, or the size gate's
# -lt $script:MinHeight widened to -lt ($script:MinHeight + 3)) cannot be observed - 30 projects
# make the viewport big enough that both mutants change what actually renders.
$many30 = @(1..30 | ForEach-Object { [pscustomobject]@{ Slug = "P$_"; Path = "C:\w\p$_"; Name = "project-$_"; Worktree = $null; LastActivity = (Get-Date).AddMinutes(-$_) } })
$wide30 = @(Get-ProjectFrame -Projects $many30 -Index 0 -Cwd 'C:\somewhere' -Width 50 -Height $script:MinHeight)
$wide30Text = $wide30 -join "`n"
Assert-True ($wide30Text -match 'known') 'the frame at MinHeight is the real render, not the too-small stub (catches the widened size gate)'
Assert-True ($wide30.Count -le ($script:MinHeight - 1)) 'the worst-case 50-column project frame leaves the headroom row (catches the $Height-3 miscalculation)'

# Empty registry still renders and still offers the pinned rows.
$empty6 = @(Get-ProjectFrame -Projects @() -Index 0 -Cwd 'C:\somewhere' -Width 78 -Height 24)
Assert-True (($empty6 -join "`n") -match 'current directory') 'an empty registry still offers the cwd'

# Mutation coverage - viewport reachability: with 2 projects + 2 pinned rows, a viewport sized off
# $items.Count (2) instead of $rows.Count (4) renders only the two projects and the pinned rows
# never appear at all. The three 'is there'/'RowCount' assertions above already catch that by
# absence; this one pins it directly by counting how many of the four expected rows are painted.
Assert-Equal 4 (@($f6 | Where-Object { $_ -match 'alpha|beta|current directory|enter a path' })).Count 'all four rows - two projects, two pinned - are actually painted, not just hit-testable'

# Mutation coverage - path-column Limit-Line: New-Box Limit-Lines every row to the box width
# regardless, so an overflowing raw line proves nothing on its own (see the launch screen's
# 120-column note above) - the age text surviving at the END of the row is what a missing inner
# clamp on the path actually breaks, because New-Box's own clamp then cuts into the overflowing
# tail instead and the trailing ' 5 min' never reaches the page. (Fix round 1, MINOR 4: the raw
# per-line width foreach loops that used to sit here are gone - they ran on the builder's already
# Complete-PickerFrame-clamped return and could never fail regardless of what the layout math did.)
$longNow6 = Get-Date
$longProjs6 = @([pscustomobject]@{ Slug = 'L'; Path = ('C:\' + ('deepfolder\' * 30) + 'end'); Name = 'longname'; Worktree = $null; LastActivity = $longNow6.AddMinutes(-5) })
$lf6 = @(Get-ProjectFrame -Projects $longProjs6 -Index 0 -Cwd 'C:\x' -Width 78 -Height 24 -Now $longNow6)
Assert-True ((($lf6 -join "`n")).Contains('5 min')) 'a long project path is capped so the age at the end of the row survives'

# Fix round 1, MINOR 3: a long NAME must not itself push the age off the row either - same failure
# mode as the long path above, different column.
$longNameProjs6 = @([pscustomobject]@{ Slug = 'N'; Path = 'C:\p'; Name = ('n' * 60); Worktree = $null; LastActivity = (Get-Date).AddMinutes(-5) })
$lnFrame6 = @(Get-ProjectFrame -Projects $longNameProjs6 -Index 0 -Cwd 'C:\x' -Width 50 -Height 24)
Assert-True ((($lnFrame6 -join "`n")).Contains('5 min')) 'a long project name is clamped so the age survives at 50 columns'

# --- Invoke-ProjectScreen (Task 7, fix round 2): the project screen input loop, with throttled
# hover. Driven entirely through injected seams - none of this needs a terminal.
#
# The existence guard now covers every row kind (fix round 2, IMPORTANT 1), so the fixtures below
# are REAL temporary directories - not the fictional 'C:\w\alpha'-style paths round 1 used - and
# are cleaned up in the `finally` at the bottom of this section.
#
# The randomness lives in ONE root directory; alpha/beta are FIXED leaf names under it (fix round
# 3, IMPORTANT). A per-project random leaf (`pp-proj-alpha-<32 hex>`) put the GUID in the very
# string a filter test matches against - filtering on 'be' matched beta's Name by design, but ALSO
# matched alpha's PATH whenever its hex GUID happened to contain the substring 'be' (P=11.41%,
# 22825/200000 measured), a flake reproducible on demand and load-bearing on any machine whose own
# %TEMP% contains 'be'. A shared root does not reintroduce the bug: the filter tests below match on
# 'eta' (from "beta"), and a 32-HEX-digit GUID (`[0-9a-f]` only) can never contain 't' - the
# substring is categorically unreachable from the random component, on either fixture, forever.
$tmpRoot  = Join-Path ([System.IO.Path]::GetTempPath()) ("pp-proj-$([Guid]::NewGuid().ToString('N'))")
$tmpAlpha = Join-Path $tmpRoot 'alpha'
$tmpBeta  = Join-Path $tmpRoot 'beta'
$tmpCwd   = Join-Path ([System.IO.Path]::GetTempPath()) ("pp-proj-cwd-$([Guid]::NewGuid().ToString('N'))")
New-Item -ItemType Directory -Path $tmpAlpha -Force | Out-Null
New-Item -ItemType Directory -Path $tmpBeta -Force | Out-Null
New-Item -ItemType Directory -Path $tmpCwd | Out-Null
try {
    $pProjs = @(
        [pscustomobject]@{ Slug = 'A'; Path = $tmpAlpha; Name = 'alpha'; Worktree = $null; LastActivity = (Get-Date) }
        [pscustomobject]@{ Slug = 'B'; Path = $tmpBeta;  Name = 'beta';  Worktree = $null; LastActivity = (Get-Date).AddDays(-1) }
    )

    $p1 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('Enter')) -Draw {}
    Assert-Equal $tmpAlpha $p1.Path 'Enter on the first row picks it'
    Assert-Equal 'new' $p1.Action 'and Enter means a new session'
    Assert-Equal 'A' $p1.Slug "and returns the row's slug"

    $p2 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('s', 'c')) -Draw {}
    Assert-Equal $tmpBeta $p2.Path 's moves down'
    Assert-Equal 'continue' $p2.Action 'c means continue'
    Assert-Equal 'B' $p2.Slug "and the second row's slug travels with it"

    $p3 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('r')) -Draw {}
    Assert-Equal 'resume' $p3.Action 'r means resume'
    $p4 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('t')) -Draw {}
    Assert-Equal 'worktree' $p4.Action 't means worktree'

    Assert-True ($null -eq (Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw {})) 'Escape cancels'

    # The cwd pinned row is two rows past the last project.
    $p5 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('s', 's', 'Enter')) -Draw {}
    Assert-Equal $tmpCwd $p5.Path 'the pinned current-directory row launches the cwd'
    Assert-Equal '' $p5.Slug 'an unknown cwd carries no slug'

    # The cwd row's slug lookup is case-insensitive and ignores a trailing separator - the session
    # picker (next task) scopes by slug, exact, because two repositories can share a folder name.
    # Still a REAL directory (alpha's own), just spelled with a different case, slash direction and
    # a trailing one - Test-Path resolves all of that natively, so the existence guard is not what
    # this assertion is pinning.
    $alphaVariant = ($tmpAlpha -replace '\\', '/').ToUpperInvariant() + '/'
    $p5b = Invoke-ProjectScreen -Projects $pProjs -Cwd $alphaVariant -ReadKey (New-ScriptedKeyReader -Keys @('s', 's', 'Enter')) -Draw {}
    Assert-Equal 'A' $p5b.Slug 'a cwd matching a known project (case/trailing-slash/slash-direction insensitive) carries its slug'

    # Filter mode: '/' then letters must not fire the action hotkeys. Filters on 'eta' (from
    # "beta"), not 'be': 'be' is entirely hex digits and can match a 32-hex-digit GUID by chance
    # (measured P=11.41% here) - 't' cannot occur in a hex GUID at all, so 'eta' is reachable only
    # through the literal word "beta" (the Name AND now the fixed leaf of the Path).
    $p6 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('/', 'e', 't', 'a', 'Enter', 'Enter')) -Draw {}
    Assert-Equal $tmpBeta $p6.Path 'typing in filter mode narrows instead of acting'

    # A letter that IS a hotkey, typed while filtering, must only edit the filter text - never fire
    # the action. Proven by the run needing a further Escape to leave rather than acting on 'c'.
    $p6b = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('/', 'c', 'Escape', 'Escape')) -Draw {}
    Assert-True ($null -eq $p6b) 'typing the "c" hotkey while filtering only edits the filter text, then Escape leaves'

    # Escape in filter mode clears the filter (first Escape) rather than leaving; a second Escape
    # leaves the screen. Both asserted: the first by what Enter picks afterwards, the second by $null.
    $p6c = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('/', 'z', 'Escape', 'Enter')) -Draw {}
    Assert-Equal $tmpAlpha $p6c.Path 'the first Escape clears the filter text rather than leaving, so Enter picks the unfiltered first row'
    $p6d = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('/', 'z', 'Escape', 'Escape')) -Draw {}
    Assert-True ($null -eq $p6d) 'the second Escape leaves the screen'

    # -Initial puts the cursor on a remembered project.
    $p7 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -Initial $tmpBeta -ReadKey (New-ScriptedKeyReader -Keys @('Enter')) -Draw {}
    Assert-Equal $tmpBeta $p7.Path 'the initial project is preselected'

    # -Initial through ConvertTo-ProjectKey (fix round 2, IMPORTANT 3): a caller passing a
    # differently-cased, forward-slashed, trailing-slashed spelling of the SAME directory must still
    # preselect it - a raw [Array]::IndexOf silently preselected the wrong row (or none) here.
    $betaVariant = ($tmpBeta -replace '\\', '/').ToUpperInvariant() + '/'
    $p7b = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -Initial $betaVariant -ReadKey (New-ScriptedKeyReader -Keys @('Enter')) -Draw {}
    Assert-Equal $tmpBeta $p7b.Path '-Initial matches by normalised key, not exact string - a case/slash variant still preselects beta'

    # The free-path row reads through -ReadPath. A path that does not exist must not be returned -
    # the loop stays open, proven by needing a further Escape to leave rather than returning on Enter.
    $p8 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('s', 's', 's', 'Enter', 'Escape')) -Draw {} -ReadPath { 'C:\this-path-does-not-really-exist-9f3a' }
    Assert-True ($null -eq $p8) 'a free path that does not exist keeps the loop open; Escape then cancels'

    # A real directory typed into the free-path row IS returned, resolved, and its slug looked up the
    # same way the cwd row's is.
    $scratchDir = Join-Path ([System.IO.Path]::GetTempPath()) ("pp-free-$([Guid]::NewGuid().ToString('N'))")
    New-Item -ItemType Directory -Path $scratchDir | Out-Null
    try {
        # The trailing Escape is never reached when the pick succeeds (the function returns on Enter);
        # it is there so a broken existence guard that wrongly rejects a real directory fails this
        # assertion cleanly instead of exhausting the scripted reader with an uncaught exception.
        $p8b = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('s', 's', 's', 'Enter', 'Escape')) -Draw {} -ReadPath { "`"$scratchDir`"" }
        Assert-Equal (Resolve-Path -LiteralPath $scratchDir).Path $p8b.Path 'a real free path is resolved and returned'
        Assert-Equal '' $p8b.Slug 'a free path outside the registry carries no slug'
    } finally { Remove-Item -LiteralPath $scratchDir -Recurse -Force -ErrorAction SilentlyContinue }

    # --- IMPORTANT 1: the existence guard now covers registry rows too. A project the registry still
    # lists but whose directory vanished between launch and this keypress (git worktree remove in
    # another terminal fits) must not be returned - the same call Prefs.ps1 already makes for a
    # remembered project ("this value becomes a Set-Location target"), extended to this screen's own,
    # longer, lifetime. ---
    $vanishedDir = Join-Path ([System.IO.Path]::GetTempPath()) ("pp-proj-vanished-$([Guid]::NewGuid().ToString('N'))")
    New-Item -ItemType Directory -Path $vanishedDir | Out-Null
    Remove-Item -LiteralPath $vanishedDir -Recurse -Force
    $vanishedProjs = @([pscustomobject]@{ Slug = 'V'; Path = $vanishedDir; Name = 'vanished'; Worktree = $null; LastActivity = (Get-Date) })
    $pVanished = Invoke-ProjectScreen -Projects $vanishedProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('Enter', 'Escape')) -Draw {}
    Assert-True ($null -eq $pVanished) 'Enter on a registry row whose directory has vanished does not return - the loop stays open'

    # --- Mouse: a single click only moves the selection; nothing but a double click or a hotkey may
    # start a session. ---
    $pRowMap = [pscustomobject]@{ FirstRowY = 4; RowCount = 4; Start = 0; FooterY = 20; Footer = @(
        [pscustomobject]@{ Key = 'Enter'; Char = '';  Start = 10; End = 14 }
        [pscustomobject]@{ Key = '';      Char = 'c'; Start = 16; End = 23 }
        [pscustomobject]@{ Key = '';      Char = 'r'; Start = 25; End = 30 }
        [pscustomobject]@{ Key = '';      Char = 't'; Start = 32; End = 40 }
    ) }
    $pDraw = { param($p, $i, $f, $t, $h) $pRowMap }.GetNewClosure()

    $w10 = New-EventReader @((New-MouseEvent -Y 5 -Left), $esc)
    $p10 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $w10 -Draw $pDraw -Wait $w10 -GetWindowTop { 0 }
    Assert-True ($null -eq $p10) 'a single click on a project row does not start anything - Escape still cancels'

    $w11 = New-EventReader @((New-MouseEvent -Y 5 -Left), $enterKey)
    $p11 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $w11 -Draw $pDraw -Wait $w11 -GetWindowTop { 0 }
    Assert-Equal $tmpBeta $p11.Path 'the click DID move the selection - Enter afterwards commits the row the click moved to'

    # --- IMPORTANT 2: a double click on a ROW commits, the way Invoke-SessionPicker's does - no
    # further key needed. ---
    # The trailing Escape is never reached when the double click commits (the function returns
    # immediately); it is there so a regression that stops the double click from committing fails
    # this assertion cleanly instead of exhausting the scripted reader with an uncaught exception
    # (fix round 3, SMALL 2).
    $w14 = New-EventReader @((New-MouseEvent -Y 5 -Left -Double), $esc)
    $p14 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $w14 -Draw $pDraw -Wait $w14 -GetWindowTop { 0 }
    Assert-Equal $tmpBeta $p14.Path 'a double click on a row commits it immediately'
    Assert-Equal 'new' $p14.Action 'as a new session, with no further key pressed'

    # A double click landing on a FOOTER button, unlike one landing on a row, does nothing - it
    # mirrors Invoke-MaintenanceScreen excluding IsDoubleClick from its footer-hit guard. The probe:
    # a physical double click over 'c' reaches this loop as two records (a plain press, then one
    # flagged IsDoubleClick) - before the -not IsMove / IsDoubleClick guard, both walked through as
    # a press and the free-path row's -ReadPath fired twice for one gesture.
    $script:dblReadPathCalls = 0
    $dblReadPath = { $script:dblReadPathCalls++; 'C:\this-bogus-path-for-doubleclick-test-9f3a' }
    $sKey = [System.ConsoleKeyInfo]::new([char]'s', 0, $false, $false, $false)
    $w15 = New-EventReader @(
        $sKey, $sKey, $sKey,                        # navigate down to the free-path row (index 3)
        (New-MouseEvent -X 17 -Y 20 -Left),          # the plain press - fires 'c' once
        (New-MouseEvent -X 17 -Y 20 -Left -Double),  # the doubleclick-flagged record - must do nothing
        $esc
    )
    $p15 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $w15 -Draw $pDraw -Wait $w15 -GetWindowTop { 0 } -ReadPath $dblReadPath
    Assert-True ($null -eq $p15) 'the double-click-over-footer run still ends with Escape (the bogus path was never returned)'
    Assert-True ($script:dblReadPathCalls -le 1) 'a double click over a footer button invokes -ReadPath at most once, not once per record'

    # --- Hover: a move inside the same footer button must not redraw. Counting $Draw proves the
    # reduction; a clamp that always passes (e.g. an upper bound with no lower one) would not. ---
    # No .GetNewClosure() here: it wraps the scriptblock in its own private scope, and $script: inside
    # THAT scope binds to the closure's own bubble rather than this file's - $script:pDraws would
    # silently increment a copy nobody ever reads. An ordinary scriptblock resolves $script: against
    # this file's scope, which is what the assertions below actually check.
    $script:pDraws = 0
    $pDrawCounting = { param($p, $i, $f, $t, $h) $script:pDraws++; $pRowMap }
    $w12 = New-EventReader @(
        (New-MouseEvent -X 17 -Y 20 -Move),
        (New-MouseEvent -X 18 -Y 20 -Move),
        $esc
    )
    $p12 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $w12 -Draw $pDrawCounting -Wait $w12 -GetWindowTop { 0 }
    Assert-True ($null -eq $p12) 'the hover run ends with Escape as usual'
    Assert-True ($script:pDraws -le 2) 'two moves inside the same footer button draw at most twice: the initial frame plus one hover change'
    Assert-True ($script:pDraws -ge 1) 'and it did draw at least once, so the upper bound is not trivially satisfied by zero'

    # A move that crosses INTO a different footer button must still redraw each time - the throttle is
    # keyed on the hovered button changing, not on "any move after the first".
    $script:pDraws2 = 0
    $pDrawCounting2 = { param($p, $i, $f, $t, $h) $script:pDraws2++; $pRowMap }
    $w13 = New-EventReader @(
        (New-MouseEvent -X 17 -Y 20 -Move),   # enters the 'c' button - hover changes, redraws
        (New-MouseEvent -X 26 -Y 20 -Move),   # enters the 'r' button - hover changes again, redraws
        $esc
    )
    $p13 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $w13 -Draw $pDrawCounting2 -Wait $w13 -GetWindowTop { 0 }
    Assert-Equal 3 $script:pDraws2 'moving between two DIFFERENT buttons draws for each change: initial + two hover changes'

    # --- IMPORTANT 4: -Hover and -Typing actually change what Get-ProjectFrame paints - round 1's
    # throttle only proved the DRAW COUNT changed, never that a redraw was worth having. ---
    $hoverMap0 = $null
    $frameNoHover = @(Get-ProjectFrame -Projects $pProjs -Index 0 -Cwd $tmpCwd -Width 100 -Height 24 -Color -RowMap ([ref]$hoverMap0))
    $cIndex = [Array]::IndexOf(@($hoverMap0.Footer | ForEach-Object { $_.Char }), 'c')
    $rIndex = [Array]::IndexOf(@($hoverMap0.Footer | ForEach-Object { $_.Char }), 'r')
    $hoverMapC = $null
    $frameHoverC = @(Get-ProjectFrame -Projects $pProjs -Index 0 -Cwd $tmpCwd -Width 100 -Height 24 -Color -Hover $cIndex -RowMap ([ref]$hoverMapC))
    $hoverMapR = $null
    $frameHoverR = @(Get-ProjectFrame -Projects $pProjs -Index 0 -Cwd $tmpCwd -Width 100 -Height 24 -Color -Hover $rIndex -RowMap ([ref]$hoverMapR))
    $footerLineNoHover = $frameNoHover[$hoverMap0.FooterY]
    $footerLineHoverC = $frameHoverC[$hoverMapC.FooterY]
    $footerLineHoverR = $frameHoverR[$hoverMapR.FooterY]
    Assert-Equal (Remove-AnsiColor $footerLineNoHover) (Remove-AnsiColor $footerLineHoverC) 'hovering repaints the footer line without changing its plain text'
    Assert-True ($footerLineHoverC.Contains($script:C.Accent)) 'hovering the c footer button paints its cap with the accent colour'
    Assert-True (-not $footerLineNoHover.Contains($script:C.Accent)) 'no button is accent-tinted when nothing is hovered'
    Assert-True ($footerLineHoverC -ne $footerLineHoverR) 'hovering a different button paints a different frame - the accent follows the hover index, not a fixed spot'
    $typingFrame = @(Get-ProjectFrame -Projects $pProjs -Index 0 -Filter 'al' -Typing -Cwd $tmpCwd -Width 100 -Height 24)
    Assert-True (($typingFrame -join "`n").Contains('filter: al_')) 'typing shows the filter text with a trailing cursor'

    # -Notice's title suffix, pinned directly at the frame level (fix round 3, SMALL 1): a mutation
    # to `if ($false) { ... }` at the call site left every existing assertion green, because nothing
    # checked the RENDERED text - the loop-level notice tests only ever inspected the argument
    # $Draw was called with, never what Get-ProjectFrame did with it.
    $noticeFrame = @(Get-ProjectFrame -Projects $pProjs -Index 0 -Cwd $tmpCwd -Notice 'path not found' -Width 100 -Height 24)
    Assert-True (($noticeFrame -join "`n").Contains('path not found')) '-Notice appears in the rendered title'
    $noNoticeFrame = @(Get-ProjectFrame -Projects $pProjs -Index 0 -Cwd $tmpCwd -Width 100 -Height 24)
    Assert-True (-not (($noNoticeFrame -join "`n").Contains('path not found'))) 'without -Notice, nothing says "path not found"'

    # --- Loop-level tie-in: the throttle is only worth having if the two hover-changed draws in the
    # LOOP actually paint different frames, not merely that $Draw was called a different number of
    # times (round 1's gap - a reverted hover paint would still pass a bare draw-count assertion). ---
    $probeMap = $null
    $null = Get-ProjectFrame -Projects $pProjs -Index 0 -Cwd $tmpCwd -Width 100 -Height 24 -Color -RowMap ([ref]$probeMap)
    $cSpan = @($probeMap.Footer | Where-Object { $_.Char -eq 'c' })[0]
    $rSpan = @($probeMap.Footer | Where-Object { $_.Char -eq 'r' })[0]
    $cY = $probeMap.FooterY + $cSpan.Line
    $rY = $probeMap.FooterY + $rSpan.Line
    $script:capturedFrames = New-Object System.Collections.Generic.List[string]
    $realDraw = {
        param($p, $i, $f, $t, $h, $n)
        $map = $null
        $lines = Get-ProjectFrame -Projects $p -Index $i -Filter $f -Typing:$t -Hover $h -Notice $n -Cwd $tmpCwd -Width 100 -Height 24 -Color -RowMap ([ref]$map)
        $script:capturedFrames.Add(($lines -join "`n"))
        $map
    }
    $wReal = New-EventReader @(
        (New-MouseEvent -X $cSpan.Start -Y $cY -Move),
        (New-MouseEvent -X $rSpan.Start -Y $rY -Move),
        $esc
    )
    $pRealHover = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $wReal -Draw $realDraw -Wait $wReal -GetWindowTop { 0 }
    Assert-True ($null -eq $pRealHover) 'the real-render hover run also ends with Escape'
    Assert-Equal 3 $script:capturedFrames.Count 'three real frames were drawn: initial, hover-c, hover-r'
    Assert-True ($script:capturedFrames[0] -ne $script:capturedFrames[1]) 'hovering c changes the rendered frame from the unhovered one'
    Assert-True ($script:capturedFrames[1] -ne $script:capturedFrames[2]) 'hovering r changes the rendered frame from hovering c'

    # --- Silent rejection now leaves a notice (minor): a bogus free path shows up in the title the
    # NEXT time the frame draws, and the loop clears it again on the following key. ---
    $script:capturedNotice = $null
    $noticeDraw = { param($p, $i, $f, $t, $h, $n) $script:capturedNotice = $n }
    $pNotice = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('s', 's', 's', 'Enter', 'Escape')) -Draw $noticeDraw -ReadPath { 'C:\bogus-notice-test-9f3a' }
    Assert-True ($null -eq $pNotice) 'the bogus free path run still ends with Escape'
    Assert-Equal 'path not found' $script:capturedNotice 'a rejected pick leaves a notice for the frame to show'

    # --- Fix round 3 (coordinator ruling): the notice clears on the next KEY only, never on a mouse
    # move/wheel/hover change - a hover that wiped "path not found" before the owner could read it
    # would defeat the notice. Sequence: bogus free path -> Enter (notice shown) -> a mouse move onto
    # a DIFFERENT footer button (a real hover change, so it does draw again) -> the draw right after
    # that move must STILL carry the notice -> then a real key -> the draw after THAT is empty. ---
    $script:noticeSequence = New-Object System.Collections.Generic.List[string]
    $noticeSeqDraw = { param($p, $i, $f, $t, $h, $n) $script:noticeSequence.Add($n); $pRowMap }
    $sKey3 = [System.ConsoleKeyInfo]::new([char]'s', 0, $false, $false, $false)
    $wKey3 = [System.ConsoleKeyInfo]::new([char]'w', 0, $false, $false, $false)
    $w17 = New-EventReader @(
        $sKey3, $sKey3, $sKey3,               # navigate to the free-path row (index 3)
        $enterKey,                             # Enter -> bogus path -> pick fails -> notice set
        (New-MouseEvent -X 17 -Y 20 -Move),    # hovers onto the 'c' footer button - a REAL hover
                                                # change, so this DOES force another draw - but must
                                                # not clear the notice
        $wKey3,                                # a real key: moves the cursor up AND clears the notice
        $esc
    )
    $p17 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $w17 -Draw $noticeSeqDraw -Wait $w17 -GetWindowTop { 0 } -ReadPath { 'C:\bogus-notice-persists-9f3a' }
    Assert-True ($null -eq $p17) 'the notice-persistence run still ends with Escape'
    Assert-Equal 7 $script:noticeSequence.Count 'one draw per event handled: 3 navigation, the failed Enter, the hover move, the clearing key, and the one after it'
    Assert-Equal 'path not found' $script:noticeSequence[4] 'the notice appears in the draw right after the rejected Enter'
    Assert-Equal 'path not found' $script:noticeSequence[5] 'a mouse move (even one that changes the hover) leaves the notice standing'
    Assert-Equal '' $script:noticeSequence[6] 'the next KEY event clears it'

    # --- Minor: \ / : are now accepted filter characters, so a pasted path matches literally
    # end to end - typing alpha's own full path (colon and backslashes included) as the filter, then
    # Enter, picks alpha. ---
    $pathChars = @($tmpAlpha.ToCharArray() | ForEach-Object { "$_" })
    $pFilterPath = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys (@('/') + $pathChars + @('Enter', 'Enter'))) -Draw {}
    Assert-Equal $tmpAlpha $pFilterPath.Path 'typing a full path (colon and backslashes included) as the filter matches it literally'

    # --- Coverage: the wheel moves the selection like w/s; Ctrl+C leaves like Escape; an uppercase C
    # does not fire continue - Test-ClaudeHotkey's case guard, proven on THIS screen's own hotkeys too.
    $wWheel = New-EventReader @((New-MouseEvent -Wheel -128), $enterKey)
    $pWheelDown = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $wWheel -Draw {} -Wait $wWheel -GetWindowTop { 0 }
    Assert-Equal $tmpBeta $pWheelDown.Path 'the wheel moves the selection down, like s'

    $wWheel2 = New-EventReader @((New-MouseEvent -Wheel -128), (New-MouseEvent -Wheel 128), $enterKey)
    $pWheelBack = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $wWheel2 -Draw {} -Wait $wWheel2 -GetWindowTop { 0 }
    Assert-Equal $tmpAlpha $pWheelBack.Path 'down then up on the wheel comes back'

    $pCtrlC = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-MixedKeyReader -Keys @($CtrlC)) -Draw {}
    Assert-True ($null -eq $pCtrlC) 'Ctrl+C leaves like Escape'

    # Built with the REAL virtual key (fix round 3, SMALL 3): New-ScriptedKeyReader's single-char
    # entries carry ConsoleKey 0, so the virtual-key branch of Test-ClaudeHotkey never matches
    # regardless of the case guard, and lifting that guard left this assertion green for the wrong
    # reason. With Key = [ConsoleKey]::C, removing the IsUpper guard WOULD make the virtual-key
    # match succeed - this is what makes the case guard itself the thing under test.
    $upperCKey = [System.ConsoleKeyInfo]::new([char]'C', [System.ConsoleKey]::C, $false, $false, $false)
    $pUpperC = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-MixedKeyReader -Keys @($upperCKey, 'Escape')) -Draw {}
    Assert-True ($null -eq $pUpperC) 'an uppercase C does not fire continue - the same case guard every hotkey has'

    # --- The action field (owner ask 2026-09-16: arrows and Enter must be enough) ------------------
    # The project screen kept every hotkey and gained a launch-screen-style row under the list:
    # 'action  < new >', cycled with Left/Right from ANY row. Enter then runs what it says, so the
    # whole screen is driveable with four arrows and Enter - which is what the owner asked for -
    # while c/r/t keep firing immediately and set the field, so the screen says what just happened.
    $gg = Get-Glyphs

    Assert-Equal 'new,continue,resume,worktree' ((Get-ProjectActions) -join ',') 'the field cycles the four actions claude-auto.ps1 already dispatches on, in that order'
    Assert-Equal 'continue' (Step-ProjectAction -Action 'new' -Delta 1) 'the stepper moves forward'
    Assert-Equal 'worktree' (Step-ProjectAction -Action 'new' -Delta -1) 'and wraps backwards, like Step-LaunchValue'
    Assert-Equal 'new' (Step-ProjectAction -Action 'not-an-action' -Delta 0) 'a value outside the four falls back to the first rather than travelling on'

    # --- Step-Option: the one wrap-stepper both screens use ---------------------------------------------
    $vals = @('new', 'continue', 'resume', 'worktree')
    Assert-Equal 'continue' (Step-Option -Values $vals -Current 'new' -Delta 1) 'Step-Option steps forward'
    Assert-Equal 'worktree' (Step-Option -Values $vals -Current 'new' -Delta -1) 'and wraps backwards from the first'
    Assert-Equal 'new' (Step-Option -Values $vals -Current 'worktree' -Delta 1) 'and wraps forwards from the last'
    Assert-Equal 'resume' (Step-Option -Values $vals -Current 'RESUME' -Delta 0) 'a differently-cased value canonicalises to the list''s spelling'
    Assert-Equal 'continue' (Step-Option -Values $vals -Current 'bogus' -Delta 1) 'an unknown value steps from the first'
    Assert-Equal 'new' (Step-Option -Values $vals -Current 'new' -Delta 8) 'a delta larger than the list wraps by modulo'

    # Left/Right from a LIST row: the owner never has to walk down to the field to use it.
    $pAct1 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('RightArrow', 'Enter')) -Draw {}
    Assert-Equal 'continue' $pAct1.Action 'RightArrow on a project row steps the field, and Enter runs what it says'
    Assert-Equal $tmpAlpha $pAct1.Path 'on the highlighted project, which the arrow did not move'
    $pAct2 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('RightArrow', 'RightArrow', 'Enter')) -Draw {}
    Assert-Equal 'resume' $pAct2.Action 'two rights reach resume'
    $pAct3 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('RightArrow', 'RightArrow', 'RightArrow', 'RightArrow', 'Enter')) -Draw {}
    Assert-Equal 'new' $pAct3.Action 'four rights wrap back to new'
    $pAct4 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('LeftArrow', 'Enter')) -Draw {}
    Assert-Equal 'worktree' $pAct4.Action 'one left from new wraps to worktree'

    # -InitialAction seeds it from the remembered preference, and is validated there too: the value
    # arrives from an ordinary text file (Prefs.ps1) and ends up deciding a command line.
    $pAct5 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -InitialAction 'resume' -ReadKey (New-ScriptedKeyReader -Keys @('Enter')) -Draw {}
    Assert-Equal 'resume' $pAct5.Action '-InitialAction seeds the field, so Enter alone reproduces the last launch'
    $pAct5b = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -InitialAction 'rm -rf' -ReadKey (New-ScriptedKeyReader -Keys @('Enter')) -Draw {}
    Assert-Equal 'new' $pAct5b.Action 'an -InitialAction outside the four falls back to new'

    # Down STOPS at the free-path row, as it did before the field existed (review W2): the field is
    # an indicator, never a cursor stop. Five downs and the cursor is still on the last list row -
    # which is what keeps the arrows-only path off a Read-Host prompt nobody asked for.
    $script:actNav = New-Object System.Collections.Generic.List[object]
    $navDraw = { param($p, $i, $f, $t, $h, $n, $a) $script:actNav.Add([pscustomobject]@{ Index = $i; Action = "$a" }); $null }
    $pAct6 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('s', 's', 's', 's', 's', 'w', 'Escape')) -Draw $navDraw
    Assert-True ($null -eq $pAct6) 'the navigation run ends with Escape'
    Assert-Equal 7 $script:actNav.Count 'one draw per key handled'
    Assert-Equal 0 $script:actNav[0].Index 'the screen opens on the first row'
    Assert-Equal 3 $script:actNav[3].Index 'three downs reach the last list row (alpha, beta, current directory, enter a path)'
    Assert-Equal 3 $script:actNav[4].Index 'a fourth down goes no further - there is no row past it'
    Assert-Equal 3 $script:actNav[5].Index 'nor a fifth'
    Assert-Equal 2 $script:actNav[6].Index 'and up moves back into the list one row at a time'

    # a/d step the field from ANY row, unconditionally (review W3): every other screen with a cursor
    # treats them as Left/Right, and a trained key that works on some rows only is worse than none.
    $pAct7 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('d', 'Enter')) -Draw {}
    Assert-Equal 'continue' $pAct7.Action 'd on a project row steps the field, like RightArrow'
    Assert-Equal $tmpAlpha $pAct7.Path 'and does not move the selection'

    $actScratch = Join-Path ([System.IO.Path]::GetTempPath()) ("pp-act-$([Guid]::NewGuid().ToString('N'))")
    New-Item -ItemType Directory -Path $actScratch | Out-Null
    try {
        $pAct8 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('a', 'Enter')) -Draw {}
        Assert-Equal 'worktree' $pAct8.Action 'a on a project row steps it left, wrapping, like LeftArrow'
        $pAct9 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('s', 's', 's', 'd', 'Enter', 'Escape')) -Draw {} -ReadPath { $actScratch }
        Assert-Equal 'continue' $pAct9.Action 'and on the free-path row too - there is no row where the key is dead'
        Assert-Equal (Resolve-Path -LiteralPath $actScratch).Path $pAct9.Path 'which still prompts for the path first'

        # Enter on the free-path row prompts FIRST and then runs the selected action.
        $pAct10 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('RightArrow', 'RightArrow', 's', 's', 's', 'Enter', 'Escape')) -Draw {} -ReadPath { $actScratch }
        Assert-Equal 'resume' $pAct10.Action 'Enter on the free-path row runs the selected action, not a hardcoded new'
        Assert-Equal (Resolve-Path -LiteralPath $actScratch).Path $pAct10.Path 'after prompting for the path'
    } finally { Remove-Item -LiteralPath $actScratch -Recurse -Force -ErrorAction SilentlyContinue }

    # c/r/t still fire immediately AND set the field. Driven on the VANISHED fixture so the pick is
    # rejected and the loop draws again - the only way to see the field the press left behind.
    $script:actHot = New-Object System.Collections.Generic.List[string]
    $hotDraw = { param($p, $i, $f, $t, $h, $n, $a, $oa) $script:actHot.Add("$a"); $null }
    $pAct11 = Invoke-ProjectScreen -Projects $vanishedProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('t', 'Escape')) -Draw $hotDraw
    Assert-True ($null -eq $pAct11) 'the hotkey-on-a-vanished-row run ends with Escape'
    Assert-Equal 'new' $script:actHot[0] 'the field starts at new'
    Assert-Equal 'worktree' $script:actHot[1] 't sets the field as well as firing it - the screen shows what happened'
    $script:actHot = New-Object System.Collections.Generic.List[string]
    $null = Invoke-ProjectScreen -Projects $vanishedProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('c', 'Escape')) -Draw $hotDraw
    Assert-Equal 'continue' $script:actHot[1] 'c does too'
    $script:actHot = New-Object System.Collections.Generic.List[string]
    $null = Invoke-ProjectScreen -Projects $vanishedProjs -Cwd $tmpCwd -ReadKey (New-ScriptedKeyReader -Keys @('r', 'Escape')) -Draw $hotDraw
    Assert-Equal 'resume' $script:actHot[1] 'and so does r'

    # --- the field on the FRAME: a launch-screen-style row under the list, with clickable caps ---
    $mapAct = $null
    $fAct = @(Get-ProjectFrame -Projects $pProjs -Index 0 -Cwd $tmpCwd -Width 80 -Height 24 -Action 'resume' -RowMap ([ref]$mapAct))
    $fActText = $fAct -join "`n"
    Assert-True ($fActText.Contains("$($gg.LAngle) resume $($gg.RAngle)")) 'the field shows the selected action between the caps the launch screen collapses its own rows to'
    Assert-True ($fAct[$mapAct.Action.Y].Contains('action')) 'the row map points at the line the field was drawn on'
    Assert-Equal 2 @($mapAct.Action.Cells).Count 'both caps are click cells'
    Assert-Equal (-1) $mapAct.Action.Cells[0].Delta 'the left cap steps back'
    Assert-Equal 1 $mapAct.Action.Cells[1].Delta 'the right cap steps forward'
    Assert-True ($fAct[$mapAct.Action.Y].Substring($mapAct.Action.Cells[0].Start, 1) -eq "$($gg.LAngle)") 'the left cap cell covers the left cap glyph'
    Assert-True ($fAct[$mapAct.Action.Y].Substring($mapAct.Action.Cells[1].Start, 1) -eq "$($gg.RAngle)") 'and the right cap cell the right one'
    Assert-True ($fActText -match ([regex]::Escape(" $($gg.Cursor) alpha"))) 'the cursor is on the list - the field never takes it'
    Assert-True (-not ($fActText -match ([regex]::Escape("$($gg.Cursor) action")))) 'and the field carries no cursor of its own, on any frame'

    # W4: the field is drawn through the SAME canonicaliser it is stepped with, so a hand-supplied
    # 'RESUME' cannot render one string and step from another (PowerShell's -in and -eq are
    # case-insensitive; [Array]::IndexOf is not).
    $fCase = @(Get-ProjectFrame -Projects $pProjs -Index 0 -Cwd $tmpCwd -Width 80 -Height 24 -Action 'RESUME')
    Assert-True (($fCase -join "`n").Contains("$($gg.LAngle) resume $($gg.RAngle)")) "a differently-cased action renders as the canonical one, never raw"
    Assert-Equal 'worktree' (Step-ProjectAction -Action 'RESUME' -Delta 1) 'and steps from where it is shown - forward off resume is worktree, not continue'
    Assert-Equal 'resume' (Step-ProjectAction -Action 'ReSuMe' -Delta 0) 'stepping by zero is the canonicaliser every caller shares'
    $pCase = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -InitialAction 'RESUME' -ReadKey (New-ScriptedKeyReader -Keys @('RightArrow', 'Enter')) -Draw {}
    Assert-Equal 'worktree' $pCase.Action 'and the loop seeds from the canonical value too, so one RightArrow reaches worktree'

    # The footer names the arrows - measured at 80 columns rather than guessed: the hint may not
    # cost the project footer a line it did not need before.
    $map80 = $null
    $f80 = @(Get-ProjectFrame -Projects $pProjs -Index 0 -Cwd $tmpCwd -Width 80 -Height 24 -RowMap ([ref]$map80))
    Assert-True (($f80 -join "`n").Contains("$($gg.LAngle) $($gg.RAngle) action")) 'the footer names the arrows at 80 columns'
    Assert-Equal 2 $map80.FooterLines 'and still fits the two footer lines the screen already had at 80 columns'

    # The extra row is absorbed by the list viewport, never by MinHeight (which is measured off the
    # LAUNCH frame alone - Test-Maintenance pins it): fit AND clamp, the pair the picker and project
    # screens are already held to further down this file.
    $actBig = @(1..40 | ForEach-Object { [pscustomobject]@{ Slug = "ab$_"; Path = "C:\Users\sample-user\Projects\project-name-$_"; Name = "project-name-$_"; Worktree = $null; LastActivity = (Get-Date).AddHours(-$_) } })
    $actFit = @(Get-ProjectFrame -Projects $actBig -Index 5 -Cwd 'C:\x' -Width 50 -Height $script:MinHeight -Action 'worktree')
    Assert-True ($actFit.Count -le ($script:MinHeight - 1)) 'a 40-project frame WITH the field still leaves the headroom row at MinHeight'
    Assert-True (($actFit -join "`n") -match 'known') 'and it is a REAL render at MinHeight, not the too-small stub'
    Assert-True (($actFit -join "`n").Contains("$($gg.LAngle) worktree $($gg.RAngle)")) 'with the field on it - the row is never the one dropped to make it fit'
    $actUnbounded = @(Get-ProjectFrame -Projects $actBig -Index 5 -Cwd 'C:\x' -Width 50 -Height 200 -Action 'worktree')
    Assert-True ($actUnbounded.Count -gt $actFit.Count) 'and it still clamps - more lines when given the room'

    # --- mouse: a click on a cap steps the field, exactly like a click on a launch-screen option
    # cell, and it does not disturb which row Enter would commit. ---
    $capMap = $null
    $null = Get-ProjectFrame -Projects $pProjs -Index 0 -Cwd $tmpCwd -Width 80 -Height 24 -RowMap ([ref]$capMap)
    $capDraw = { param($p, $i, $f, $t, $h, $n, $a, $oa) $capMap }.GetNewClosure()
    $capRight = $capMap.Action.Cells[1]
    $wCap = New-EventReader @((New-MouseEvent -X $capRight.Start -Y $capMap.Action.Y -Left), $enterKey)
    $pCap = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $wCap -Draw $capDraw -Wait $wCap -GetWindowTop { 0 }
    Assert-Equal 'continue' $pCap.Action 'clicking the right cap steps the field forward'
    Assert-Equal $tmpAlpha $pCap.Path 'without moving the selection off the highlighted project'
    $capLeft = $capMap.Action.Cells[0]
    $wCap2 = New-EventReader @((New-MouseEvent -X $capLeft.Start -Y $capMap.Action.Y -Left), $enterKey)
    $pCap2 = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $wCap2 -Draw $capDraw -Wait $wCap2 -GetWindowTop { 0 }
    Assert-Equal 'worktree' $pCap2.Action 'clicking the left cap steps it back, wrapping'

    # A double click on a row commits the FIELD, not a hardcoded 'new'.
    $rightArrowKey = [System.ConsoleKeyInfo]::new([char]0, [System.ConsoleKey]::RightArrow, $false, $false, $false)
    $wDbl = New-EventReader @($rightArrowKey, (New-MouseEvent -Y 5 -Left -Double), $esc)
    $pDbl = Invoke-ProjectScreen -Projects $pProjs -Cwd $tmpCwd -ReadKey $wDbl -Draw $pDraw -Wait $wDbl -GetWindowTop { 0 }
    Assert-Equal 'continue' $pDbl.Action 'a double click commits whatever the field says'
    Assert-Equal $tmpBeta $pDbl.Path 'on the row it landed on'
} finally {
    Remove-Item -LiteralPath $tmpRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $tmpCwd -Recurse -Force -ErrorAction SilentlyContinue
}

# --- picker paging ------------------------------------------------------------------------------
# A cold listing of 40 transcripts is what the launcher pays before the picker can draw anything.
# The picker now takes a FIRST PAGE plus a way to ask for the next one, and asks only when the
# cursor reaches the last row - so the frame appears after one page's worth of work and the rest is
# paid for by whoever actually scrolls that far.
$pgPage1 = @(
    [pscustomobject]@{ SessionId='p1a'; Path='X:\p\p1a.jsonl'; Slug='S'; Project='Paged'; Worktree=$null; Modified=(Get-Date '2026-09-01 10:00'); SizeBytes=100; PromptCount=3; Title='page one first'; LastUser='u'; LastAssistant='a' }
    [pscustomobject]@{ SessionId='p1b'; Path='X:\p\p1b.jsonl'; Slug='S'; Project='Paged'; Worktree=$null; Modified=(Get-Date '2026-09-01 09:00'); SizeBytes=100; PromptCount=3; Title='page one second'; LastUser='u'; LastAssistant='a' }
)
$pgPage2 = @(
    [pscustomobject]@{ SessionId='p2a'; Path='X:\p\p2a.jsonl'; Slug='S'; Project='Paged'; Worktree=$null; Modified=(Get-Date '2026-09-01 08:00'); SizeBytes=100; PromptCount=3; Title='page two first'; LastUser='u'; LastAssistant='a' }
    [pscustomobject]@{ SessionId='p2b'; Path='X:\p\p2b.jsonl'; Slug='S'; Project='Paged'; Worktree=$null; Modified=(Get-Date '2026-09-01 07:00'); SizeBytes=100; PromptCount=3; Title='page two second'; LastUser='u'; LastAssistant='a' }
)

# Down to the last row of page one, Down again to grow, Enter. The fetcher records what it was
# asked for, so "asked once, for the rows it already had" is asserted rather than assumed.
$script:pgAsked = @()
# No .GetNewClosure() and no leading comma: a closure's $script: writes land in the closure's own
# scope and never reach this file, and ',$array' returns the array as ONE element, which the picker
# would then append as a single nested row.
$pgFetch = { param($have) $script:pgAsked += $have; $pgPage2 }
$pgSel = Invoke-SessionPicker -Sessions $pgPage1 -FetchMore $pgFetch -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow','DownArrow','Enter')) -Draw {}
Assert-Equal 'p2a' $pgSel.Session.SessionId 'reaching the last row fetches the next page and the cursor lands on its first row'
Assert-Equal '2' ($script:pgAsked -join ',') 'the fetcher is asked exactly once, for the number of rows already on the list'

# An empty page means the end. The picker must stop asking - otherwise every further Down hits disk
# for nothing - and must still open the row the cursor is on.
$script:pgAsked = @()
$pgEmptyFetch = { param($have) $script:pgAsked += $have; @() }
$pgSelEnd = Invoke-SessionPicker -Sessions $pgPage1 -FetchMore $pgEmptyFetch -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow','DownArrow','DownArrow','Enter')) -Draw {}
Assert-Equal '2' ($script:pgAsked -join ',') 'an empty page ends the paging - the fetcher is not asked again on the next Down'
Assert-Equal 'p1b' $pgSelEnd.Session.SessionId 'and Enter still opens the row the cursor is on'

# The window moves when a session is written while the picker is open, so the next page can overlap
# the last one. A row already on the list must not appear twice.
$script:pgAsked = @()
$pgOverlapFetch = { param($have) $script:pgAsked += $have; @(@($pgPage1[1]) + @($pgPage2[0])) }
$script:pgDrawn = 0
$pgOverlapDraw = { param($s, $i, $f, $sc, $pn) $script:pgDrawn = @($s).Count; $null }
$pgSelOverlap = Invoke-SessionPicker -Sessions $pgPage1 -FetchMore $pgOverlapFetch -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow','DownArrow','Enter')) -Draw $pgOverlapDraw
Assert-Equal 3 $script:pgDrawn 'a page that overlaps the previous one adds only the row that was not already there'
Assert-Equal 'p2a' $pgSelOverlap.Session.SessionId 'and the cursor still lands on the genuinely new row'

# No fetcher at all is every existing caller: the picker behaves exactly as it always has, and Down
# at the last row does nothing rather than throwing on a $null scriptblock.
$pgNoFetch = Invoke-SessionPicker -Sessions $pgPage1 -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow','DownArrow','Enter')) -Draw {}
Assert-Equal 'p1b' $pgNoFetch.Session.SessionId 'without a fetcher the cursor stops at the last row, exactly as before'

# A transcript past the prompt counter's byte budget reports "N+" instead of a number (Sessions.ps1,
# Measure-ClaudePrompts). Select-ResumableSessions drops a session on PromptCount -gt 0, and dropping
# the machine's biggest sessions from the picker would be the worst possible way to pay for that
# bound - so the capped form is pinned here, where Screens.ps1 is actually loaded.
Assert-Equal 1 (Select-ResumableSessions -Sessions @([pscustomobject]@{ SessionId='big'; PromptCount='120+' })).Count 'a capped prompt count still reads as a resumable session'

# PromptCount is an INT and PromptCountCapped is the marker; the picker renders the '+' from the
# flag. The old single string was compared with -gt, and PowerShell coerces the other operand to the
# LEFT one's type: '0+' -gt 0 is TRUE, so a >4 MB transcript whose first 4 MB holds nothing a human
# typed was offered as resumable (adversarial review 2026-09-16, E4a/E4b/E4c).
Assert-Equal '3+' (Format-PromptCount -Session ([pscustomobject]@{ PromptCount = 3; PromptCountCapped = $true })) 'a capped count renders as N+'
Assert-Equal '3' (Format-PromptCount -Session ([pscustomobject]@{ PromptCount = 3; PromptCountCapped = $false })) 'an exact count renders as a plain number'
Assert-Equal '120+' (Format-PromptCount -Session ([pscustomobject]@{ PromptCount = '120+' })) 'a row cached by an older build, carrying the string form, still renders'
Assert-Equal 0 (Select-ResumableSessions -Sessions @([pscustomobject]@{ SessionId = 'zerocap'; PromptCount = 0; PromptCountCapped = $true })).Count 'a capped ZERO has nothing to resume into and is dropped, exactly as an exact zero is'
Assert-Equal 0 (Select-ResumableSessions -Sessions @([pscustomobject]@{ SessionId = 'zerocapstr'; PromptCount = '0+' })).Count 'and so is the old string form of the same row, where ''0+'' -gt 0 used to be TRUE'

# --- paging: the offset, what "the end" means, and the SCOPE ---------------------------------------
$pgRow = {
    param([string]$Id, [string]$Slug = 'S', [string]$Project = 'Paged')
    [pscustomobject]@{ SessionId = $Id; Path = "X:\p\$Id.jsonl"; Slug = $Slug; Project = $Project; Worktree = $null
                       Modified = (Get-Date '2026-09-01 10:00'); SizeBytes = 100; PromptCount = 3
                       Title = "title $Id"; LastUser = 'u'; LastAssistant = 'a'; RecentMessages = @() }
}
# An all-overlap page is the exact case the dedup exists for - one page's worth of appends while the
# picker is open. Equating "added nothing" with "the end" ended the paging permanently there.
$ovHeld = @((& $pgRow 'o1'), (& $pgRow 'o2'))
$ovGrown = Expand-SessionPage -Sessions $ovHeld -FetchMore { param($have, $scope) @($ovHeld) } -Fetched 2
Assert-Equal 0 $ovGrown.Added 'a page that is entirely overlap adds no row'
Assert-Equal $false $ovGrown.Exhausted 'but an overlapping page is not the end - unseen rows can still be behind it'
Assert-Equal $true (Expand-SessionPage -Sessions $ovHeld -FetchMore { param($have, $scope) @() } -Fetched 2).Exhausted 'an EMPTY page is the end'
Assert-Equal 4 $ovGrown.Fetched 'the fetched offset counts what the fetcher returned, not what survived the dedup'
$script:ovAsked = @()
$ovFetch = { param($have, $scope) $script:ovAsked += $have; @((& $pgRow 'o2'), (& $pgRow 'o3')) }
$ovG1 = Expand-SessionPage -Sessions $ovHeld -FetchMore $ovFetch -Fetched 2
$ovG2 = Expand-SessionPage -Sessions $ovG1.Sessions -FetchMore $ovFetch -Fetched $ovG1.Fetched
Assert-Equal 3 $ovG1.Sessions.Count 'an overlapping page leaves fewer rows held than were read'
Assert-Equal '2,4' ($script:ovAsked -join ',') 'and the next page is asked for from the FETCHED offset, not from the deduped row count'
Assert-Equal 6 $ovG2.Fetched 'the offset keeps advancing past an overlap rather than re-reading it forever'

# An empty first page: one Down must land on the FIRST fetched row, not step over it.
$script:emptyAsked = @()
$emptyFetch = { param($have, $scope) $script:emptyAsked += $have; @((& $pgRow 'e1'), (& $pgRow 'e2')) }
$emptySel = Invoke-SessionPicker -Sessions @() -FetchMore $emptyFetch -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow', 'Enter')) -Draw {}
Assert-Equal '0' ($script:emptyAsked -join ',') 'an empty first page asks the fetcher from offset 0'
Assert-Equal 'e1' $emptySel.Session.SessionId 'and the cursor lands on the first fetched row, not the second'

# A filter that hides every loaded row: Down must not become a synchronous cold disk page.
$script:filterAsked = 0
$filterFetch = { param($have, $scope) $script:filterAsked++; @((& $pgRow 'f9')) }
$null = Invoke-SessionPicker -Sessions $ovHeld -FetchMore $filterFetch -Draw {} `
        -ReadKey (New-ScriptedKeyReader -Keys @('/', 'z', 'z', 'z', 'Enter', 'DownArrow', 'DownArrow', 'DownArrow', 'DownArrow', 'Escape'))
Assert-Equal 0 $script:filterAsked 'a filter that matches nothing turns Down into no disk page at all'

# THE SCOPE. The picker pages the project it is scoped to, and Tab asks for page 1 of the account.
$mineRows = @((& $pgRow 'm1' 'MINE' 'Mine'), (& $pgRow 'm2' 'MINE' 'Mine'), (& $pgRow 'm3' 'MINE' 'Mine'))
$otherRows = @((& $pgRow 'x1' 'OTHER' 'Other'), (& $pgRow 'x2' 'OTHER' 'Other'))
$script:scopeAsks = @()
$scopeFetch = {
    param($have, $scope)
    $script:scopeAsks += ('{0}:{1}' -f $have, (@($scope) -join '+'))
    if (@($scope) -contains 'MINE') { @($mineRows | Select-Object -Skip $have -First 2) }
    else { @((@($mineRows) + @($otherRows)) | Select-Object -Skip $have -First 2) }
}
$scopedSel = Invoke-SessionPicker -Sessions @($mineRows[0], $mineRows[1]) -FetchMore $scopeFetch -ProjectSlug @('MINE') -ProjectName 'Mine' `
             -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow', 'DownArrow', 'Enter')) -Draw {}
Assert-Equal '2:MINE' ($script:scopeAsks -join ',') 'a scoped picker pages with its own slug, so the rows it fetches can actually be shown'
Assert-Equal 'm3' $scopedSel.Session.SessionId 'and the page it fetched is reachable with one more Down'
$script:scopeAsks = @()
$tabSel = Invoke-SessionPicker -Sessions @($mineRows[0]) -FetchMore $scopeFetch -ProjectSlug @('MINE') -ProjectName 'Mine' `
          -ReadKey (New-ScriptedKeyReader -Keys @('Tab', 'DownArrow', 'Enter')) -Draw {}
Assert-Equal '0:' ($script:scopeAsks -join ',') 'Tab widens to the whole account by asking the same fetcher for page 1 with NO scope'
Assert-Equal 'm2' $tabSel.Session.SessionId 'and the widened page is what the cursor then moves through'

# One real directory, two slug folders: a picker scoped to the merged project reaches both.
$twoSlug = @((& $pgRow 'a1' 'C--tmp-Shared' 'Shared'), (& $pgRow 'b1' 'C--tmp-Shared-alt' 'Shared'))
$twoMap = $null
$twoFrame = @(Get-PickerFrame -Sessions $twoSlug -Index 0 -Width 100 -Height 30 -RowMap ([ref]$twoMap))
Assert-Equal 2 $twoMap.RowCount 'both slug folders of one directory are rows'
$twoSel = Invoke-SessionPicker -Sessions $twoSlug -ProjectSlug @('C--tmp-Shared', 'C--tmp-Shared-alt') -ProjectName 'Shared' `
          -ReadKey (New-ScriptedKeyReader -Keys @('DownArrow', 'Enter')) -Draw {}
Assert-Equal 'b1' $twoSel.Session.SessionId 'a picker scoped to a merged project reaches the sibling slug''s sessions too'

# --- END TO END: the launcher's own wiring against a real projects tree ------------------------------
# Not a source regex. The guarantee "the picker opens on the project the owner just chose" was pinned
# only as a pattern over the launcher's first-page LINE, and a [string] parameter three files away
# emptied the snapshot with nothing going red: -Files came back unbound and Get-ClaudeSessions fell
# through to the whole unscoped account (re-review 2026-09-16, C1). This drives the real functions.
$e2eRoot = Join-Path $env:TEMP ('claude-auto-e2e-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$e2eBase = Get-Date '2026-09-01 12:00:00'
$e2eWrite = {
    param([string]$Slug, [string]$Name, [int]$Minute, [string]$Cwd)
    $d = Join-Path $e2eRoot $Slug
    New-Item -ItemType Directory -Force -Path $d | Out-Null
    $p = Join-Path $d $Name
    [IO.File]::WriteAllText($p, '{"type":"user","cwd":' + (ConvertTo-Json $Cwd) + ',"message":{"role":"user","content":"prompt ' + $Name + '"}}' + "`n", (New-Object System.Text.UTF8Encoding($false)))
    (Get-Item -LiteralPath $p).LastWriteTime = $e2eBase.AddMinutes($Minute)
}
# One real directory under TWO slug folders, 4 sessions between them; 12 NEWER foreign sessions, so
# an unscoped page of ten holds none of the chosen project's; and one slug folder with nothing in it.
foreach ($n in 1..2) { & $e2eWrite 'C--src-mine' "a$n.jsonl" $n 'C:\src\mine' }
foreach ($n in 1..2) { & $e2eWrite 'C--src-mine-alt' "b$n.jsonl" ($n + 2) 'C:/src/mine' }
foreach ($n in 1..12) { & $e2eWrite 'C--src-busy' "z$n.jsonl" (100 + $n) 'C:\src\busy' }
New-Item -ItemType Directory -Force -Path (Join-Path $e2eRoot 'C--src-empty') | Out-Null

$e2eCache = Join-Path $e2eRoot 'sessions.json'
$e2ePageSize = 10
$e2eSnapshots = @{}
# The launcher's fetcher, in the shape claude-auto.ps1 builds it.
$e2eFetch = {
    param($have, [string[]]$ProjectSlug)
    $k = (@($ProjectSlug | Where-Object { $_ }) -join '|')
    if (-not $e2eSnapshots.ContainsKey($k)) {
        $e2eSnapshots[$k] = [string[]](Get-ClaudeSessionFile -ProjectsRoot $e2eRoot -ProjectSlug $ProjectSlug)
    }
    $snapshot = [string[]]$e2eSnapshots[$k]
    @(Get-ClaudeSessions -ProjectsRoot $e2eRoot -CachePath $e2eCache -Limit $e2ePageSize -Skip $have -Files $snapshot)
}.GetNewClosure()

$e2eMine = @('C--src-mine', 'C--src-mine-alt')
$e2ePage1 = @(& $e2eFetch 0 $e2eMine)
Assert-Equal 4 $e2ePage1.Count 'the first page of a MERGED two-slug project is that project''s sessions'
Assert-Equal 4 @($e2ePage1 | Where-Object { $_.Slug -in $e2eMine }).Count 'and every row on it is in scope, although 12 newer foreign sessions exist'
Assert-Equal 2 @($e2ePage1 | Where-Object { $_.Slug -eq 'C--src-mine-alt' }).Count 'including the sibling slug''s sessions, which a single-slug scope would have missed'
$script:e2eDrawn = -1
$e2eDraw = { param($s, $i, $f, $sc, $pn) if ($script:e2eDrawn -lt 0) { $script:e2eDrawn = @($s).Count }; $null }
$null = Invoke-SessionPicker -Sessions $e2ePage1 -FetchMore $e2eFetch -ProjectSlug $e2eMine -ProjectName 'mine' `
        -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw $e2eDraw
Assert-Equal 4 $script:e2eDrawn 'and the picker''s FIRST FRAME is handed those four rows, not an empty list'
Assert-Equal 0 @(& $e2eFetch 4 $e2eMine).Count 'paging past the end of a scope returns nothing rather than leaking the account'
# A scope that really holds no transcripts: an empty picker, never the whole account.
Assert-Equal 0 @(& $e2eFetch 0 @('C--src-empty')).Count 'a slug with no sessions yields an EMPTY page, not the unscoped listing'
# Tab widens: no scope at all, and the account's newest ten come back.
$e2eAll = @(& $e2eFetch 0 @())
Assert-Equal 10 $e2eAll.Count 'Tab widens to the account and gets a full page'
Assert-Equal 10 @($e2eAll | Where-Object { $_.Slug -eq 'C--src-busy' }).Count 'of the newest sessions, whatever project they belong to'
Remove-Item -LiteralPath $e2eRoot -Recurse -Force -ErrorAction SilentlyContinue

# Paging is disabled only when the filter can see NOTHING - not merely when it hides something.
$w1Rows = @((& $pgRow 'w1a'), (& $pgRow 'w1b'))
$w1Rows[0].Title = 'alpha one'
$w1Rows[1].Title = 'beta two'
$script:w1Asked = 0
$w1Fetch = { param($have, $scope) $script:w1Asked++; @((& $pgRow "w1n$have")) }
$null = Invoke-SessionPicker -Sessions $w1Rows -FetchMore $w1Fetch -Draw {} `
        -ReadKey (New-ScriptedKeyReader -Keys @('/', 'a', 'l', 'p', 'h', 'a', 'Enter', 'DownArrow', 'DownArrow', 'Escape'))
Assert-True ($script:w1Asked -gt 0) 'a filter that hides one of two loaded rows still pages - a session matching it one page deeper must be reachable'

# A fetcher that fails because the WORLD changed ends the paging; one that fails because it is
# MISWIRED must not be swallowed. Get-ClaudeSessions throws an ArgumentException for -ProjectSlug
# beside -Files, and a catch-all here would turn that into "this scope has no more rows" - the same
# silence a scoped picker opening on the whole account hid behind (re-review 2, N2).
$n2Row = & $pgRow 'n2a'
$n2Arg = $false
try { $null = Expand-SessionPage -Sessions @($n2Row) -FetchMore { param($h, $s) throw [ArgumentException]::new('-ProjectSlug cannot be combined with -Files') } -Fetched 1 }
catch [System.ArgumentException] { $n2Arg = $true }
catch { }
Assert-True $n2Arg 'a MISWIRED fetcher (ArgumentException) propagates instead of reading as the end of the list'
# A binding failure, raised without touching the filesystem: handing a [int] parameter a word.
# NOT `Get-ClaudeSessions -NoSuchParameter` - that function has no [CmdletBinding()], so an unknown
# parameter lands in $args and the call RUNS, against the real projects root and its shared cache.
$n2Bind = $false
try { $null = Expand-SessionPage -Sessions @($n2Row) -FetchMore { param($h, $s) & { param([int]$X) $X } -X 'not-an-int' } -Fetched 1 }
catch { $n2Bind = ($_.Exception -is [System.Management.Automation.ParameterBindingException]) }
Assert-True $n2Bind 'and so does a parameter-binding failure'
$n2Io = Expand-SessionPage -Sessions @($n2Row) -FetchMore { param($h, $s) throw [IO.IOException]::new('the projects root vanished') } -Fetched 1
Assert-Equal 0 $n2Io.Added 'an IO failure still ends the paging quietly'
Assert-Equal $true $n2Io.Exhausted 'and marks the list exhausted rather than taking the picker down'

# The same IO failure must also reach the launcher log - the morning crash this exists for left
# nothing there but start/ui/decision/exit. Write-LauncherLog is stubbed at the top of this file
# (Env.ps1's own definition never runs here), so this reads what Expand-SessionPage recorded
# instead of writing to the real log file.
$script:loggedCalls = @()
$null = Expand-SessionPage -Sessions @($n2Row) -FetchMore { param($h, $s) throw [IO.IOException]::new('the projects root vanished') } -Fetched 1
$n2ErrLogs = @($script:loggedCalls | Where-Object { $_.Stage -eq 'error' })
Assert-Equal 1 $n2ErrLogs.Count 'and the IO failure logs exactly one error record'
Assert-Equal 'Expand-SessionPage' $n2ErrLogs[0].Data.where 'tagged with where it happened'

# Preview must stay side-effect-free (the same fact that made claude-auto.ps1's own UI catch guard
# on -not $Preview): a preview run can hit this same IOException reading real session files, and
# must never write a real record to the owner's launcher log. $script:Preview is the launcher's own
# script-scope variable, same as $script:RunId above - set and restored around this one probe only.
$script:Preview = $true
$script:loggedCalls = @()
$null = Expand-SessionPage -Sessions @($n2Row) -FetchMore { param($h, $s) throw [IO.IOException]::new('the projects root vanished') } -Fetched 1
Assert-Equal 0 $script:loggedCalls.Count 'and a preview run logs nothing for the same IO failure'
$script:Preview = $false

# --- UI-stage logging: the screens say what happened, never what was typed ------------------------
# The 07:17 crash run had a `ui` record and nothing else: no screen the owner saw, no key that
# decided anything. Write-UiLog (Ui.ps1) is the ONE helper every screen writes through. Tests inject
# $script:UiLogSink, so nothing here opens the real log directory - and the sink is read BEFORE the
# $Preview guard on purpose, which is what lets a preview run be read the same way.
$script:uiRecords = @()
$uiSink = { param($s, $d) $script:uiRecords += [pscustomobject]@{ Stage = $s; Data = $d } }
# stage:screen:phase for a screen record, stage:screen:key for a key one - the ORDER of these is the
# assertion, because "what happened" is a sequence and a set would pass on a scrambled one.
$uiTrace = {
    @($script:uiRecords | ForEach-Object {
        if ($_.Stage -eq 'screen') { "screen:$($_.Data.name):$($_.Data.phase)" }
        else { "$($_.Stage):$($_.Data.screen):$($_.Data.key)" }
    })
}
$uiLogRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("pp-uilog-$([Guid]::NewGuid().ToString('N'))")
$uiLogProj = Join-Path $uiLogRoot 'gamma'
New-Item -ItemType Directory -Path $uiLogProj -Force | Out-Null
try {
    $uiProjs = @([pscustomobject]@{ Slug = 'G'; Path = $uiLogProj; Name = 'gamma'; Worktree = $null; LastActivity = (Get-Date) })
    $uiFake = @(
        [pscustomobject]@{ SessionId = 'cccc3333'; Project = 'gamma'; Slug = 'G'; Worktree = $null; Modified = (Get-Date '2026-09-16 09:00'); SizeBytes = 4096; PromptCount = 4; Title = 'x'; LastUser = 'u'; LastAssistant = 'a' }
    )

    $script:UiLogSink = $uiSink
    $script:uiRecords = @()
    $null = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('Enter')) -Draw {}
    # LeftArrow before the hotkey, and Tab on the picker: the two decisive keys that change what the
    # screen MEANS without leaving it (the action Enter will run, and whether the list is this
    # project or the whole account). Both were in the product and in no test - each call could be
    # deleted with every suite still green (review W1).
    $null = Invoke-ProjectScreen -Projects $uiProjs -Cwd $uiLogProj -ReadKey (New-ScriptedKeyReader -Keys @('LeftArrow', 'r')) -Draw {}
    # Scoped, exactly as claude-auto.ps1 opens it after the project screen - an unscoped picker
    # reports scope 'none', and Tab is guarded on having a scope at all, so both assertions below
    # would pass on the wrong thing.
    $null = Invoke-SessionPicker -Sessions $uiFake -ProjectSlug @('G') -ProjectName 'gamma' -ReadKey (New-ScriptedKeyReader -Keys @('Tab', 'Escape')) -Draw {}
    Assert-Equal ('screen:launch:enter,key:launch:Enter,screen:launch:leave,' +
                  'screen:project:enter,key:project:LeftArrow,key:project:r,screen:project:leave,' +
                  'screen:picker:enter,key:picker:Tab,key:picker:Escape,screen:picker:leave') ((& $uiTrace) -join ',') 'a scripted launch -> project -> picker -> Escape run logs exactly these records, in this order'

    # The PAYLOAD, not just the key name: a record saying "LeftArrow" without what it left the field
    # on answers nothing, and 'worktree' is what Step-ProjectAction gives stepping back from 'new'.
    $uiLeft = @($script:uiRecords | Where-Object { $_.Stage -eq 'key' -and $_.Data.key -eq 'LeftArrow' })[0]
    Assert-Equal 'worktree' $uiLeft.Data.action 'the action field step records the value it landed on'
    $uiTab = @($script:uiRecords | Where-Object { $_.Stage -eq 'key' -and $_.Data.key -eq 'Tab' })[0]
    Assert-Equal 'all' $uiTab.Data.scope 'Tab records the scope it widened TO'
    $uiPickLeave = @($script:uiRecords | Where-Object { $_.Stage -eq 'screen' -and $_.Data.name -eq 'picker' -and $_.Data.phase -eq 'leave' })[0]
    Assert-Equal 'all' $uiPickLeave.Data.scope 'and the picker leaves in that scope, not the one it opened in'

    $uiEnter = @($script:uiRecords | Where-Object { $_.Stage -eq 'screen' -and $_.Data.name -eq 'project' -and $_.Data.phase -eq 'enter' })[0]
    Assert-Equal 3 $uiEnter.Data.rows 'a screen record carries the row COUNT (one project, the cwd row, the free-path row)'
    Assert-Equal 0 $uiEnter.Data.index 'and where the cursor was'
    $uiLeave = @($script:uiRecords | Where-Object { $_.Stage -eq 'screen' -and $_.Data.name -eq 'project' -and $_.Data.phase -eq 'leave' })[0]
    Assert-True ($uiLeave.Data.ContainsKey('ms')) 'a leave record carries how long the screen was up'
    Assert-True ([int]$uiLeave.Data.ms -ge 0) 'as a non-negative number of milliseconds'
    $uiPick = @($script:uiRecords | Where-Object { $_.Stage -eq 'screen' -and $_.Data.name -eq 'picker' })[0]
    Assert-Equal 'project' $uiPick.Data.scope 'the picker records the scope it opened in'

    # DECISIVE keys only. Typing into the filter must leave ONE record for opening it and one for
    # closing it - never one per character, or the log becomes a keylogger and the file open per
    # keystroke the brief forbids.
    $script:uiRecords = @()
    $sentinel = 'zqxvw42'
    $sentinelKeys = @('/') + @($sentinel.ToCharArray() | ForEach-Object { "$_" }) + @('Escape', 'Escape')
    $null = Invoke-ProjectScreen -Projects $uiProjs -Cwd $uiLogProj -ReadKey (New-ScriptedKeyReader -Keys $sentinelKeys) -Draw {}
    $uiKeys = @($script:uiRecords | Where-Object { $_.Stage -eq 'key' })
    Assert-Equal 3 $uiKeys.Count "$($sentinel.Length) filter characters produce no key records at all - only / open, Escape close, Escape leave"
    Assert-Equal '/,Escape,Escape' (($uiKeys | ForEach-Object { $_.Data.key }) -join ',') 'and those three are the decisive ones'
    $uiJson = ($script:uiRecords | ConvertTo-Json -Depth 8 -Compress)
    Assert-True ($uiJson -notmatch $sentinel) 'the filter TEXT never appears in any record'
    $uiClose = @($uiKeys | Where-Object { $_.Data.filter -eq 'clear' })[0]
    Assert-Equal $sentinel.Length $uiClose.Data.filterLength 'only its LENGTH does'

    # The free path is the owner's own directory name - the one string on these screens that is
    # nobody's business but his. Read-ClaudeFreePath is driven directly here: the project screen's
    # -ReadPath is injected by every other test, so this is the only place the real prompt runs.
    $script:uiRecords = @()
    $pathSentinel = 'B:\qqzz-never-logged-9182'
    $fpOk = Read-ClaudeFreePath -MouseState $null -GetSize { @(80, 24) } -GetWindowTop { 0 } -Write { param($t) } -SetCursor { param($x, $y) } -ReadLine { $pathSentinel }
    Assert-Equal $pathSentinel $fpOk.Line 'the free-path prompt still returns what was typed'
    Assert-Equal 'screen:freepath:enter,screen:freepath:leave' ((& $uiTrace) -join ',') 'and logs entering and leaving the prompt'
    $fpJson = ($script:uiRecords | ConvertTo-Json -Depth 8 -Compress)
    Assert-True ($fpJson -notmatch 'qqzz-never-logged') 'the typed path never reaches the log'
    $fpLeave = @($script:uiRecords | Where-Object { $_.Data.phase -eq 'leave' })[0]
    Assert-Equal $pathSentinel.Length $fpLeave.Data.length 'only how many characters it was'

    # A prompt that THROWS returns an empty line and says nothing - that silence is what item 3 is
    # about. One error record, and the typed-path rule holds there too.
    $script:uiRecords = @()
    $fpBad = Read-ClaudeFreePath -MouseState $null -GetSize { @(80, 24) } -GetWindowTop { 0 } -Write { param($t) } -SetCursor { param($x, $y) } -ReadLine { throw [IO.IOException]::new('the console handle is invalid') }
    Assert-Equal '' $fpBad.Line 'a free-path read that throws still returns an empty line'
    $fpErr = @($script:uiRecords | Where-Object { $_.Stage -eq 'error' })
    Assert-Equal 1 $fpErr.Count 'and logs exactly one error record instead of swallowing it'
    Assert-Equal 'Read-ClaudeFreePath' $fpErr[0].Data.where 'tagged with where it happened'
    Assert-Equal 'IOException' $fpErr[0].Data.type 'and with the exception type'

    # The sink is a TEST seam. Without it the records must reach Write-LauncherLog for real, or every
    # assertion above would pass over a helper that writes nothing anywhere (Write-LauncherLog is
    # stubbed at the top of this file, so this reads the call rather than the owner's log file).
    $script:UiLogSink = $null
    $script:loggedCalls = @()
    $null = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw {}
    $uiReal = @($script:loggedCalls | Where-Object { $_.Stage -eq 'screen' -or $_.Stage -eq 'key' })
    Assert-Equal 3 $uiReal.Count 'with no sink, the screen writes its records through Write-LauncherLog itself'

    # Fail-open is the whole design, and no suite reached it: every suite that loads Ui.ps1 either
    # has a logger or never calls Write-UiLog, so patching the helper to throw left all of them green
    # (review W2). Both halves are driven here - no logger at all, and a logger that throws.
    # Function:\ is the same table Env.ps1's definition and this file's stub share, so removing it
    # leaves NOTHING to resolve; the cached writer is reset because the miss is cached on purpose.
    $uiStub = (Get-Command Write-LauncherLog -CommandType Function).ScriptBlock
    Remove-Item Function:Write-LauncherLog
    $script:UiLogWriter = $null
    # Get-Command is shadowed for the length of this ONE drive: the miss has to be cached, or a
    # screen with no logger pays a command lookup per record - the per-keystroke cost the whole
    # design is built to avoid. The Escape run below writes three records.
    $script:getCommandCalls = 0
    function Get-Command { $script:getCommandCalls++; Microsoft.PowerShell.Core\Get-Command @args }
    $uiNoLoggerThrew = $false
    try { $null = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw {} }
    catch { $uiNoLoggerThrew = $true }
    Remove-Item Function:Get-Command
    Assert-Equal $false $uiNoLoggerThrew 'with no logger in scope at all, a screen still runs - the miss is a no-op, never a crash'
    Assert-Equal 1 $script:getCommandCalls 'and the miss is resolved ONCE for three records, never looked up per record'

    # A logger that THROWS (a log volume that went away mid-launch) must not reach the screen loop:
    # that is what the helper's own catch is for, and it is the one guard a menu cannot afford to lose.
    function Write-LauncherLog { param([string]$Stage, [hashtable]$Data = @{}, [string]$RunId, [string]$Root) throw [IO.IOException]::new('the log volume went away') }
    $script:UiLogWriter = $null
    $uiThrowingThrew = $false
    try { $null = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw {} }
    catch { $uiThrowingThrew = $true }
    Assert-Equal $false $uiThrowingThrew 'and a logger that THROWS is swallowed rather than taking the menu down'

    # Positive control on the restore: without it the two cases above would silently disarm every
    # assertion after them (they would all run with no logger and count nothing).
    Set-Item Function:Write-LauncherLog $uiStub
    $script:UiLogWriter = $null
    $script:loggedCalls = @()
    $null = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw {}
    Assert-Equal 3 $script:loggedCalls.Count "and the suite's own stub is back in place afterwards"

    # Preview must stay side-effect-free, exactly like the launcher's own UI catch and
    # Expand-SessionPage's - a preview run drives these very loops.
    $script:Preview = $true
    $script:loggedCalls = @()
    $null = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-ScriptedKeyReader -Keys @('Escape')) -Draw {}
    Assert-Equal 0 $script:loggedCalls.Count 'and a preview run writes none of them'
    $script:Preview = $false
} finally {
    $script:UiLogSink = $null
    Remove-Item -LiteralPath $uiLogRoot -Recurse -Force -ErrorAction SilentlyContinue
}

# The launcher's own `ui` record (preselect / chosen / the module-load error) is pinned in
# Test-Maintenance.ps1, beside the other claude-auto.ps1 AST pins - this file drives the screens
# (review W5).

Remove-Item Env:CLAUDE_AUTO_CONFIG -ErrorAction SilentlyContinue
if ($script:Ran -ne 1049) { Write-Host "COULD NOT RUN: expected 1049 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
