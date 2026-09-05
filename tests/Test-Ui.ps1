# Assertions for Ui.ps1. Run: pwsh -File Test-Ui.ps1
# Every screen is driven through the injectable key reader, so none of this needs a terminal.
$env:CLAUDE_AUTO_CONFIG = "$PSScriptRoot\fixtures\config-four.json"   # BEFORE the dot-sources
try {
    . "$PSScriptRoot\..\claude-auto\Theme.ps1"
    . "$PSScriptRoot\..\claude-auto\Layout.ps1"
    . "$PSScriptRoot\..\claude-auto\Sessions.ps1"   # Get-PickerFrame calls Format-RelativeAge at render time
    . "$PSScriptRoot\..\claude-auto\Screens.ps1"
    . "$PSScriptRoot\..\claude-auto\Prefs.ps1"   # Invoke-LaunchScreen calls Switch-LaunchAccount / Reset-LaunchTab
    . "$PSScriptRoot\..\claude-auto\Input.ps1"   # Invoke-SessionPicker maps a click through Get-ClaudeMouseRow
    . "$PSScriptRoot\..\claude-auto\Ui.ps1"
    . "$PSScriptRoot\..\claude-auto\Maintenance.ps1"   # Invoke-MaintenanceScreen calls Get-ClaudeInstallInfo internally
    . "$PSScriptRoot\..\claude-auto\Env.ps1"   # Get-FriendlyModelName / Get-DefaultModelLabel - the model-row label shortener
    Set-LaunchRoster -Accounts (Read-LauncherConfig).Accounts -Remote
} catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }

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
Assert-Equal 'Account,Remote,Action,Model,Effort,Advisor,Permission,Mode' (((Get-LaunchRows) | ForEach-Object { $_.Name }) -join ',') 'the row order is account, remote, action, model, effort, advisor, permission, mode'

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

# Colour identity for the picker, at both layouts.
foreach ($w in @(80, 120)) {
    $plainP   = Get-PickerFrame -Sessions $fake -Index 0 -Filter '' -Width $w -Height 24 -Now $now
    $coloredP = Get-PickerFrame -Sessions $fake -Index 0 -Filter '' -Width $w -Height 24 -Now $now -Color
    $mismatch = 0
    for ($i = 0; $i -lt $plainP.Count; $i++) {
        if ((Remove-AnsiColor -Text $coloredP[$i]) -ne $plainP[$i]) { $mismatch++ }
    }
    Assert-Equal 0 $mismatch "stripping colour returns the plain picker frame exactly at width $w"
}

# --- colour ------------------------------------------------------------------------------
# The whole point of painting after layout: stripping the escapes must give back exactly the
# plain frame. If this ever fails, colour has started corrupting the width arithmetic.

$plain   = Get-LaunchFrame -State (New-LaunchState) -Width 84 -Height 24 -Limits @{ work = [pscustomobject]@{ FiveHour = 15; SevenDay = 97; AgeText = 'just now' } }
$colored = Get-LaunchFrame -State (New-LaunchState) -Width 84 -Height 24 -Limits @{ work = [pscustomobject]@{ FiveHour = 15; SevenDay = 97; AgeText = 'just now' } } -Color
Assert-Equal $plain.Count $colored.Count 'colouring does not change the number of lines'
$mismatch = 0
for ($i = 0; $i -lt $plain.Count; $i++) {
    if ((Remove-AnsiColor -Text $colored[$i]) -ne $plain[$i]) { $mismatch++ }
}
Assert-Equal 0 $mismatch 'stripping colour returns the plain launch frame exactly'
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

# --- Speaker colour in the preview (owner ask 2026-08-15: "не видно чьё сообщение ... добавь
# цвета"). The words were already there; what was missing was colour. These pin BOTH halves: the
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
$actionIdx = [Array]::FindIndex($rowsDef, [Predicate[object]]{ param($r) $r.Name -eq 'Action' })
$actionRow = $lmap.Rows[$actionIdx]
$resumeCell = @($actionRow.Cells | Where-Object { $_.Value -eq 'resume' })[0]
Assert-Equal $true ($null -ne $resumeCell) 'the Action row exposes a clickable cell for every option'

$ldraw = { param($s) $lmap }.GetNewClosure()
$esc = [System.ConsoleKeyInfo]::new([char]0, [System.ConsoleKey]::Escape, $false, $false, $false)
$enterKey = [System.ConsoleKeyInfo]::new([char]0, [System.ConsoleKey]::Enter, $false, $false, $false)

# A click on the row selects it, and nothing else changes.
$w = New-EventReader @((New-MouseEvent -Y $actionRow.Y -X 1 -Left), $enterKey)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal $actionIdx $out.Row 'a click on a row selects that row'
Assert-Equal 'new' $out.Action 'and, away from the option cells, leaves the value alone'

# A click ON the 'resume' cell selects the row AND the value.
$w = New-EventReader @((New-MouseEvent -Y $actionRow.Y -X $resumeCell.Start -Left), $enterKey)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 'resume' $out.Action 'a click on an option cell selects that option'
Assert-Equal $actionIdx $out.Row 'and selects its row too'
# The far edge of the span must hit as well - an off-by-one there makes the last option unclickable.
$w = New-EventReader @((New-MouseEvent -Y $actionRow.Y -X $resumeCell.End -Left), $enterKey)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal 'resume' $out.Action 'the last column of an option cell still hits it'
# One column past it must NOT.
$w = New-EventReader @((New-MouseEvent -Y $actionRow.Y -X ($resumeCell.End + 1) -Left), $enterKey)
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey $w -Draw $ldraw -Wait $w -GetWindowTop { 0 }
Assert-Equal $false ($out.Action -eq 'resume') 'one column past the cell does not select it'

# Nothing a mouse does may START a session: only Enter returns the state.
$w = New-EventReader @((New-MouseEvent -Y $actionRow.Y -X $resumeCell.Start -Left -Double), $esc)
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

# Action and Mode are deliberately outside the reset: they are not habits, they already start at
# their defaults, and clearing them would undo a choice made for THIS launch. A behaviour change
# from the old ctrl+r, which flattened every row.
$actionIdx2 = Get-RowIndex -Name 'Action'
$keys = (@('DownArrow') * $actionIdx2) + @('RightArrow') + (@('UpArrow') * $actionIdx2) + @($CtrlR, 'Enter')
$out = Invoke-LaunchScreen -State (New-LaunchState) -ReadKey (New-MixedKeyReader -Keys $keys) -Draw {}
Assert-Equal 'continue' $out.Action 'ctrl+r leaves the action alone - it describes this launch, not a habit'

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
Assert-Equal $true ($paintedFooter.Contains([string][char]27 + '[96m')) 'the key words are tinted'
Assert-Equal $plainFooter.Text (Add-HintColor -Line $plainFooter.Text -Spans $plainFooter.Spans) 'colour disabled leaves the footer exactly as it was'

# --- Maintenance screen. It has no rows to select, so the mouse does exactly one thing there. The
# assertion works by EVENT BUDGET: the reader holds a single event, so a click that is honoured
# leaves on the first pass, and one that is ignored asks for a second event and throws. That is what
# makes both directions falsifiable without inspecting internal state. ---
# Any EXISTING file: the fake -Runner never executes it, but the existence check runs regardless of the runner.
$cfgActions = @([pscustomobject]@{ Key = 'i'; Label = 'cbm reindex'; Script = (Join-Path $PSScriptRoot 'fixtures\config-four.json'); ConfirmTwice = $true })
$fakeInfo = [pscustomobject]@{ Matches = $true; NewestVersion = '2.1.233'; InstalledHash = 'A'; NewestHash = 'A'; VersionCount = 3; BinPath = 'x' }
$mmap = $null
$null = Get-MaintenanceFrame -Info $fakeInfo -Width 100 -Height 24 -RowMap ([ref]$mmap) -Actions $cfgActions
$mDraw = { param($i, $s) $mmap }.GetNewClosure()
# Asserting the SET rather than a count: a count says nothing about which hint went missing, and
# adding one should not make an unrelated assertion fail with a number.
$mChars = @($mmap.Footer | ForEach-Object { if ($_.Char) { $_.Char } else { $_.Key } }) -join ','
Assert-Equal 'u,r,d,m,p,i,Escape' $mChars 'every maintenance action is clickable, reindex included'

$escSpan = Get-HintSpan -Map $mmap -Key 'Escape'
$w = New-EventReader @((New-MouseEvent -Y $mmap.FooterY -X $escSpan.Start -Left))
$threw = $false
try { Invoke-MaintenanceScreen -ReadKey $w -Draw $mDraw -Wait $w -GetWindowTop { 0 } } catch { $threw = $true }
Assert-Equal $false $threw 'clicking "esc back" leaves the maintenance screen on the first event'

# The same click one column past the span must NOT leave - proving the hit test is what decided it,
# not the mere arrival of a mouse event.
$w = New-EventReader @((New-MouseEvent -Y $mmap.FooterY -X ($escSpan.End + 1) -Left))
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

# A click on "i cbm reindex" must behave identically to the key - it becomes that key.
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
$missing = Invoke-MaintenanceScript -ScriptPath 'C:\nope\does-not-exist.ps1' -Label 'cbm reindex'
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
Assert-Equal $null $picked 'an uppercase F does not fork a session - picker key matching is case-sensitive'

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
$narrowInfo = [pscustomobject]@{ Matches = $false; NewestVersion = '2.1.240'; InstalledHash = ('a' * 64); NewestHash = ('b' * 64); VersionCount = 3; VersionsBytes = 900000000; BinPath = (Join-Path $HOME '.local\bin\claude.exe') }
# With the model bucket: three bars is what the minimum height is measured against, so the block
# that asserts the minimum has to render the case that produced it.
$narrowLimits = @{ work = [pscustomobject]@{ FiveHour = 41; SevenDay = 63; AgeText = '12 min ago'; Model = 15; ModelLabel = 'FABLE' } }
$longStatus = (1..40 | ForEach-Object { "status line $_ with some words in it" }) -join "`n"

# MinHeight is MEASURED, not chosen: the worst 50-column launch frame is rendered at a height the
# guard cannot refuse, its lines are counted, and the constant must be that count plus the headroom
# row Write-Frame needs. Written this way the number re-measures itself on every run - a literal
# would go stale the first time a row or a bar is added, which is exactly how the old 16 survived
# being one short. Measured 2026-09-04: 20 lines (3 box + blank + 8 rows + 3 bars + blank +
# separator + restored + 2 wrapped footer lines) -> 21.
$worstCase = @(Get-LaunchFrame -State (New-LaunchState) -Width 50 -Height 200 -Limits $narrowLimits `
    -Restored @('Model') -RestoredAge '12 min' -DefaultModelLabel 'default (Fable 5.1[1M])' -DefaultAdvisorLabel 'default (fable)')
Assert-Equal 20 $worstCase.Count 'the worst 50-column frame is 20 lines'
Assert-Equal ($worstCase.Count + 1) $script:MinHeight 'MinHeight is that count plus the headroom row'

foreach ($h in @($script:MinHeight, 50)) {
    $nmap = $null
    $lf = @(Get-LaunchFrame -State (New-LaunchState) -Width 50 -Height $h -Limits $narrowLimits -Restored @('Model') -RestoredAge '12 min' -RowMap ([ref]$nmap))
    $lfText = $lf -join "`n"
    foreach ($hint in @('enter start', 'u maintenance', 'esc quit', 'up/down row', 'left/right value')) {
        Assert-Equal $true $lfText.Contains($hint) "50x${h} launch: the hint '$hint' is readable"
    }
    Assert-Equal $true ($lf.Count -le ($h - 1)) "50x${h} launch: $($lf.Count) lines leave the headroom row"
    Assert-Equal $true ($lfText.Contains('ctrl+r resets')) "50x${h} launch: the restored line keeps its reset advice"
    Assert-Equal 3 @($nmap.Footer).Count "50x${h} launch: all three actions are clickable"

    $pf = @(Get-PickerFrame -Sessions $narrowSessions -Index 2 -Width 50 -Height $h)
    $pfText = $pf -join "`n"
    foreach ($hint in @('/ filter', 'enter open', 'f fork', 'esc back')) {
        Assert-Equal $true $pfText.Contains($hint) "50x${h} picker: the hint '$hint' is readable"
    }
    Assert-Equal $true ($pf.Count -le ($h - 1)) "50x${h} picker: $($pf.Count) lines leave the headroom row"

    $mf = @(Get-MaintenanceFrame -Info $narrowInfo -Width 50 -Height $h -Status $longStatus -Actions $cfgActions)
    $mfText = $mf -join "`n"
    foreach ($hint in @('u update', 'r rename swap', 'd doctor', 'm mcp list', 'p prune', 'i cbm reindex', 'esc back')) {
        Assert-Equal $true $mfText.Contains($hint) "50x${h} maintenance: the hint '$hint' is readable"
    }
    Assert-Equal $true ($mf.Count -le ($h - 1)) "50x${h} maintenance: $($mf.Count) lines with a long status leave the headroom row"
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
Assert-Equal $false (($noActions | ForEach-Object { Remove-AnsiColor $_ }) -join "`n" -match 'cbm reindex') 'no action, no hint'
$withAction = Get-MaintenanceFrame -Info $narrowInfo -Width 78 -Height 24 -Actions $cfgActions
Assert-Equal $true (($withAction | ForEach-Object { Remove-AnsiColor $_ }) -join "`n" -match 'i\s+cbm reindex') 'a configured action is a footer hint'
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

# Without -Width the footer is one line, as every caller that never wraps expects.
$oneLine = New-HintFooter -Glyphs (Get-Glyphs) -Hints @(
    @{ Token = 'u'; Label = 'update'; Clickable = $true; Key = ''; Char = 'u' }
    @{ Token = 'r'; Label = 'rename swap'; Clickable = $true; Key = ''; Char = 'r' }
    @{ Token = 'i'; Label = 'cbm reindex'; Clickable = $true; Key = ''; Char = 'i' })
Assert-Equal 1 @($oneLine.Lines).Count 'no width: one footer line'
$wrapped = New-HintFooter -Glyphs (Get-Glyphs) -Width 20 -Hints @(
    @{ Token = 'u'; Label = 'update'; Clickable = $true; Key = ''; Char = 'u' }
    @{ Token = 'r'; Label = 'rename swap'; Clickable = $true; Key = ''; Char = 'r' }
    @{ Token = 'i'; Label = 'cbm reindex'; Clickable = $true; Key = ''; Char = 'i' })
Assert-Equal 3 @($wrapped.Lines).Count 'width 20: three hints of 8-13 characters take three lines'
Assert-Equal 0 (@($wrapped.Lines | Where-Object { $_.Text.Length -gt 20 }).Count) 'width 20: no footer line exceeds the width'
Assert-Equal '  r rename swap' $wrapped.Lines[1].Text 'each wrapped line is indented like the first'

# --- roster is configurable; the Remote row is optional ------------------------------------------
$rowsBefore = @(Get-LaunchRows)
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
Assert-Equal $rowsBefore.Count @(Get-LaunchRows).Count 'restoring the fixture roster restores the row count'

Remove-Item Env:CLAUDE_AUTO_CONFIG -ErrorAction SilentlyContinue
if ($script:Ran -ne 716) { Write-Host "COULD NOT RUN: expected 716 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
