# Terminal UI for the launcher.
#
# Frame builders are pure and return string[]; only the Invoke-* loops touch input, and even they
# take the key source as a parameter. That split is what makes these screens assertable from an
# agent session, where stdin is redirected and the real UI never draws at all.
#
# No external module: PwshSpectreConsole was rejected because its selection prompt renders
# multi-line rows badly (spectre.console#1577) and throws on '[' in row text (#608) - and row text
# here is arbitrary conversation content. See the design doc for the full argument.
#
# The palette, plain-text layout helpers and the launch-screen frame builders moved out:
# $script:Rows, $script:E, $script:C, Test-ColorSupported, Remove-AnsiColor, Get-PercentColor and
# Limit-Line now live in Theme.ps1 or Layout.ps1; Add-LaunchColor, New-LaunchState, Get-LaunchFrame
# and Step-LaunchValue now live in Screens.ps1. This file keeps the input loops and the session
# picker screen.

# The last row activation, as @{ Y = <the record's own screen Y>; At = <record stamp or $null> },
# $null when none is armed (spec P14). SCRIPT scope on purpose and this is the whole point: an
# Activate whose result ends the screen returns from inside Invoke-ScreenLoop, and the second record
# of that one physical gesture is still sitting in the terminal's queue. It reaches the NEXT screen,
# which has no idea a gesture was in progress, and lands there as an ordinary press on whatever
# occupies the same Y - measured: picking a project opened a session on the picker's row 0 that
# nobody chose. A loop-local guard cannot see across that boundary; this can.
$script:LastActivation = $null

function Test-AltBufferSupported {
    # Redirected output is not a terminal, and preview mode must print linearly so its output can be
    # captured. Everything else is assumed capable and repaired by Exit-AltBuffer's finally.
    if ($env:CLAUDE_AUTO_PREVIEW -eq '1') { return $false }
    try { if ([Console]::IsOutputRedirected -or [Console]::IsInputRedirected) { return $false } } catch { return $false }
    return $true
}

function Enter-AltBuffer {
    # ?1049h switches to the alternate screen buffer (what vim and htop use), ?25l hides the cursor.
    # The point is that the menu leaves NO trace in the scrollback once Claude starts.
    [Console]::Write("$([char]27)[?1049h$([char]27)[?25l")
    # Ctrl+C is read as a normal key instead of tearing the process down, so it can be treated as
    # Escape while the alternate buffer is up - the alternative is the process dying before the
    # `finally` that restores the buffer ever runs, leaving the terminal on the wrong screen with no
    # cursor. A console-less host (tests, redirected stdin) throws here; that failure must be silent.
    #
    # Kept here as well as in Open-ClaudeConsoleInput because the two paths are independent: with
    # CLAUDE_AUTO_NO_MOUSE=1 the console is never armed and this is the only thing standing between
    # Ctrl+C and a terminal left on the alternate screen. Nested set-and-restore is safe as long as
    # each side puts back what IT found, which is what the previous hardcoded $false did not do -
    # a process started with Ctrl+C already treated as input had that setting taken away by a
    # launcher that never set it.
    # Logged, not merely swallowed: a console that refuses this setting is the difference between
    # Ctrl+C being read as a key and the process dying before the finally that restores the buffer.
    # The catch still has to be silent, so the record is the only place that fact can land.
    try { $script:AltBufferPrevTcc = [Console]::TreatControlCAsInput; [Console]::TreatControlCAsInput = $true }
    catch { Write-UiLog -Stage 'error' -Data @{ where = 'Enter-AltBuffer'; type = $_.Exception.GetType().Name; message = $_.Exception.Message.Substring(0, [Math]::Min(300, $_.Exception.Message.Length)) } }
}

function Exit-AltBuffer {
    [Console]::Write("$([char]27)[?25h$([char]27)[?1049l")
    if ($null -ne $script:AltBufferPrevTcc) {
        try { [Console]::TreatControlCAsInput = [bool]$script:AltBufferPrevTcc }
        catch { Write-UiLog -Stage 'error' -Data @{ where = 'Exit-AltBuffer'; type = $_.Exception.GetType().Name; message = $_.Exception.Message.Substring(0, [Math]::Min(300, $_.Exception.Message.Length)) } }
    }
    $script:AltBufferPrevTcc = $null
}

function Write-Frame {
    # Home the cursor, overwrite each line, erase what the line used to be, then erase everything
    # below. No Clear(): clearing is what produced the flicker.
    param([string[]]$Lines)
    $sb = [System.Text.StringBuilder]::new()
    [void]$sb.Append("$([char]27)[H")
    foreach ($l in $Lines) { [void]$sb.Append($l); [void]$sb.Append("$([char]27)[K`n") }
    [void]$sb.Append("$([char]27)[J")
    [Console]::Write($sb.ToString())
}

function Write-UiLog {
    # One UI-stage record - `screen`, `key` or `error`. The single place every screen in this file
    # writes through, so the rules below are stated once instead of at each call site.
    #
    # SILENT and fail-open, exactly like the logger it wraps (Write-LauncherLog, Env.ps1):
    # check-launcher-regression.ps1 compares the launcher's console output against a stored
    # reference, so one Write-Host under here reddens a check that exists for real regressions. A
    # throw would be worse still - it would take down a menu over a log line.
    #
    # NEVER the filter text, never a typed path. These records are kept for two weeks and read back
    # later; what the owner typed into a filter box is not the launcher's to remember. Call sites
    # pass lengths.
    param(
        [Parameter(Mandatory)][string]$Stage,
        [hashtable]$Data = @{}
    )
    try {
        # The sink first, and deliberately NOT under the $Preview guard below: a suite injects one to
        # READ the records without ever opening the log directory, and that is also how a preview run
        # can be read - the run whose whole point is that it writes nothing anywhere.
        if ($script:UiLogSink) { & $script:UiLogSink $Stage $Data; return }
        # Preview must stay side-effect-free - the same guard claude-auto.ps1's UI catch and
        # Expand-SessionPage's IO catch carry, for the same reason: preview drives these very loops.
        if ($script:Preview) { return }
        # Resolved ONCE per process, hit OR miss ($false caches the miss). Test-Input and
        # Test-Maintenance load this file without Env.ps1, so there is no logger at all there and
        # every call must become a no-op rather than a Get-Command per keypress. (Test-Ui DOES load
        # Env.ps1 and then shadows Write-LauncherLog with a counting stub, which is how it reads the
        # records without writing any; it removes the stub in one case to drive this miss.)
        # -CommandType Function so an alias, or a stray Write-LauncherLog.exe on PATH, cannot win.
        if ($null -eq $script:UiLogWriter) {
            $cmd = Get-Command -Name Write-LauncherLog -CommandType Function -ErrorAction SilentlyContinue
            $script:UiLogWriter = if ($cmd) { $cmd } else { $false }
        }
        if (-not $script:UiLogWriter) { return }
        $logArgs = @{ Stage = $Stage; Data = $Data }
        # $RunId and $Preview above are the LAUNCHER's script-scope variables, not this file's: every
        # module here is dot-sourced into claude-auto.ps1's scope, so both resolve at call time and
        # are simply absent (hence skipped) in a suite that loads Ui.ps1 on its own.
        if ($script:RunId) { $logArgs.RunId = $script:RunId }
        $null = & $script:UiLogWriter @logArgs
    } catch { }
}

function Wait-KeyOrResize {
    # .NET raises no resize event on Windows, so the wait is a poll: 60 ms is imperceptible for a
    # keypress and cheap enough to leave running. -GetSize and -KeyAvailable are injected so this
    # is assertable without a console.
    param(
        [Parameter(Mandatory)][scriptblock]$ReadKey,
        [int]$Width, [int]$Height,
        [scriptblock]$GetSize = { @([Console]::WindowWidth, [Console]::WindowHeight) },
        [scriptblock]$KeyAvailable = { [Console]::KeyAvailable },
        [int]$MaxLoops = 0,
        # When the console has been armed for mouse input (Open-ClaudeConsoleInput), events come
        # from THAT queue and never from [Console]. Absent, everything below behaves exactly as it
        # did before, which is what keeps every existing suite meaningful.
        $MouseState = $null
    )
    $loops = 0
    while ($true) {
        $size = & $GetSize
        if ($size[0] -ne $Width -or $size[1] -ne $Height) { return 'resize' }
        if ($MouseState) {
            # [Console]::KeyAvailable must NEVER be called while the mouse is armed: it silently
            # DEQUEUES a mouse record while scanning for a key (measured - buffer 1 unread to 0), so
            # the poll would eat the very events it is standing next to. That is also why this is a
            # replacement rather than an addition.
            $ev = Read-ClaudeInputEvent -State $MouseState -TimeoutMs 60
            if ($null -ne $ev) { return $ev }
            # Idle is the only moment the trace may touch the disk: nothing is waiting on the menu
            # here, so the write cannot land between a keypress and the frame that answers it.
            if ($script:TraceOn) { Flush-ClaudeInputTrace }
            $loops++
            if ($MaxLoops -gt 0 -and $loops -ge $MaxLoops) { return (& $ReadKey) }
            continue
        }
        if (& $KeyAvailable) { return (& $ReadKey) }
        $loops++
        if ($MaxLoops -gt 0 -and $loops -ge $MaxLoops) { return (& $ReadKey) }
        Start-Sleep -Milliseconds 60
    }
}

function New-ScriptedKeyReader {
    # Turns key names into the ConsoleKeyInfo values [Console]::ReadKey would return, so a test
    # drives exactly the same loop a terminal does. A one-character entry is treated as a typed
    # character (that is how '/' and filter text arrive); anything longer is a ConsoleKey name.
    param([Parameter(Mandatory)][string[]]$Keys)
    $queue = [System.Collections.Queue]::new()
    foreach ($k in $Keys) {
        if ($k.Length -eq 1) {
            $queue.Enqueue([System.ConsoleKeyInfo]::new([char]$k, [System.ConsoleKey]0, $false, $false, $false))
        } else {
            $parsed = [System.ConsoleKey]::A
            if (-not [enum]::TryParse([System.ConsoleKey], $k, [ref]$parsed)) {
                throw "unknown key name '$k'"
            }
            $queue.Enqueue([System.ConsoleKeyInfo]::new([char]0, $parsed, $false, $false, $false))
        }
    }
    return { if ($queue.Count -eq 0) { throw 'scripted keys exhausted' }; $queue.Dequeue() }.GetNewClosure()
}

function Switch-LaunchTab {
    # Step-LaunchValue has ALREADY moved the account row's value by the time this runs, so the tab
    # being left has to be handed in separately and put back before Switch-LaunchAccount stashes it
    # - otherwise the current answers would be parked under the account being arrived at, which
    # overwrites exactly the profile the switch was meant to load.
    param($State, [string]$From, $Prefs, $Rows)
    if ($State.Account -eq $From) { return $State }
    $to = $State.Account
    $State.Account = $From
    return (Switch-LaunchAccount -State $State -To $to -Prefs $Prefs -Rows $Rows)
}

function Test-MapHas {
    # Duck-typed shape check for a row map, for Get-HitAt's four branches below: PSObject.Properties
    # on a HASHTABLE enumerates the hashtable's OWN members (Keys, Values, Count...), never its
    # entries, so a hashtable-built row map (a test fixture, or any future caller that does not
    # bother with pscustomobject) used to answer 'none' for every shape check that read it that way.
    # A named function, not a scriptblock built inside Get-HitAt: that function is called once per
    # mouse record (Invoke-ScreenLoop's R19 note a few lines down says why that matters), and a
    # scriptblock literal is a fresh allocation on every one of those calls.
    param($RowMap, [string]$Name)
    if ($RowMap -is [hashtable]) { return $RowMap.ContainsKey($Name) }
    return ($null -ne $RowMap.PSObject.Properties[$Name])
}

function Get-HitAt {
    # Where a click landed, for BOTH row-map shapes: the launch/maintenance map (Rows[] with Cells)
    # and the list map (FirstRowY/RowCount/Start, optional Action row). Footer first, then the
    # action row, then a list row or a launch row and its option cell. Replaces the four inline
    # hit tests the loops carried (the launch screen's own copy never used Get-ClaudeMouseRow).
    param($RowMap, [int]$X, [int]$Y, [int]$WindowTop = 0)
    $none = [pscustomobject]@{ Kind = 'none'; Footer = $null; FooterIndex = -1; Row = $null; Cell = $null; Value = $null }
    if (-not $RowMap) { return $none }
    $hint = Get-ClaudeFooterHit -RowMap $RowMap -X $X -Y $Y -WindowTop $WindowTop
    if ($hint) { return [pscustomobject]@{ Kind = 'footer'; Footer = $hint; FooterIndex = [Array]::IndexOf(@($RowMap.Footer), $hint); Row = $null; Cell = $null; Value = $null } }
    # A distinct name, not $y: PowerShell variable names are case-insensitive, so $y would be the
    # SAME variable as the -Y parameter and silently clobber it - breaking the FirstRowY branch
    # below, which needs the raw, unadjusted $Y (Get-ClaudeMouseRow does its own WindowTop math).
    $rowY = $Y - $WindowTop
    if ((Test-MapHas $RowMap 'Action') -and $RowMap.Action -and $rowY -eq $RowMap.Action.Y) {
        $cell = @($RowMap.Action.Cells | Where-Object { $X -ge $_.Start -and $X -le $_.End })
        return [pscustomobject]@{ Kind = 'action'; Footer = $null; FooterIndex = -1; Row = $null; Cell = $(if ($cell.Count) { $cell[0] } else { $null }); Value = $(if ($cell.Count) { $cell[0].Value } else { $null }) }
    }
    if (Test-MapHas $RowMap 'Rows') {
        $hit = @($RowMap.Rows | Where-Object { $_.Y -eq $rowY })
        if ($hit.Count -eq 0) { return $none }
        $cell = @($hit[0].Cells | Where-Object { $X -ge $_.Start -and $X -le $_.End })
        if ($cell.Count -gt 0) { return [pscustomobject]@{ Kind = 'cell'; Footer = $null; FooterIndex = -1; Row = $hit[0]; Cell = $cell[0]; Value = $cell[0].Value } }
        return [pscustomobject]@{ Kind = 'row'; Footer = $null; FooterIndex = -1; Row = $hit[0]; Cell = $null; Value = $null }
    }
    # RowYs wins wherever it is there (the project screen, whose list is broken up by blank lines):
    # the rows are no longer one per line, so a row index cannot be derived from a y by arithmetic -
    # a click under a separator would land one row too far down the list. A y that is not a row's own
    # is no row at all, which is what makes a click on a gap do nothing. Every other list map carries
    # FirstRowY/RowCount only and falls through to Get-ClaudeMouseRow, untouched.
    if ((Test-MapHas $RowMap 'RowYs') -and $RowMap.RowYs) {
        $at = [Array]::IndexOf([int[]]@($RowMap.RowYs), [int]$rowY)
        if ($at -lt 0) { return $none }
        return [pscustomobject]@{ Kind = 'row'; Footer = $null; FooterIndex = -1; Row = [int]($RowMap.Start + $at); Cell = $null; Value = $null }
    }
    if (Test-MapHas $RowMap 'FirstRowY') {
        $row = Get-ClaudeMouseRow -Y $Y -FirstRowY $RowMap.FirstRowY -RowCount $RowMap.RowCount -WindowTop $WindowTop
        if ($null -eq $row) { return $none }
        return [pscustomobject]@{ Kind = 'row'; Footer = $null; FooterIndex = -1; Row = [int]($RowMap.Start + $row); Cell = $null; Value = $null }
    }
    return $none
}

function Invoke-ScreenLoop {
    # The mechanics every screen shares - draw, wait, resize, the mouse, the arrows, Enter/Escape,
    # hotkeys, the log records - written once. A screen is a handler table around it. Plain
    # scriptblocks throughout: never .GetNewClosure() (the forwarder-shape trap, see claude-auto.ps1).
    #
    # Every local below is named loop*, and that is load-bearing. A handler is a plain scriptblock,
    # so PowerShell resolves ITS variables against the scope that INVOKES it - this function - before
    # the screen that wrote it. A local named $key, $rowsOf, $index or $logKey here would silently
    # answer a handler reaching for the screen's own (measured: a screen helper named $rowsOf is
    # shadowed outright). The PARAMETERS keep their interface names, so the rule for a handler is:
    # never read $Screen/$State/$Draw/$Wait/$GetWindowTop/$Handlers/$Silent/$InputPending/$RecordTime
    # - it gets THIS loop's. A screen that must reach its own painter or state inside a handler keeps
    # it under another name.
    param(
        [Parameter(Mandatory)][string]$Screen,
        [Parameter(Mandatory)][hashtable]$State,
        [Parameter(Mandatory)][scriptblock]$Draw,
        [Parameter(Mandatory)][scriptblock]$Wait,
        [scriptblock]$GetWindowTop = { try { [Console]::WindowTop } catch { 0 } },
        [hashtable]$Handlers = @{},
        # No screen/key records at all - the maintenance screen logs nothing today and keeps it so.
        [switch]$Silent,
        # "Is another input record already waiting" (R19). Injected so the coalescing below is
        # assertable without a console; the default answers $false wherever no console is armed, so
        # every keyboard-only caller and every existing suite behaves exactly as it did.
        [scriptblock]$InputPending = { Test-ClaudeInputPending },
        # When the terminal delivered the record now being handled (P7). Same shape and the same
        # reason as Invoke-MaintenanceScreen's: from the INPUT RECORD, never from this loop's own
        # clock - a redraw between two real presses easily outlasts any threshold worth setting,
        # while the two halves of one gesture arrive milliseconds apart however slow the screen is.
        # $null - a scripted reader, the keyboard-only path - leaves the stamp half of the twin
        # guard below inert rather than swallowing a click the owner really did make twice.
        [scriptblock]$RecordTime = { try { Get-ClaudeInputRecordTime } catch { $null } }
    )
    $loopH = $Handlers
    $loopIsCtrl = { param($loopK) [bool]($loopK.Modifiers -band [System.ConsoleModifiers]::Control) }
    # LogFields rides on EVERY record of the screen (the picker's scope), evaluated once per record.
    $loopFields = { param($s) $loopBag = @{}; if ($loopH.LogFields) { $loopAdd = & $loopH.LogFields $s; if ($loopAdd) { foreach ($loopK in $loopAdd.Keys) { $loopBag[$loopK] = $loopAdd[$loopK] } } }; $loopBag }
    $loopRows = { param($s) if ($loopH.Rows) { [int](& $loopH.Rows $s) } else { 0 } }
    $loopLogKey = {
        param($s, [string]$Key, [hashtable]$Extra = @{})
        if ($Silent) { return }
        $loopData = @{ screen = $Screen; key = $Key; index = [int]$s.Index }
        $loopAdd = & $loopFields $s; foreach ($loopK in $loopAdd.Keys) { $loopData[$loopK] = $loopAdd[$loopK] }
        foreach ($loopK in $Extra.Keys) { $loopData[$loopK] = $Extra[$loopK] }
        $null = Write-UiLog -Stage 'key' -Data $loopData
    }
    # The handler return contract in ONE place: @{ NoLog = $true } writes no record for this key (the
    # picker's Enter and 'f' on an empty list log nothing today), @{ Log = @{ ... } } adds this
    # record's own fields over $Base and over LogFields. Merged key by key, never `+`: adding two
    # hashtables that share a key THROWS, and `button` beside a handler's own Log would do exactly that.
    $loopLogRes = {
        param($s, [string]$Key, $Res, [hashtable]$Base = @{})
        if ($Res -is [hashtable] -and $Res.NoLog) { return }
        $loopBag = @{}
        foreach ($loopK in $Base.Keys) { $loopBag[$loopK] = $Base[$loopK] }
        if ($Res -is [hashtable] -and $Res.Log) { foreach ($loopK in $Res.Log.Keys) { $loopBag[$loopK] = $Res.Log[$loopK] } }
        $null = & $loopLogKey $s $Key $loopBag
    }
    $loopEnteredAt = Get-Date
    $loopLogScreen = {
        param($s, [string]$Phase)
        if ($Silent) { return }
        # ScreenRows only where the cursor bound is not the number to report (the picker: the page it
        # was handed on enter, what survived the filter on leave); otherwise the cursor bound itself.
        $loopCount = if ($loopH.ScreenRows) { [int](& $loopH.ScreenRows $s $Phase) } else { [int](& $loopRows $s) }
        $loopData = @{ name = $Screen; phase = $Phase; rows = $loopCount; index = [int]$s.Index }
        if ($Phase -eq 'leave') { $loopData.ms = [int]((Get-Date) - $loopEnteredAt).TotalMilliseconds }
        $loopAdd = & $loopFields $s; foreach ($loopK in $loopAdd.Keys) { $loopData[$loopK] = $loopAdd[$loopK] }
        # ScreenFields ride on enter/leave only - the project screen's and the picker's filterLength.
        if ($loopH.ScreenFields) { $loopMore = & $loopH.ScreenFields $s; if ($loopMore) { foreach ($loopK in $loopMore.Keys) { $loopData[$loopK] = $loopMore[$loopK] } } }
        $null = Write-UiLog -Stage 'screen' -Data $loopData
    }
    $loopFinish = { param($s, $Res) $null = & $loopLogScreen $s 'leave'; $Res.Result }
    # Constant for the life of the screen, and a scriptblock allocation per keypress is exactly the
    # per-keystroke cost these screens are built to avoid: w/a/s/d are ALIASES, live only when the
    # screen has no hotkey of that letter and something for the arrow to do (C1 - the maintenance
    # screen's 'd' is doctor, and a configured action may sit on any letter).
    $loopAlias = { param([string]$c) (-not ($loopH.Hotkeys -and $loopH.Hotkeys.ContainsKey($c))) -and (Test-ClaudeHotkey -Key $loopKey -Char $c) }
    $loopHasCursor = [bool]$loopH.Rows
    $loopHasLR = [bool]($loopH.Left -or $loopH.Right)

    if ($loopH.Before) { $null = & $loopH.Before $State }
    $null = & $loopLogScreen $State 'enter'
    $loopNeedDraw = $true
    $loopMap = $null
    # Did the click just handled ACT (spec D9), on which row, and when? One physical double click
    # arrives as a plain press AND a record flagged IsDoubleClick over the same spot (Input.ps1), so
    # without this the twin would run the activation a second time wherever the screen did not end.
    # The ROW and the STAMP ride along because the flag alone is not enough (P7): the VT text-mouse
    # protocol has no double-click at all, so ConvertFrom-ClaudeMouseReport sets IsDoubleClick $false
    # always, and in a VT terminal (Rider) the twin arrives as an ordinary second press - measured
    # acts=2 on one gesture. Two records of the same gesture land within a few ms of each other;
    # 500 ms is far under any human's second CLICK and far over any terminal's twin.
    $loopActed = $false
    $loopActedRow = -1
    $loopActedAt = $null
    $loopTwinMs = 500
    # The same question for a FOOTER button, which has no Activate to hang off: when did the press
    # that became this button's key arrive, and over which Y.
    $loopFooterY = -1
    $loopFooterAt = $null
    while ($true) {
        if ($loopH.Before) { $null = & $loopH.Before $State }
        if ($loopNeedDraw) { $loopMap = & $Draw $State }
        $loopNeedDraw = $true
        $loopKey = & $Wait
        if ("$loopKey" -eq 'resize') { continue }

        if ($loopKey -and $loopKey.Kind -eq 'mouse') {
            $loopTop = & $GetWindowTop
            if ($loopKey.WheelUp -or $loopKey.WheelDown) {
                $loopDelta = if ($loopKey.WheelUp) { -1 } else { 1 }
                if ($loopH.Wheel) { $null = & $loopH.Wheel $State $loopDelta }
                else { $loopN = & $loopRows $State; if ($loopN -gt 0) { $State.Index = [Math]::Max(0, [Math]::Min($loopN - 1, $State.Index + $loopDelta)) } }
                continue
            }
            $loopHit = Get-HitAt -RowMap $loopMap -X $loopKey.X -Y $loopKey.Y -WindowTop $loopTop
            if ($loopKey.IsMove) {
                # Spec D8: a move PAINTS, never selects. A footer button, a list row and an option
                # value are the three things a hover can name; it publishes them on the state and the
                # frame reads them from there. Nothing here touches Index - the cursor is wherever a
                # click or a key put it, and so are the action, the preview and every record.
                # All three are set before either verdict: a move from a lit button straight onto a
                # row has to clear that light in the SAME frame, not leave it lit until some later
                # move happens to land on a gap.
                # -is [int] (R18): a Rows[]-shaped map (launch, maintenance) hands back the row
                # OBJECT, a list map the row INDEX; the object's own Index is the row number, and a
                # row object that never grew one names no row rather than row 0.
                $loopBtn = if ($loopHit.Kind -eq 'footer') { $loopHit.FooterIndex } else { -1 }
                $loopRow = -1; $loopVal = ''
                if ($loopHit.Kind -eq 'row' -and $loopHit.Row -is [int]) { $loopRow = $loopHit.Row }
                elseif ($loopHit.Kind -eq 'cell') {
                    $loopRow = $(if ($null -ne $loopHit.Row.Index) { [int]$loopHit.Row.Index } else { -1 })
                    $loopVal = "$($loopHit.Value)"
                }
                elseif ($loopHit.Kind -eq 'action') { $loopVal = "$($loopHit.Value)" }
                $loopNeedDraw = ($loopBtn -ne $State.Hover) -or ($loopRow -ne $State.HoverRow) -or ($loopVal -ne $State.HoverValue)
                $State.Hover = $loopBtn; $State.HoverRow = $loopRow; $State.HoverValue = $loopVal
                # R19 - LAST MOVE WINS. A terminal delivers 6-12 mouse records per pixel of travel
                # (Input.ps1) and a full render costs 65-180 ms at 198 columns, 474 ms on a 40-session
                # picker: every frame but the last of a sweep is overwritten before anyone sees it.
                # Only the DRAW is dropped - the state above is already updated, so the frame the
                # next event does draw carries every move that led to it. The check is one
                # non-blocking read of the queue's depth (no sleep, no wait, no consume), so an empty
                # queue always draws and the mouse can never leave a stale frame standing.
                if ($loopNeedDraw -and (& $InputPending)) { $loopNeedDraw = $false }
                continue
            }
            # -not $loopKey.IsMove used to gate this too, but it can never be false here: the IsMove
            # branch above always `continue`s the loop, so a move event never reaches this line - the
            # IsMove block above is the real move guard.
            if (-not $loopKey.Left) { continue }
            # Read ONCE for all three twin tests below - the cross-screen record, the footer guard
            # and the row guard - so a press costs one -RecordTime call rather than three.
            $loopNow = & $RecordTime
            # P14: the twin that OUTLIVED its screen (the header carries the whole argument). Every
            # Left press in every screen looks at the record first, footer presses included - the Y
            # decides, because the next screen's rows are not this one's. It guards the NEXT press
            # and only that one: matched or not, it is consumed right here, so an ordinary click
            # that happens to follow an activation is never swallowed twice over.
            if ($script:LastActivation) {
                $loopLast = $script:LastActivation
                $script:LastActivation = $null
                if ($loopKey.Y -eq $loopLast.Y -and ($loopKey.IsDoubleClick -or
                    ($null -ne $loopNow -and $null -ne $loopLast.At -and ($loopNow - $loopLast.At) -lt $loopTwinMs))) {
                    $loopNeedDraw = $false
                    continue
                }
            }
            # A double click on a footer button is ignored outright: the first press already became
            # its key, and a second synthetic press would fire the action twice (or -ReadPath twice).
            if ($loopHit.Kind -eq 'footer') {
                # The FLAG is the console's answer and the STAMP is the only one a VT terminal can
                # give (P7): ConvertFrom-ClaudeMouseReport has no double click to report, so one
                # gesture on a button fired its key twice there - probe, 2 fires 80 ms apart.
                if ($loopKey.IsDoubleClick -or ($loopKey.Y -eq $loopFooterY -and $null -ne $loopNow -and
                    $null -ne $loopFooterAt -and ($loopNow - $loopFooterAt) -lt $loopTwinMs)) { $loopNeedDraw = $false; continue }
                $loopFooterY = $loopKey.Y
                $loopFooterAt = $loopNow
                # Disarms the twin guard above, and it is load-bearing rather than defensive: an
                # Activate that does NOT end the screen (a rejected project pick) leaves it armed,
                # and the next press on that row - after a detour through a button - is a NEW
                # gesture that must act.
                $loopActed = $false
                $loopKey = New-SyntheticKey -Key $loopHit.Footer.Key -Char $loopHit.Footer.Char   # falls through to the key path
            } else {
                # Spec D9: a click on the row that already carries the cursor ACTS - it means what
                # Enter means on this screen. Every other click goes to Click, which only selects.
                # A double click is two clicks: the first selects, the second lands on the selected
                # row and acts, so there is no IsDoubleClick branch on rows at all.
                # Reachable for LIST maps only (Row -is [int]): a Rows[]-shaped screen (launch,
                # maintenance) hands back the row OBJECT, so an Activate registered there is dead by
                # design - those screens act on Enter and on their footer buttons.
                if ($loopHit.Kind -eq 'row' -and $loopHit.Row -is [int] -and $loopHit.Row -eq $State.Index -and $loopH.Activate) {
                    # The twin of a press that JUST acted is the same gesture, not a second one:
                    # without this a rejected project pick prompted for a path twice. Two tests,
                    # because only ONE of them works per terminal (P7) - a console mouse flags the
                    # twin, a VT terminal cannot - and the SAME ROW as well, because a press that
                    # walked to another row is a new gesture however fast it arrived. A press that
                    # follows a plain SELECT is the other half of D9 and still acts.
                    if ($loopActed -and $loopHit.Row -eq $loopActedRow -and ($loopKey.IsDoubleClick -or
                        ($null -ne $loopNow -and $null -ne $loopActedAt -and ($loopNow - $loopActedAt) -lt $loopTwinMs))) {
                        # Nothing on the state changed, so there is nothing to repaint: a swallowed
                        # twin used to cost a full frame (65-180 ms at 198 columns, 474 ms on a
                        # 40-session picker) for a record that does nothing.
                        $loopNeedDraw = $false
                        continue
                    }
                    $loopActed = $true
                    $loopActedRow = [int]$loopHit.Row
                    $loopActedAt = $loopNow
                    # The cross-screen half of the same record (P14), armed BEFORE the handler runs:
                    # a handler that ends the screen returns out of this loop, so anything written
                    # after it would never be written at all. The Y is the RECORD's own, not the
                    # row's - the twin carries the same one, and the next screen's rows are not this
                    # screen's.
                    $script:LastActivation = @{ Y = $loopKey.Y; At = $loopNow }
                    $loopRes = & $loopH.Activate $State $loopHit
                    $null = & $loopLogRes $State 'click' $loopRes @{ button = 'row' }
                    if ($loopRes -and $loopRes.Done) { return (& $loopFinish $State $loopRes) }
                } elseif ($loopHit.Kind -ne 'none' -and $loopH.Click) {
                    $loopActed = $false
                    $loopRes = & $loopH.Click $State $loopHit
                    # A click that logs says so by naming its own key ('click'); one that does not
                    # (a plain row select) leaves no record, exactly as the screens do today.
                    if ($loopRes -is [hashtable] -and $loopRes.Log -and $loopRes.Log.key) {
                        $loopName = [string]$loopRes.Log.key; $loopRes.Log.Remove('key')
                        $null = & $loopLogRes $State $loopName $loopRes
                    }
                    if ($loopRes -and $loopRes.Done) { return (& $loopFinish $State $loopRes) }
                } else {
                    # A click on a gap, or a screen with no Click handler: disarms the twin guard for
                    # the reason the footer branch above records - the press after it is a new gesture.
                    $loopActed = $false
                }
                continue
            }
        }

        if ($loopH.OnKey -and (& $loopH.OnKey $State $loopKey)) { continue }
        $loopName = "$($loopKey.Key)"
        if ($State.Typing -and $loopH.Type) {
            # The Type handler owns every key while typing. It returns $null (not consumed), $true
            # (consumed, nothing to log) or @{ Log = @{ key = 'Enter'; filter = 'close'; ... } } - the
            # loop is the ONLY writer of key records, so a handler describes its record instead of
            # writing one (the trace test pins the exact fields).
            $loopTyped = & $loopH.Type $State $loopKey
            if ($loopTyped) {
                if ($loopTyped -is [hashtable] -and $loopTyped.Log -and $loopTyped.Log.key) {
                    $loopName = [string]$loopTyped.Log.key; $loopTyped.Log.Remove('key')
                    $null = & $loopLogRes $State $loopName $loopTyped
                }
                continue
            }
        }

        # Named arrows always; the w/a/s/d aliases under the guard $loopAlias carries.
        if ($loopName -eq 'UpArrow' -or ($loopHasCursor -and (& $loopAlias 'w'))) {
            if ($loopH.Up) { $null = & $loopH.Up $State } elseif ($State.Index -gt 0) { $State.Index-- }
        } elseif ($loopName -eq 'DownArrow' -or ($loopHasCursor -and (& $loopAlias 's'))) {
            if ($loopH.Down) { $null = & $loopH.Down $State } elseif ($State.Index -lt (& $loopRows $State) - 1) { $State.Index++ }
        } elseif ($loopHasLR -and ($loopName -eq 'LeftArrow' -or $loopName -eq 'RightArrow' -or (& $loopAlias 'a') -or (& $loopAlias 'd'))) {
            $loopBack = ($loopName -eq 'LeftArrow') -or (& $loopAlias 'a')
            # Guarded on the handler, not on $loopHasLR: a screen with only one of the two would
            # otherwise reach `& $null`, which throws rather than doing nothing.
            $loopRes = if ($loopBack) { if ($loopH.Left) { & $loopH.Left $State } } else { if ($loopH.Right) { & $loopH.Right $State } }
            if ($loopH.LogArrows) { $null = & $loopLogRes $State $(if ($loopBack) { 'LeftArrow' } else { 'RightArrow' }) $loopRes }
            if ($loopRes -and $loopRes.Done) { return (& $loopFinish $State $loopRes) }
        } elseif ($loopName -eq 'Enter') {
            $loopRes = if ($loopH.Enter) { & $loopH.Enter $State } else { $null }
            $null = & $loopLogRes $State 'Enter' $loopRes
            if ($loopRes -and $loopRes.Done) { return (& $loopFinish $State $loopRes) }
        } elseif ($loopName -eq 'Escape' -or ($loopKey.Key -eq 'C' -and (& $loopIsCtrl $loopKey))) {
            $loopRes = if ($loopH.Escape) { & $loopH.Escape $State } else { @{ Done = $true; Result = $null } }
            $null = & $loopLogRes $State $(if ($loopName -eq 'Escape') { 'Escape' } else { 'Ctrl+C' }) $loopRes
            if ($loopRes -and $loopRes.Done) { return (& $loopFinish $State $loopRes) }
        } elseif ($loopName -eq 'Tab' -and $loopH.Tab) {
            $loopRes = & $loopH.Tab $State
            $null = & $loopLogRes $State 'Tab' $loopRes
            if ($loopRes -and $loopRes.Done) { return (& $loopFinish $State $loopRes) }
        } elseif ($loopH.Hotkeys) {
            foreach ($loopChar in $loopH.Hotkeys.Keys) {
                if (Test-ClaudeHotkey -Key $loopKey -Char $loopChar) {
                    $loopRes = & $loopH.Hotkeys[$loopChar] $State
                    $null = & $loopLogRes $State $loopChar $loopRes
                    if ($loopRes -and $loopRes.Done) { return (& $loopFinish $State $loopRes) }
                    break
                }
            }
        }
    }
}

function Invoke-LaunchScreen {
    # Returns the finished state, or $null when the user pressed Esc. -Draw is injected so tests
    # pass an empty scriptblock and assert only the state that comes out. Draw, wait, resize, the
    # mouse, the arrows, Enter/Escape and the log records are Invoke-ScreenLoop's; what is left
    # here is what the LAUNCH screen alone does.
    param(
        [Parameter(Mandatory)]$State,
        [Parameter(Mandatory)][scriptblock]$ReadKey,
        # The prefs file as Read-LaunchPrefs returns it. Arriving at a tab with no stash yet loads
        # that account's remembered profile from here; the default makes every existing caller
        # behave as if the file remembered nothing, rather than throwing.
        $Prefs = @{ Profiles = @{} },
        # Returns the row map when it can - see Invoke-SessionPicker for why the renderer, and only
        # the renderer, is allowed to say where things landed.
        [scriptblock]$Draw = {
            param($s)
            $map = $null
            Get-LaunchFrame -State $s -RowMap ([ref]$map) | ForEach-Object { Write-Host $_ }
            $map
        },
        [scriptblock]$Wait = { & $ReadKey },
        [scriptblock]$GetWindowTop = { try { [Console]::WindowTop } catch { 0 } },
        # Called BEFORE arrow handling with the raw key; when it returns $true the loop redraws
        # without interpreting the key further. That is how 'u' opens maintenance from here and
        # comes back. Default returns $false so every existing caller that omits it is unaffected.
        [scriptblock]$OnKey = { param($k) $false }
    )
    # Aliased because a handler resolves its names against Invoke-ScreenLoop first - its header has
    # the rule; this screen's own state travels as $s.State, never as $State.
    $paintLaunch = $Draw
    $askCaller = $OnKey
    $tabPrefs = $Prefs

    # One walk of the stepper - $Count steps of $Delta - and the tab switch the account row is. The
    # arrows walk one step and a click on a cell walks to it, both through the SAME stepper: a
    # direct assignment would skip whatever changing a row is supposed to do, and the two paths
    # would drift apart silently.
    $walkRow = {
        param($s, [int]$Delta, [int]$Count)
        # Captured before the first step, not after the last: a click on a tab may walk two
        # accounts, and the stash belongs to the one the walk started on.
        $leaving = $s.State.Account
        for ($n = 0; $n -lt $Count; $n++) { $s.State = Step-LaunchValue -State $s.State -Delta $Delta }
        # The account row is a tab strip: stepping it is a tab switch, and the five habit rows have
        # to travel with it. Every other row steps and nothing else happens.
        if ((Get-LaunchRows)[$s.State.Row].Name -eq 'Account') {
            $s.State = Switch-LaunchTab -State $s.State -From $leaving -Prefs $tabPrefs -Rows (Get-LaunchRows)
        }
        $s.Index = [int]$s.State.Row
    }

    $st = @{ Index = [int]$State.Row; Hover = -1; HoverRow = -1; HoverValue = ''; Typing = $false; State = $State }
    return (Invoke-ScreenLoop -Screen 'launch' -State $st -Wait $Wait -GetWindowTop $GetWindowTop `
        -Draw { param($s) & $paintLaunch $s.State } -Handlers @{
        # The cursor lives in two places - the loop's Index, and the state's Row that the frame
        # draws and Step-LaunchValue steps. Index wins here; a handler that replaces the state
        # copies Row back onto Index itself. The three hover fields reach the frame the same way.
        Before = { param($s) $s.State.Row = $s.Index; $s.State.Hover = $s.Hover; $s.State.HoverRow = $s.HoverRow; $s.State.HoverValue = $s.HoverValue }
        Rows   = { @(Get-LaunchRows).Count }
        Left   = { param($s) $null = & $walkRow $s (-1) 1 }
        Right  = { param($s) $null = & $walkRow $s 1 1 }
        # DECISIVE keys only: a row step changes a setting the next frame shows anyway, while Enter
        # and Escape END the screen. A footer click has already become its synthetic key inside the
        # loop, so clicking 'enter next' lands here as Enter - one record, one path.
        Enter  = { param($s) @{ Done = $true; Result = $s.State } }
        # A click on a row selects it; a click on one of its option cells selects that value as
        # well, which is what makes this a menu rather than a picture of one. Nothing here can
        # START a session - only Enter does - so a stray click costs at most a changed setting the
        # owner can see on the very next frame.
        Click = {
            param($s, $hit)
            if ($hit.Kind -ne 'row' -and $hit.Kind -ne 'cell') { return }
            $s.Index = [int]$hit.Row.Index
            $s.State.Row = $s.Index
            if ($hit.Kind -ne 'cell') { return }
            $values = @((Get-LaunchRows)[$hit.Row.Index].Values)
            $from = [Array]::IndexOf($values, $s.State.($hit.Row.Name))
            $to = [Array]::IndexOf($values, $hit.Value)
            $steps = 0
            $dir = 1
            if ($from -ge 0 -and $to -ge 0 -and $from -ne $to) {
                $dir = if ($to -lt $from) { -1 } else { 1 }
                $steps = [Math]::Abs($to - $from)
            }
            $null = & $walkRow $s $dir $steps
        }
        # The caller's hook FIRST - that is how clicking 'u maintenance' opens the maintenance
        # screen through exactly the path the letter u takes - and ctrl+r after it.
        # Ctrl+R, not a bare 'r': one keystroke flattens all seven rows, the hint is not in the
        # footer, and the flattened state used to be saved - so a stray keypress read as "my
        # settings reset themselves" (reproduced with `tests\preview.ps1 -Keys r,Enter`). Matched on
        # .Key so it survives a non-Latin keyboard layout, where the character would arrive as
        # something else entirely while the virtual key stays R. Reset-LaunchTab, not
        # New-LaunchState, since the rows became per account: a whole-state reset would flatten
        # every tab's stash, which is the same failure one level up.
        OnKey = {
            param($s, $k)
            if (& $askCaller $k) { return $true }
            if ($k.Key -eq [System.ConsoleKey]::R -and ($k.Modifiers -band [System.ConsoleModifiers]::Control)) {
                $s.State = Reset-LaunchTab -State $s.State
                $s.Index = [int]$s.State.Row
                return $true
            }
            return $false
        }
    })
}

function Invoke-ProjectScreen {
    # Where the session runs and what it does there. Returns @{ Path; Action; Slug } or $null on
    # Escape. Escape at this screen means "back to the launch screen", never "start anyway".
    #
    # Slug rides along because two repositories can share a folder name: the session picker scopes
    # sessions by slug, never by path, so an ambiguous name must never silently fall back to the
    # wrong project's sessions.
    #
    # Draw, wait, resize, the mouse, the arrows, Enter/Escape, the hotkeys and every log record are
    # Invoke-ScreenLoop's; what is left here is what the PROJECT screen alone does.
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][array]$Projects,
        [string]$Cwd = '',
        [string]$Initial = '',
        # What the action field opens on. Nothing persists it (review W1) - the launcher passes
        # nothing and the default applies - but it stays a parameter so the suites can open the
        # screen on any of the four. Canonicalised, never trusted: it decides which flags reach
        # `claude`.
        [string]$InitialAction = 'new',
        [Parameter(Mandatory)][scriptblock]$ReadKey,
        # The hover pair is APPENDED (spec D1's rule for every renderer seam): a caller's Draw that
        # is two parameters short does not fail - the extras land in $args - it simply never paints
        # a band, which is what this screen did before spec D10.
        [scriptblock]$Draw = {
            param($p, $i, $f, $t, $h, $n, $a, $hr, $hv)
            $map = $null
            Get-ProjectFrame -Projects $p -Index $i -Filter $f -Typing:$t -Hover $h -Notice $n -Action $a -Cwd $Cwd -HoverRow $hr -HoverValue $hv -RowMap ([ref]$map) | ForEach-Object { Write-Host $_ }
            $map
        },
        [scriptblock]$Wait = { & $ReadKey },
        [scriptblock]$GetWindowTop = { try { [Console]::WindowTop } catch { 0 } },
        # Reading a free path is I/O, so it is injected: the suites pass a scriptblock and never
        # block on a console prompt.
        [scriptblock]$ReadPath = { Read-Host '  path' },
        # Forwarded to the loop's hover coalescing (R19/P12). A pass-through, not a decision: the
        # default is the loop's own, so a caller that omits it behaves exactly as before, and a
        # suite driving this screen end to end can say "the queue is empty" and get every frame.
        [scriptblock]$InputPending = { Test-ClaudeInputPending },
        # Forwarded the same way, and for P14 it has to be: the record that survives this screen is
        # compared against a stamp taken on the NEXT one, so both screens must read the same clock.
        [scriptblock]$RecordTime = { try { Get-ClaudeInputRecordTime } catch { $null } }
    )
    # Aliased because a handler resolves its names against Invoke-ScreenLoop first - its header has
    # the rule; everything this screen keeps travels on $s.
    $paintProject = $Draw
    $askPath = $ReadPath
    $projectList = $Projects
    $cwdPath = $Cwd

    # Mirrors Get-ProjectFrame's row assembly. Kept here rather than exported so the frame stays
    # pure; the two are pinned against each other by the RowCount assertion in Test-Ui. Slug rides
    # along on a project row so a pick never has to look the project back up by (ambiguous) name.
    $rowsFor = {
        param([string]$f)
        $items = @(Select-ProjectMatch -Projects $projectList -Filter $f)
        # Same order as the frame: the current directory first (spec D6), then the registry, then the
        # free-path row. The two orders are pinned against each other - a list built the other way
        # here would pick a different row than the one the cursor is drawn on.
        $built = @([pscustomobject]@{ Kind = 'cwd'; Path = $cwdPath; Slug = ''; Slugs = @() })
        $built += @($items | ForEach-Object { [pscustomobject]@{ Kind = 'project'; Path = $_.Path; Slug = $_.Slug; Slugs = @(if ($_.Slugs) { $_.Slugs } else { $_.Slug }) } })
        $built += [pscustomobject]@{ Kind = 'path'; Path = '';       Slug = ''; Slugs = @() }
        return @($built)
    }

    # Where the cursor belongs for a given filter text (R16). A filter is a SEARCH: once it narrows
    # to at least one project, the cursor stands on the first match - row 1, the row under the
    # current directory - so closing the box and pressing Enter runs what was searched for. An empty
    # filter, or one nothing matches, leaves it on row 0: there is no match to stand on, and the cwd
    # row is what Enter means on this screen with nothing chosen.
    #
    # Applied where the filter CHANGES rather than in Before, so the owner can still walk back onto
    # the current directory with the cursor keys while a filter is open - a Before that re-parked
    # every frame would make row 0 unreachable and 'w' look dead.
    $parkOnMatch = {
        param([string]$f)
        if ($f -and @(Select-ProjectMatch -Projects $projectList -Filter $f).Count -gt 0) { return 1 }
        return 0
    }

    # The pinned rows carry no slug of their own - the directory they resolve to may still be a
    # known project (the cwd IS one, or a typed path resolves to one), and the session picker needs
    # to know that exactly. ConvertTo-ProjectKey (Projects.ps1) is the one shared normaliser - see
    # its own comment for why a second, ad-hoc one here would eventually drift from it.
    # Returns EVERY slug of that directory: Get-ProjectRegistry merges the slug folders of one real
    # directory onto one row, and the picker must reach all of them.
    # $slugsForPath, not $slugsFor: a handler is resolved against the scope that INVOKES it, and the
    # session picker has a helper of its own with the same job and a different signature. Two screens
    # never share a helper NAME here, however far apart they look in this file.
    $slugsForPath = {
        param([string]$Path)
        if (-not $Path) { return @() }
        $wanted = ConvertTo-ProjectKey $Path
        $same = @($projectList | Where-Object { (ConvertTo-ProjectKey $_.Path) -eq $wanted })
        if ($same.Count -gt 0) { return @($same | ForEach-Object { if ($_.Slugs) { $_.Slugs } else { $_.Slug } }) }
        return @()
    }

    # Resolves the row under the cursor into the result the caller returns.
    #
    # EVERY row kind is checked for existence (fix round 2, IMPORTANT 1): Prefs.ps1's remembered-
    # project guard makes the identical call the other way ("this value becomes a Set-Location
    # target"), and this screen's lifetime is a second window on top of that - long enough for
    # `git worktree remove` in another terminal to invalidate a row the registry still lists. Only
    # the free-path row additionally resolves the path: a registry or cwd path is already in its
    # canonical form, and resolving it here would be pointless.
    #
    # -How is carried on the RESULT rather than logged: the launcher's `ui` record answers "how did
    # this launch choose its project" from one field, and a pick that is REJECTED (a path that no
    # longer exists) must not leave a chosen=... record behind claiming otherwise.
    $pickRow = {
        param($s, [string]$How = 'enter')
        $row = @($s.Rows)[$s.Index]
        $target = $row.Path
        $slugs = @($row.Slugs)
        if ($row.Kind -eq 'path') { $target = ("$(& $askPath)").Trim('"', ' ') }
        # A NUL in a typed path makes Test-Path raise a non-terminating ArgumentException instead of
        # answering $false, and at the default $ErrorActionPreference a four-line red dump lands on
        # the screen in place of this screen's own "path not found" notice (adversarial review
        # 2026-09-16, A6). The decision was always right; only the output was wrong.
        if (-not $target -or $target.IndexOf([char]0) -ge 0) { return $null }
        if (-not (Test-Path -LiteralPath $target -PathType Container)) { return $null }
        if ($row.Kind -eq 'path') {
            $target = (Resolve-Path -LiteralPath $target).Path
            $slugs = @(& $slugsForPath $target)
        } elseif ($row.Kind -eq 'cwd') {
            $slugs = @(& $slugsForPath $target)
        }
        return [pscustomobject]@{ Path = $target; Action = $s.Action; How = $How; Slug = $(if ($slugs.Count -gt 0) { $slugs[0] } else { '' }); Slugs = $slugs }
    }

    # One pick, one notice, one record shape: Enter, a hotkey and a click on the selected row differ
    # only in the How they carry onto the result. A rejected pick still writes its key record - the
    # press did happen - and leaves the notice the next frame shows.
    # N3's written scope, where the code says it: the pick-bearing records (Enter, c/r/t, click)
    # are written AFTER the pick - the loop logs what this handler returns, so `action` on the record
    # is the value the pick actually ran with, never the one the field held before it.
    $commit = {
        param($s, [string]$How)
        $picked = & $pickRow $s $How
        if ($picked) { return @{ Done = $true; Result = $picked; Log = @{ action = $s.Action } } }
        $s.Notice = 'path not found'
        return @{ Log = @{ action = $s.Action } }
    }

    # The remembered project starts under the cursor rather than at the top: arriving at this screen
    # and pressing Enter must reproduce the last launch. Compared through ConvertTo-ProjectKey, not
    # raw string equality: -Initial is whatever the caller last stored, which may differ from the
    # registry's own spelling by case or slash direction (fix round 2, reviewer: 'c:/w/beta/' silently
    # preselected the wrong row under a bare [Array]::IndexOf).
    # Row 0 is the CURRENT DIRECTORY now, so a remembered project sits one row lower than its index
    # in the registry: 1 + that index. With nothing remembered the cursor stays on row 0, which is
    # what makes "arrive and press Enter" mean "run here" (spec D6).
    $startIndex = 0
    if ($Initial) {
        $initialKey = ConvertTo-ProjectKey $Initial
        $at = [Array]::IndexOf(@($Projects | ForEach-Object { ConvertTo-ProjectKey $_.Path }), $initialKey)
        if ($at -ge 0) { $startIndex = 1 + $at }
    }

    # The action field is an INDICATOR, not a cursor stop (review W2/W3): Left/Right and a/d step it
    # from every row, so it never needs focus - and the focus state it used to have could only be
    # entered from the LAST list row, which parked the commit on 'enter a path...' and made the
    # arrows-only path end at a Read-Host prompt.
    $st = @{ Index = $startIndex; Hover = -1; HoverRow = -1; HoverValue = ''; Typing = $false
             Filter = ''; Notice = ''; Action = (Step-ProjectAction -Action $InitialAction -Delta 0); Rows = @() }
    return (Invoke-ScreenLoop -Screen 'project' -State $st -Wait $Wait -GetWindowTop $GetWindowTop -InputPending $InputPending -RecordTime $RecordTime `
        -Draw { param($s) & $paintProject $projectList $s.Index $s.Filter $s.Typing $s.Hover $s.Notice $s.Action $s.HoverRow $s.HoverValue } -Handlers @{
        # The filter decides the rows, so they are rebuilt before every frame and the cursor is
        # clamped to whatever survived it.
        Before = {
            param($s)
            $s.Rows = @(& $rowsFor $s.Filter)
            if ($s.Index -ge @($s.Rows).Count) { $s.Index = [Math]::Max(0, @($s.Rows).Count - 1) }
        }
        # Up/Down walk the LIST and stop at the free-path row, exactly as before the field existed:
        # the action row is not a cursor stop and is not counted here.
        Rows = { param($s) @($s.Rows).Count }
        # A new KEY retires the previous rejection notice - it explains the press that just
        # happened, not every press after it. OnKey is the only hook on the KEY path, and every
        # mouse-only path (a move, the wheel, a row select, a double click) returns before it, so
        # hovering away from a shown notice cannot wipe it before it is read (fix round 3,
        # coordinator ruling). Never consumed: this hook clears, it decides nothing.
        OnKey = { param($s, $k) $s.Notice = ''; $false }
        # The filter box owns every key while it is open - which is what keeps 'c' from firing
        # continue in the middle of a word. The two ways it CLOSES are recorded; the characters
        # between them are not, and neither is Backspace. That is the whole difference between a
        # launcher log and a keylogger - and it is what keeps a file open per keystroke out of here.
        Type = {
            param($s, $k)
            $typed = "$($k.Key)"
            if ($typed -eq 'Enter') {
                $s.Typing = $false
                return @{ Log = @{ key = 'Enter'; filter = 'close'; filterLength = $s.Filter.Length } }
            }
            if ($typed -eq 'Escape' -or ($k.Key -eq 'C' -and ($k.Modifiers -band [System.ConsoleModifiers]::Control))) {
                # Built BEFORE the filter is dropped and BEFORE the cursor goes home: the record says
                # how long what was cleared was, and where the owner was standing when he cleared it.
                # The loop writes the record AFTER the handler ran, so without the index here every
                # clear would report 0 whatever row was selected.
                $cleared = @{ Log = @{ key = 'Escape'; filter = 'clear'; filterLength = $s.Filter.Length; index = [int]$s.Index } }
                $s.Typing = $false
                $s.Filter = ''
                $s.Index = 0
                return $cleared
            }
            if ($typed -eq 'Backspace') {
                if ($s.Filter.Length -gt 0) { $s.Filter = $s.Filter.Substring(0, $s.Filter.Length - 1) }
                # Re-parked on the way back as well: deleting to an empty filter puts the cursor home
                # on the current directory, exactly where clearing it with Escape does.
                $s.Index = & $parkOnMatch $s.Filter
                return $true
            }
            # \ / : are filter characters (fix round 2, minor): Select-ProjectMatch's documented
            # purpose is matching a PASTED path literally, and a path is not a path without its
            # separators and drive colon.
            if ($k.KeyChar -and ([char]::IsLetterOrDigit($k.KeyChar) -or $k.KeyChar -in @(' ', '-', '.', '_', '\', '/', ':'))) {
                $s.Filter += $k.KeyChar
                $s.Index = & $parkOnMatch $s.Filter
            }
            # Everything else is SWALLOWED while typing, exactly as the inline branch did: the box
            # holds the keyboard until it closes.
            return $true
        }
        # Left/Right - and a/d, from ANY row (review W3) - cycle the field. Logged as the DIRECTION,
        # never as the character: a/d and the arrows mean the same thing here, and on a Cyrillic
        # layout the character would be a different letter anyway (LogArrows).
        Left  = { param($s) $s.Action = Step-ProjectAction -Action $s.Action -Delta -1; @{ Log = @{ action = $s.Action } } }
        Right = { param($s) $s.Action = Step-ProjectAction -Action $s.Action -Delta 1;  @{ Log = @{ action = $s.Action } } }
        LogArrows = $true
        Enter = { param($s) & $commit $s 'enter' }
        # The hotkeys fire immediately AND set the field: the press is the answer, and the screen
        # has to say what just happened - which matters most exactly when the pick is REJECTED and
        # the screen draws again with the field the press left behind.
        Hotkeys = @{
            'c' = { param($s) $s.Action = 'continue'; & $commit $s 'hotkey' }
            'r' = { param($s) $s.Action = 'resume';   & $commit $s 'hotkey' }
            't' = { param($s) $s.Action = 'worktree'; & $commit $s 'hotkey' }
            '/' = { param($s) $s.Typing = $true; @{ Log = @{ filter = 'open' } } }
        }
        Click = {
            param($s, $hit)
            # The action field sits below the last list row and is NOT in RowCount, so it arrives as
            # its own hit kind. A click on a VALUE walks there through the SAME stepper the arrows
            # use, because a click that assigned a value directly would be a second implementation
            # of the field waiting to drift (the launch screen's own rule). A click elsewhere on the
            # row does nothing (W5): the field has no focus to take, and it must never move the
            # selection.
            if ($hit.Kind -eq 'action') {
                if ($null -eq $hit.Value -or $hit.Value -eq $s.Action) { return }
                $values = @(Get-ProjectActions)
                $from = [Array]::IndexOf($values, $s.Action)
                $to = [Array]::IndexOf($values, $hit.Value)
                $dir = if ($to -lt $from) { -1 } else { 1 }
                for ($step = 0; $step -lt [Math]::Abs($to - $from); $step++) { $s.Action = Step-ProjectAction -Action $s.Action -Delta $dir }
                # Selecting a value is decisive in the brief's sense: it changes what Enter will DO.
                return @{ Log = @{ key = 'click'; button = 'action'; action = $s.Action } }
            }
            # A single click only MOVES. Starting a session on a stray click is the one mistake
            # nobody forgives - the same rule the session picker follows.
            if ($hit.Kind -eq 'row') { $s.Index = [int]$hit.Row }
            return
        }
        # A click on the row that already carries the cursor is the exception to that rule (spec D9):
        # it is Enter's gesture with a mouse. It commits the FIELD, not a hardcoded 'new' - the
        # gesture means "this row, that action", and two answers to "what does a commit do here"
        # would disagree the first time one of them changed.
        Activate = { param($s, $hit) & $commit $s 'mouse' }
        # filterLength, never the filter: a filter is often a pasted PATH, and the log is not the
        # place for it. Screen records only - the key records carry what the key itself changed.
        ScreenFields = { param($s) @{ filterLength = $s.Filter.Length } }
    })
}

function Expand-SessionPage {
    # Appends the next page of sessions to a picker's list. Pulled out of Invoke-SessionPicker so
    # the keyboard and the wheel grow the list through the SAME rule, and so that rule is assertable
    # without driving a picker loop.
    #
    # A row already on the list is never added again: the newest-first window moves whenever a
    # session is written while the picker is open, so the next page can overlap the last one and the
    # same session would otherwise be offered twice.
    #
    # An EMPTY page means the end. "Added nothing" does not: a page that is entirely overlap - the
    # exact case the dedup exists for - would then end the paging permanently with sessions still
    # behind it (adversarial review 2026-09-16, D1). A fetcher that throws (a root that vanished
    # mid-session) ends the paging rather than the picker.
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][array]$Sessions,
        [Parameter(Mandatory)][scriptblock]$FetchMore,
        # How many rows have been FETCHED so far - the offset into the caller's snapshot. This is
        # NOT $Sessions.Count once a page has overlapped: the dedup drops rows, and an offset taken
        # from the deduped count re-reads the same window forever. -1 keeps the old behaviour for a
        # caller that does not thread it.
        [int]$Fetched = -1,
        # Which scope the page is for, handed to the fetcher beside the offset so a scoped picker
        # pages its OWN project instead of the whole account.
        [string[]]$Scope = @()
    )
    $offset = if ($Fetched -ge 0) { $Fetched } else { @($Sessions).Count }
    $page = @()
    # A fetcher that fails because the WORLD changed - a projects root that vanished, a transcript
    # that went away mid-read - ends the paging rather than the picker. A fetcher that fails because
    # it is MISWIRED does not: an ArgumentException or a binding failure is a programming error, and
    # swallowing it turns a scoped picker that can no longer page into one that quietly stops, which
    # is the same silence C1 hid behind for a whole round (re-review 2 2026-09-16, N2).
    try { $page = @(& $FetchMore $offset $Scope) }
    catch [System.ArgumentException] { throw }
    catch [System.Management.Automation.ParameterBindingException] { throw }
    catch {
        $page = @()
        # Same log the launcher's UI try/catch writes (claude-auto.ps1), for the same reason: a
        # fetcher failure here used to vanish with nothing in the launcher log but start/ui/decision/
        # exit. $RunId and $Preview are script-scope in the launcher, not in this file, so read them
        # defensively; tests load Ui.ps1 on its own (no Env.ps1), so skip silently when the function
        # is not there. -not $script:Preview: a preview run can hit this same IOException reading
        # real session files, and preview must stay side-effect-free - the same fact that made the
        # launcher's own UI catch guard on -not $Preview.
        if ((Get-Command Write-LauncherLog -ErrorAction SilentlyContinue) -and -not $script:Preview) {
            $logArgs = @{
                Stage = 'error'
                Data  = @{
                    where   = 'Expand-SessionPage'
                    type    = $_.Exception.GetType().Name
                    message = $_.Exception.Message.Substring(0, [Math]::Min(300, $_.Exception.Message.Length))
                }
            }
            if ($script:RunId) { $logArgs.RunId = $script:RunId }
            $null = Write-LauncherLog @logArgs
        }
    }
    $seen = @{}
    foreach ($s in $Sessions) { $seen["$($s.SessionId)|$($s.Path)"] = $true }
    $added = @($page | Where-Object { $_ -and -not $seen["$($_.SessionId)|$($_.Path)"] })
    return [pscustomobject]@{
        Sessions  = @(@($Sessions) + $added)
        Added     = $added.Count
        Fetched   = $offset + $page.Count
        Exhausted = ($page.Count -eq 0)
    }
}

function Invoke-SessionPicker {
    # Returns the chosen session object, or $null when the user pressed Esc at the list level.
    #
    # Draw, wait, resize, the mouse, the arrows, Enter/Escape, the hotkeys and every log record are
    # Invoke-ScreenLoop's; what is left here is what the PICKER alone does - the scope, the per-scope
    # page buckets, the filter box, and what a row means once it is chosen.
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][array]$Sessions,
        [Parameter(Mandatory)][scriptblock]$ReadKey,
        # Scopes the picker to one project. -ProjectSlug is the exact key (a session's transcript
        # directory name) and is what actually filters; -ProjectName is kept ONLY as the display
        # label for the title/footer, except as a fallback filter when no slug is known at all (an
        # unrecognised cwd has none). Two repositories can share a folder name, so scoping by name
        # alone would silently mix their sessions together - slug never does, because it comes from
        # the transcript path itself, not from a display string.
        # A LIST: one real directory can own several slug folders (a cwd recorded with different
        # separators, a folder renamed and renamed back). Get-ProjectRegistry merges those into one
        # row and keeps every slug on it, and the picker must reach all of them or half the project's
        # sessions are unreachable from the screen that just named it.
        [string[]]$ProjectSlug = @(),
        [string]$ProjectName = '',
        # $Draw RETURNS the row map when it can - where the session rows landed on screen - so a
        # click can be turned into an index by the same arithmetic that drew them. A Draw that
        # returns nothing (every existing test injects one) simply leaves the mouse inert.
        # SEVEN parameters since the rows learned a hover band on top of the footer's lit button
        # (spec D1, then D10): a renderer one short does not fail - the extra argument lands in
        # $args - it simply never paints, which is what the screen did before each of them.
        [scriptblock]$Draw = {
            param($s, $i, $f, $sc, $pn, $hv, $hr)
            $map = $null
            Get-PickerFrame -Sessions $s -Index $i -Filter $f -Scope $sc -ProjectName $pn -Hover $hv -HoverRow $hr -RowMap ([ref]$map) | ForEach-Object { Write-Host $_ }
            $map
        },
        [scriptblock]$Wait = { & $ReadKey },
        # Mouse coordinates are SCREEN-BUFFER rows; the frame is drawn relative to the visible
        # window. Injected so the mapping is assertable without a console.
        [scriptblock]$GetWindowTop = { try { [Console]::WindowTop } catch { 0 } },
        # Paging. Called with the number of rows already on the list, whenever the cursor reaches
        # the last row, and expected to return the next page. The launcher hands the picker one
        # page instead of every session so the first frame costs one page's worth of summarising
        # rather than forty. $null for a caller that already holds the whole list - the picker then
        # behaves exactly as it did, and the cursor simply stops at the last row.
        [scriptblock]$FetchMore = $null,
        # Forwarded to the loop's hover coalescing (R19/P12). A pass-through, not a decision: the
        # default is the loop's own, so a caller that omits it behaves exactly as before, and a
        # suite driving this screen end to end can say "the queue is empty" and get every frame.
        [scriptblock]$InputPending = { Test-ClaudeInputPending },
        # Forwarded the same way, and for P14 it has to be: the record that survives the PREVIOUS
        # screen is compared against a stamp taken here, so both screens must read the same clock.
        [scriptblock]$RecordTime = { try { Get-ClaudeInputRecordTime } catch { $null } }
    )
    # Aliased because a handler resolves its names against Invoke-ScreenLoop first - its header has
    # the rule; everything this screen keeps travels on $s.
    $paintPicker = $Draw
    $fetchNext = $FetchMore
    $pickerTitle = $ProjectName
    $firstPage = @($Sessions)

    # No slug AND no name means there is nothing to scope to (an unrecognised cwd, or a caller that
    # never learned a project at all) - the picker then behaves exactly as it always has, and Tab
    # does nothing (its handler is not installed at all, below), because there is no "other" scope to
    # widen from or narrow to.
    # 'none' (fix round 1, IMPORTANT 1), not 'all': Get-PickerFrame renders 'none' with NO tab hint
    # at all, where 'all' would show one labelled "this project" that Tab could never act on - a
    # button that always does nothing, wrapping the footer at 80 columns for every user who has
    # never even seen the project screen.
    # Empties filtered out: a caller passing -ProjectSlug '' means "no slug", and a one-element array
    # holding '' would otherwise read as a scope that matches nothing.
    $slugSet = @($ProjectSlug | Where-Object { $_ })
    $hasScope = ($slugSet.Count -gt 0) -or [bool]$pickerTitle
    $openScope = if ($hasScope) { 'project' } else { 'none' }

    # One page bucket PER SCOPE. The page the launcher handed in was fetched for the scope the
    # picker opens in; Tab is a different question of disk ("every session of this account", not
    # "the next ten of this project") and gets its own page 1 and its own paging offset. Keeping
    # both means Tab back and forth costs one fetch each way, not one per press.
    $slugsForScope = { param([string]$ForScope) if ($ForScope -eq 'project') { $slugSet } else { @() } }
    $newBucket = {
        param([string]$ForScope)
        $fresh = [pscustomobject]@{ Sessions = $firstPage; Fetched = $firstPage.Count; Exhausted = $true }
        if ($fetchNext) {
            $fresh = [pscustomobject]@{ Sessions = @(); Fetched = 0; Exhausted = $false }
            $grown = Expand-SessionPage -Sessions @() -FetchMore $fetchNext -Fetched 0 -Scope (& $slugsForScope $ForScope)
            $fresh.Sessions = $grown.Sessions; $fresh.Fetched = $grown.Fetched; $fresh.Exhausted = $grown.Exhausted
        }
        return $fresh
    }

    # The cursor at the last row and one notch of wheel past it are the SAME question - "what is
    # behind the end of this list" - so they are one rule here instead of two copies that drift.
    # $s.Index is bumped past the end on purpose: the appended rows still have to pass the scope and
    # the filter, and Before re-clamps $s.Index against $s.Items before the next frame, so this lands
    # on the first NEW visible row or stays put when the page added nothing visible. Not bumped when
    # the list was EMPTY: there was no row under the cursor to step off, so the first fetched row
    # would be skipped over (adversarial review 2026-09-16, D3).
    $stepDown = {
        param($s)
        if ($s.Index -lt @($s.Items).Count - 1) { $s.Index++; return }
        if (-not $s.CanPage) { return }
        $bucket = $s.Pages[$s.Scope]
        $grown = Expand-SessionPage -Sessions $bucket.Sessions -FetchMore $fetchNext -Fetched $bucket.Fetched -Scope (& $slugsForScope $s.Scope)
        $bucket.Sessions = $grown.Sessions; $bucket.Fetched = $grown.Fetched; $bucket.Exhausted = $grown.Exhausted
        if ($grown.Added -gt 0 -and @($s.Items).Count -gt 0) { $s.Index++ }
    }

    # What a chosen row answers - Enter, f and a double click differ only in the Fork they carry.
    # An empty list answers NOTHING, and the press that asked writes no record either (the loop's
    # @{ NoLog = $true }): both keys are guarded on the count today and log only when they return a
    # session. A row index past the end is the same case - the map can outlive the list it was drawn
    # from - and Before re-clamps the cursor before the next frame.
    $takeRow = {
        param($s, [bool]$Fork)
        $shown = @($s.Items)
        if ($shown.Count -eq 0 -or $s.Index -lt 0 -or $s.Index -ge $shown.Count) { return $null }
        return [pscustomobject]@{ Session = $shown[$s.Index]; Fork = $Fork }
    }

    $st = @{ Index = 0; Hover = -1; HoverRow = -1; HoverValue = ''; Typing = $false; Filter = ''
             Scope = $openScope; Pages = @{}; Pool = @(); Resumable = @(); Items = @(); CanPage = $false
             # What the caller handed in. `rows` on the ENTER record is that page, never what
             # survived the scope, the zero-prompt drop and the filter - the two differing is
             # information, not drift: "handed ten, showed none" is exactly the shape of the
             # empty-picker defects this screen has had (C9).
             HandedIn = $firstPage.Count }
    $st.Pages[$openScope] = [pscustomobject]@{ Sessions = $firstPage; Fetched = $firstPage.Count; Exhausted = (-not $fetchNext) }

    $handlers = @{
        # Scoped BEFORE Select-ResumableSessions/Select-SessionMatch run, so $s.Items - and therefore
        # $s.Index - only ever ranges over the sessions the current scope actually shows. $s.Pool
        # (not $Sessions) is what gets handed to $Draw too, so Get-PickerFrame's own hiddenCount and
        # "N sessions" title reflect the scoped pool, never the full account.
        Before = {
            param($s)
            if (-not $s.Pages.ContainsKey($s.Scope)) { $s.Pages[$s.Scope] = & $newBucket $s.Scope }
            $bucket = $s.Pages[$s.Scope]
            # @() wraps the WHOLE if/else, not just its branches: `$x = if (...) {...} else { @() }`
            # unwraps an empty-array branch to $null on assignment regardless of how that branch
            # built it.
            $s.Pool = @(
                if ($s.Scope -eq 'project' -and $hasScope) {
                    if ($slugSet.Count -gt 0) { $bucket.Sessions | Where-Object { $_.Slug -in $slugSet } }
                    else { $bucket.Sessions | Where-Object { $_.Project -eq $pickerTitle } }
                } else { $bucket.Sessions }
            )
            # Select-ResumableSessions drops empty (zero-prompt) sessions before the filter runs, and
            # Get-PickerFrame does the exact same thing before rendering - the two must never
            # disagree about which index points at which session.
            # @() is load-bearing even though Select-ResumableSessions already wraps ITS OWN return:
            # a function returning zero items unrolls to $null on the pipeline regardless of how it
            # built that array internally, and Select-SessionMatch's -Sessions is Mandatory - an
            # account with every session filtered out (or none at all) crashed the picker here with a
            # raw PowerShell binding error. Found via tests\check-preview.ps1's empty-fixture run.
            $s.Resumable = @(Select-ResumableSessions -Sessions $s.Pool)
            $s.Items = @(Select-SessionMatch -Sessions $s.Resumable -Filter $s.Filter)
            # Paging stops only when the fetcher is out of rows, or when the filter can see NOTHING
            # at all - the case where every Down was a synchronous cold disk page that could not
            # change the frame (adversarial review 2026-09-16, D4). Gating on "the filter hides
            # nothing" instead (`$items.Count -ge $resumable.Count`) went too far: one hidden row
            # stopped every fetch, so a session matching the filter one page deeper was unreachable
            # (re-review, W1).
            $s.CanPage = [bool]$fetchNext -and -not $bucket.Exhausted -and -not (@($s.Items).Count -eq 0 -and @($s.Resumable).Count -gt 0)
            if ($s.Index -ge @($s.Items).Count) { $s.Index = [Math]::Max(0, @($s.Items).Count - 1) }
        }
        Rows = { param($s) @($s.Items).Count }
        # Up is the loop's own clamp; Down is the paging rule, and the wheel asks both the same way.
        Down  = { param($s) & $stepDown $s }
        Wheel = { param($s, $d) if ($d -lt 0) { if ($s.Index -gt 0) { $s.Index-- } } else { & $stepDown $s } }
        Enter = { param($s) $picked = & $takeRow $s $false; if ($picked) { return @{ Done = $true; Result = $picked } }; return @{ NoLog = $true } }
        # The filter box owns every key while it is open - which is what keeps 'f' from FORKING a
        # session in the middle of a typed word. The two ways it CLOSES are recorded; the characters
        # between them are not, and neither is Backspace. That is the whole difference between a
        # launcher log and a keylogger - and it is what keeps a file open per keystroke out of here.
        Type = {
            param($s, $k)
            $typed = "$($k.Key)"
            if ($typed -eq 'Enter') {
                $s.Typing = $false
                return @{ Log = @{ key = 'Enter'; filter = 'close'; filterLength = $s.Filter.Length } }
            }
            # Esc here clears the filter rather than leaving: while typing, Esc means "undo the
            # filter", and losing the whole picker to a stray Esc would be infuriating. Ctrl+C
            # matches that same semantics rather than leaving the picker.
            if ($typed -eq 'Escape' -or ($k.Key -eq 'C' -and ($k.Modifiers -band [System.ConsoleModifiers]::Control))) {
                # Built BEFORE the filter is dropped and BEFORE the cursor goes home: the record says
                # how long what was cleared was, and where the owner was standing when he cleared it.
                # The loop writes the record AFTER the handler ran, so without the index here every
                # clear would report 0 whatever the owner had selected.
                $cleared = @{ Log = @{ key = 'Escape'; filter = 'clear'; filterLength = $s.Filter.Length; index = [int]$s.Index } }
                $s.Typing = $false
                $s.Filter = ''
                $s.Index = 0
                return $cleared
            }
            if ($typed -eq 'Backspace') {
                if ($s.Filter.Length -gt 0) { $s.Filter = $s.Filter.Substring(0, $s.Filter.Length - 1) }
                return $true
            }
            if ($k.KeyChar -and ([char]::IsLetterOrDigit($k.KeyChar) -or $k.KeyChar -eq ' ' -or $k.KeyChar -eq '-')) {
                $s.Filter += $k.KeyChar
                $s.Index = 0
            }
            # Everything else is SWALLOWED while typing, exactly as the inline branch did: the box
            # holds the keyboard until it closes, so no key behind it can reach a hotkey.
            return $true
        }
        Hotkeys = @{
            # Test-ClaudeHotkey (Input.ps1): the character, the virtual key or the Cyrillic letter on
            # the same physical key - and never uppercase or modified, for the same reason the
            # maintenance screen was case-sensitive first: an uppercase F arriving from a terminal
            # that reports the mouse as text would FORK a session, which starts one (hover defect,
            # 2026-08-25).
            'f' = { param($s) $picked = & $takeRow $s $true; if ($picked) { return @{ Done = $true; Result = $picked; Log = @{ fork = $true } } }; return @{ NoLog = $true } }
            '/' = { param($s) $s.Typing = $true; @{ Log = @{ filter = 'open' } } }
        }
        # Deliberately conservative about what opens a session: a click on a row the cursor is not on
        # only MOVES the selection (and loads its preview), and only a click on the SELECTED row - or
        # Enter - opens one. A stray click that launched a session would be the kind of mistake
        # nobody forgives, and the cost of the caution is one extra click. A click past the end of
        # the list moves nothing.
        Click = { param($s, $hit) if ($hit.Kind -eq 'row' -and $hit.Row -ge 0 -and $hit.Row -lt @($s.Items).Count) { $s.Index = [int]$hit.Row }; return }
        # Enter's gesture with a mouse (spec D9); the loop ignores a click on a footer button here.
        Activate = { param($s, $hit) $picked = & $takeRow $s $false; if ($picked) { return @{ Done = $true; Result = $picked } }; return @{ NoLog = $true } }
        # `scope` rides on EVERY record: Tab is the one key here that changes what the whole list
        # MEANS, and "the picker was empty" reads completely differently scoped to a project than
        # widened to the account.
        LogFields = { param($s) @{ scope = $s.Scope } }
        # filterLength, never the filter: a filter is often a pasted PATH, and the log is not the
        # place for it.
        ScreenFields = { param($s) @{ filterLength = $s.Filter.Length } }
        ScreenRows = { param($s, $phase) if ($phase -eq 'enter') { [int]$s.HandedIn } else { @($s.Items).Count } }
    }
    # Guarded on $hasScope by not existing at all: with nothing to scope to there is no "other" scope
    # to widen from or narrow to, so Tab does nothing rather than toggling between two labels that
    # would both mean "everything". Tab, not a letter: Test-ClaudeHotkey is for the Latin letters a
    # footer hint advertises via -Char, and the loop matches every named key by name.
    if ($hasScope) {
        # The scope AFTER the toggle: the record answers "what was the owner looking at", and what he
        # was looking at from here on is the new one.
        $handlers.Tab = { param($s) $s.Scope = $(if ($s.Scope -eq 'project') { 'all' } else { 'project' }); $s.Index = 0; @{ Log = @{ scope = $s.Scope } } }
    }
    return (Invoke-ScreenLoop -Screen 'picker' -State $st -Wait $Wait -GetWindowTop $GetWindowTop -InputPending $InputPending -RecordTime $RecordTime `
        -Draw { param($s) & $paintPicker $s.Pool $s.Index $s.Filter $s.Scope $pickerTitle $s.Hover $s.HoverRow } -Handlers $handlers)
}

function ConvertTo-StatusText {
    # Whatever a child process printed, reduced to text that is safe to lay out. An escape sequence
    # adds invisible characters to .Length, so a coloured line makes every width calculation
    # downstream cut in the wrong place - a failure this codebase has already paid for once. The
    # capture happens with stdout redirected, where Claude Code emits no colour today, but that is
    # the child's choice to change and not a property this screen should depend on.
    param($Output)
    # -Width pinned: Out-String otherwise hard-wraps at the HOST width, so the same output would
    # break differently in a 80-column window, a 200-column one and a redirected run - and the
    # wrapping this screen does is Split-OutputLines' job, at the width of the box.
    $text = ($Output | Out-String -Width 4096)
    # CSI: colour, cursor moves, erases - everything of the form ESC [ ... <letter>.
    $text = $text -replace "$([char]27)\[[0-9;?]*[A-Za-z]", ''
    # Any remaining control character. The line breaks are kept because Split-OutputLines splits on
    # them; the tab is kept NOT because the layout understands it - it does not, a tab is 1 in
    # .Length and up to 8 cells on screen - but because the word wrapper downstream splits on \s+
    # and turns it into a single space. Stripping it here would glue "Key:<tab>value" into
    # "Key:value" instead.
    $text = $text -replace '[^\P{C}\r\n\t]', ''
    return $text.Trim()
}

function Invoke-ClaudeCommandText {
    # `& claude ...` throws outright when the binary is missing or unreadable, and this is precisely
    # the screen someone opens when their install is already broken - so a throw here is the worst
    # possible failure mode. It becomes a status line instead.
    param([Parameter(Mandatory)][string[]]$Arguments)
    try { return (ConvertTo-StatusText (& claude @Arguments 2>&1)) }
    catch { return "claude $($Arguments -join ' ') could not be started: $($_.Exception.Message)" }
}

function Invoke-MaintenanceScreen {
    # Returns nothing: this screen acts and comes back. Every action reports through $s.Status rather
    # than printing, so the frame stays the single source of what is on screen.
    #
    # Draw, wait, resize, the mouse, Escape and Ctrl+C are Invoke-ScreenLoop's; what is left here is
    # what THIS menu does - the confirm-twice guard, the keys, and the drain after a child process
    # owned the screen.
    #
    # Every action that shells out draws a "running ..." frame first. Without it the screen simply
    # freezes for as long as the child takes - `claude update` downloading ~300 MB is the bad case -
    # with nothing on screen to say anything is happening, which reads as a broken key.
    param(
        [Parameter(Mandatory)][scriptblock]$ReadKey,
        [scriptblock]$Wait = { & $ReadKey },
        # THREE parameters since this footer learned to light its hovered button (spec D1) - see
        # Invoke-SessionPicker's own renderer for why one short is silent rather than fatal.
        [scriptblock]$Draw = {
            param($info, $status, $hv)
            $map = $null
            Get-MaintenanceFrame -Info $info -Status $status -Hover $hv -RowMap ([ref]$map) -Actions $Actions | ForEach-Object { Write-Host $_ }
            $map
        },
        [scriptblock]$GetWindowTop = { try { [Console]::WindowTop } catch { 0 } },
        # Configured actions: { Key, Label, Script, ConfirmTwice } each. One footer hint and one
        # key per action; the key runs the script through Invoke-MaintenanceScript.
        [object[]]$Actions = @(),
        # Injected into every action so the path can be asserted without running any script. $null
        # means "use the real one", which is what the launcher passes.
        [scriptblock]$Runner = $null,
        # Called after every action that handed the screen to a child process. Whatever the terminal
        # queued in the meantime is noise behind a ten-second command, not an instruction to this
        # menu - see the drain comment below. $null keeps every existing caller (and every test that
        # omits it) on the old behaviour.
        [scriptblock]$Drain = $null,
        # When the terminal delivered the record now being handled. From the INPUT RECORD, never
        # from this loop's own clock: a redraw plus Get-ClaudeInstallInfo between two real presses
        # easily outlasts any threshold worth setting, while two characters of one paste arrive
        # microseconds apart however slow the screen is. $null - the keyboard-only ReadKey path -
        # leaves the guard inert rather than blocking a confirm the reader really did press twice.
        [scriptblock]$RecordTime = { try { Get-ClaudeInputRecordTime } catch { $null } },
        [int]$ConfirmMinMs = 150
    )
    # Aliased because a handler resolves its names against Invoke-ScreenLoop first - its header has
    # the rule. This screen also opens from inside the LAUNCH screen's OnKey, so its scope chain
    # reaches that screen's locals too, which is why every name below is its own.
    $paintMaint = $Draw
    $drainAfter = $Drain
    $recordAt = $RecordTime
    $actionList = @($Actions)
    $scriptRunner = $Runner
    $confirmMs = $ConfirmMinMs
    # One place, so a new action cannot forget it: run the child, then throw away everything the
    # terminal queued while it owned the screen (a replayed hover ran `mcp list` over and over).
    $runMaint = {
        param([scriptblock]$Deferred)
        $maintOut = & $Deferred
        if ($drainAfter) { $null = & $drainAfter }
        return $maintOut
    }
    # Every configured action shares ONE handler: the loop dispatches by character and does not say
    # WHICH entry matched, so OnKey - which runs first, on the same key, through the same
    # Test-ClaudeHotkey the loop dispatches with - leaves the matched action on $s.Hit.
    # ConfirmTwice ones are confirmed first because they are SLOW - a menu that freezes for minutes
    # with no warning reads as a hung launcher; the confirm text says so.
    $maintAction = {
        param($s)
        $chosen = $s.Hit
        if (-not $chosen) { return }
        if ($chosen.ConfirmTwice -and ($s.WasPending -ne $chosen.Key -or $s.TooFast)) {
            $s.Pending = $chosen.Key
            $s.PendingAt = $s.Now
            $s.Status = "confirm: press $($chosen.Key) again to run $($chosen.Label) (the screen will sit still while it runs)"
        } else {
            $s.Status = "running $($chosen.Label)..."
            $null = & $paintMaint $s.Info $s.Status $s.Hover
            $s.Status = (& $runMaint { (Invoke-MaintenanceScript -ScriptPath $chosen.Script -Label $chosen.Label -Runner $scriptRunner).Message })
        }
    }
    # The five built-in keys, then the configured actions - and a configured action never takes a
    # built-in letter. Dispatched through Test-ClaudeHotkey, never -eq: its own header carries why
    # (uppercase and modified keys are refused, and every layout is matched).
    $maintKeys = @{
        'u' = { param($s) $s.Status = 'running claude update...'; $null = & $paintMaint $s.Info $s.Status $s.Hover; $s.Status = (& $runMaint { (Invoke-ClaudeUpdate).Message }) }
        'r' = { param($s) $s.Status = (& $runMaint { (Repair-ClaudeBinaryByRename).Message }) }
        # No Select-Object -Last 3 on either of these. Both reports put what matters at the TOP -
        # doctor's version, path, install method and last update attempt; the mcp list's first
        # servers - so keeping the last three lines showed doctor's closing boilerplate and one
        # arbitrary server, which is why both keys looked like they did nothing.
        'd' = { param($s) $s.Status = 'running claude doctor...'; $null = & $paintMaint $s.Info $s.Status $s.Hover; $s.Status = (& $runMaint { Invoke-ClaudeCommandText -Arguments @('doctor') }) }
        'm' = { param($s) $s.Status = 'running claude mcp list...'; $null = & $paintMaint $s.Info $s.Status $s.Hover; $s.Status = (& $runMaint { Invoke-ClaudeCommandText -Arguments @('mcp', 'list') }) }
        'p' = {
            param($s)
            if ($s.WasPending -ne 'p' -or $s.TooFast) { $s.Pending = 'p'; $s.PendingAt = $s.Now; $s.Status = "confirm: press p again to delete all but the 2 newest builds" }
            else { $pruned = Remove-OldClaudeVersions -Keep 2; $s.Status = "deleted $($pruned.Deleted.Count) builds, freed $('{0:N1}' -f ($pruned.FreedBytes / 1GB)) GB" }
        }
    }
    foreach ($maintCfg in $actionList) {
        # A key the table already holds is a built-in. An action with NO key is dropped rather than
        # registered: Test-ClaudeHotkey's -Char is a Mandatory [string], so an empty one does not
        # merely fail to match - it throws at parameter binding, and from this table it would throw
        # on every single keypress, taking the launch screen behind this one down with it.
        if ($null -eq $maintCfg -or -not $maintCfg.Key) { continue }
        if ($maintKeys.ContainsKey([string]$maintCfg.Key)) { continue }
        $maintKeys[[string]$maintCfg.Key] = $maintAction
    }

    $maintState = @{
        # HoverRow/HoverValue are seeded although nothing on this screen reads them (P11): it has
        # nothing hoverable but its footer, and the seeds only keep the loop's change test off a
        # missing key.
        Index = 0; Hover = -1; HoverRow = -1; HoverValue = ''; Typing = $false
        # What the frame on screen was drawn from, so an action's "running ..." frame repaints the
        # same install info the frame under it already showed.
        Info = $null
        Status = ''
        # The key whose confirm is armed, $null otherwise. State, never a parse of the status text:
        # a key interpolated into a regex ('.' or '[') either matched everything or threw, and a
        # generic '^confirm' let one key's warning confirm ANOTHER key's action. Any press consumes
        # it. PendingAt is when it was armed - a PASTE is not two presses: this screen acts on every
        # character it is handed and ConfirmTwice was the only brake, so a pasted string containing
        # 'pp' deleted builds and 'ii' started a five-minute fleet reindex with nobody touching the
        # keyboard.
        Pending = $null
        PendingAt = $null
        # What OnKey read off the press being dispatched, for the handler behind it: the confirm that
        # was armed when it arrived, whether it came too fast to be a second press, its arrival
        # stamp, and the configured action it names.
        WasPending = $null
        TooFast = $false
        Now = $null
        Hit = $null
    }

    $maintHandlers = @{
        # Everything the old loop body did BEFORE dispatching a key, in the slot that runs before the
        # hotkeys and so keeps that precedence: the trace line, the confirm-twice bookkeeping, and
        # which configured action this press names. It never consumes the key.
        OnKey = {
            param($s, $k)
            # What the menu is about to ACT on, beside the raw record the reader already traced.
            # Reading only one of the two answers "a key arrived"; reading both answers "and this is
            # the branch it took", which is the question the hover diagnosis kept getting wrong.
            # Guarded on the flag, not on Get-Command: a Get-Command per loop iteration is a command
            # lookup on every keypress and every mouse move, which is the kind of cost a diagnostic
            # has no business adding to the thing it observes.
            if ($script:TraceOn) {
                $maintChar = "$($k.KeyChar)"
                $maintCode = if ($maintChar.Length -gt 0) { [int][char]$maintChar[0] } else { 0 }
                Write-ClaudeInputTrace ("ACT  maintenance ch=U+{0:X4} key={1}" -f $maintCode, "$($k.Key)")
            }
            $s.WasPending = $s.Pending
            $maintArmedAt = $s.PendingAt
            $s.Pending = $null
            $s.PendingAt = $null
            # Too fast to be a second press. The armed key is RE-armed by the branch behind this one
            # rather than cancelled, so a long paste of the same letter is a stream of re-arms and
            # never an action. The stamp comes from the INPUT RECORD, never from this screen's own
            # clock: a redraw plus Get-ClaudeInstallInfo between two real presses easily outlasts any
            # threshold worth setting, while two characters of one paste arrive microseconds apart
            # however slow the screen is. $null - the keyboard-only ReadKey path - leaves the guard
            # inert rather than blocking a confirm the reader really did press twice.
            # Runs on the exit press too (Escape/'q' route through OnKey like every other key) - one
            # extra RecordTime call per screen exit, whose value nothing on that path reads.
            $s.Now = & $recordAt
            $s.TooFast = ($null -ne $s.Now -and $null -ne $maintArmedAt -and ($s.Now - $maintArmedAt) -lt $confirmMs)
            # An unrelated key cancels an armed confirm AND its text: otherwise "press i again" stays
            # on screen while the next i only re-arms. Branches that re-arm set their own text.
            if ($s.WasPending -and -not (Test-ClaudeHotkey -Key $k -Char $s.WasPending)) { $s.Status = '' }
            $s.Hit = $null
            foreach ($maintCand in $actionList) {
                if ($null -eq $maintCand -or -not $maintCand.Key) { continue }
                if (Test-ClaudeHotkey -Key $k -Char $maintCand.Key) { $s.Hit = $maintCand; break }
            }
            return $false
        }
        Hotkeys = $maintKeys
    }
    # No Rows and no Click: this screen has no cursor (which keeps the loop's w/a/s/d aliases inert -
    # 'd' is doctor) and nothing on it is clickable but the footer, which the loop already turns into
    # that hint's key. -Silent: this screen writes no log records and never did.
    # The install info is refreshed by the DRAW, so the frame and the info on it are always one pass.
    $null = Invoke-ScreenLoop -Screen 'maintenance' -State $maintState -Wait $Wait -GetWindowTop $GetWindowTop -Silent `
        -Draw { param($s) $s.Info = Get-ClaudeInstallInfo; & $paintMaint $s.Info $s.Status $s.Hover } -Handlers $maintHandlers
}
