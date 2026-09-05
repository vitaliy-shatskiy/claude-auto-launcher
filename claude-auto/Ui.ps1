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
    try { [Console]::TreatControlCAsInput = $true } catch { }
}

function Exit-AltBuffer {
    [Console]::Write("$([char]27)[?25h$([char]27)[?1049l")
    try { [Console]::TreatControlCAsInput = $false } catch { }
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

function Invoke-LaunchScreen {
    # Returns the finished state, or $null when the user pressed Esc. -Draw is injected so tests
    # pass an empty scriptblock and assert only the state that comes out.
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
    while ($true) {
        $rowMap = & $Draw $State
        $key = & $Wait
        if ("$key" -eq 'resize') { continue }

        # Mouse. A click on a row selects it; a click on one of its option cells selects that value
        # as well, which is what makes this a menu rather than a picture of one. Nothing here can
        # START a session - only Enter does that - so a stray click costs at most a changed setting
        # the owner can see on the very next frame.
        if ($key -and $key.Kind -eq 'mouse') {
            $rows = @(Get-LaunchRows)
            $synthetic = $null
            if ($key.WheelUp) { if ($State.Row -gt 0) { $State.Row-- }; continue }
            if ($key.WheelDown) { if ($State.Row -lt $rows.Count - 1) { $State.Row++ }; continue }
            if ($key.Left -and -not $key.IsMove -and $rowMap) {
                $y = $key.Y - (& $GetWindowTop)
                $hint = Get-ClaudeFooterHit -RowMap $rowMap -X $key.X -Y $key.Y -WindowTop (& $GetWindowTop)
                if ($hint) { $synthetic = New-SyntheticKey -Key $hint.Key -Char $hint.Char }
                $hit = if ($hint) { @() } else { @($rowMap.Rows | Where-Object { $_.Y -eq $y }) }
                if ($hit.Count -gt 0) {
                    $State.Row = $hit[0].Index
                    $cell = @($hit[0].Cells | Where-Object { $key.X -ge $_.Start -and $key.X -le $_.End })
                    if ($cell.Count -gt 0) {
                        # Walk to the clicked value with the SAME stepper the arrow keys use, one
                        # step at a time. Assigning the value directly would skip whatever changing
                        # a row is supposed to do, and the two paths would drift apart silently.
                        $values = @($rows[$hit[0].Index].Values)
                        $from = [Array]::IndexOf($values, $State.($hit[0].Name))
                        $to = [Array]::IndexOf($values, $cell[0].Value)
                        # Captured before the first step, not after the last: a click on a tab may
                        # walk two accounts, and the stash belongs to the one the click started on.
                        $leaving = $State.Account
                        if ($from -ge 0 -and $to -ge 0 -and $from -ne $to) {
                            $dir = if ($to -lt $from) { -1 } else { 1 }
                            for ($n = 0; $n -lt [Math]::Abs($to - $from); $n++) {
                                $State = Step-LaunchValue -State $State -Delta $dir
                            }
                        }
                        if ($hit[0].Name -eq 'Account') { $State = Switch-LaunchTab -State $State -From $leaving -Prefs $Prefs -Rows $rows }
                    }
                }
            }
            # A footer click becomes its key and falls through to the handlers below - including
            # $OnKey, which is how clicking 'u maintenance' opens the maintenance screen through
            # exactly the path the letter u takes.
            if (-not $synthetic) { continue }
            $key = $synthetic
        }

        if (& $OnKey $key) { continue }
        $name = "$($key.Key)"

        # if/elseif rather than switch: inside a while loop, `continue` in a PowerShell switch
        # continues the LOOP rather than leaving the branch, which is a trap worth not setting.
        # Ctrl+R, not a bare 'r': one keystroke flattens all seven rows, the hint is not in the
        # footer, and the flattened state used to be saved - so a stray keypress read as "my settings
        # reset themselves" (reproduced with `tests\preview.ps1 -Keys r,Enter`).
        # Matched on .Key so it survives a non-Latin keyboard layout, where the character would arrive
        # as something else entirely while the virtual key stays R.
        #
        # Reset-LaunchTab, not New-LaunchState, since the rows became per account (2026-09-04): a
        # whole-state reset would flatten every tab's stash, which is the same "my settings reset
        # themselves" failure one level up.
        if ($key.Key -eq [System.ConsoleKey]::R -and ($key.Modifiers -band [System.ConsoleModifiers]::Control)) { $State = Reset-LaunchTab -State $State }
        elseif ($name -eq 'UpArrow') { if ($State.Row -gt 0) { $State.Row-- } }
        elseif ($name -eq 'DownArrow') { if ($State.Row -lt (Get-LaunchRows).Count - 1) { $State.Row++ } }
        # The account row is a tab strip: stepping it is a tab switch, and the five habit rows have
        # to travel with it. Every other row steps and nothing else happens.
        elseif ($name -eq 'LeftArrow' -or $name -eq 'RightArrow') {
            $leaving = $State.Account
            $State = Step-LaunchValue -State $State -Delta $(if ($name -eq 'LeftArrow') { -1 } else { 1 })
            if ((Get-LaunchRows)[$State.Row].Name -eq 'Account') { $State = Switch-LaunchTab -State $State -From $leaving -Prefs $Prefs -Rows (Get-LaunchRows) }
        }
        elseif ($name -eq 'Enter') { return $State }
        # Ctrl+C is read as input (TreatControlCAsInput, set in Enter-AltBuffer) and treated exactly
        # like Escape, so the `finally` that restores the alternate buffer still runs.
        elseif ($name -eq 'Escape' -or ($key.Key -eq 'C' -and ($key.Modifiers -band [System.ConsoleModifiers]::Control))) { return $null }
    }
}

function Invoke-SessionPicker {
    # Returns the chosen session object, or $null when the user pressed Esc at the list level.
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][array]$Sessions,
        [Parameter(Mandatory)][scriptblock]$ReadKey,
        # $Draw RETURNS the row map when it can - where the session rows landed on screen - so a
        # click can be turned into an index by the same arithmetic that drew them. A Draw that
        # returns nothing (every existing test injects one) simply leaves the mouse inert.
        [scriptblock]$Draw = {
            param($s, $i, $f)
            $map = $null
            Get-PickerFrame -Sessions $s -Index $i -Filter $f -RowMap ([ref]$map) | ForEach-Object { Write-Host $_ }
            $map
        },
        [scriptblock]$Wait = { & $ReadKey },
        # Mouse coordinates are SCREEN-BUFFER rows; the frame is drawn relative to the visible
        # window. Injected so the mapping is assertable without a console.
        [scriptblock]$GetWindowTop = { try { [Console]::WindowTop } catch { 0 } }
    )
    $index = 0
    $filter = ''
    $typing = $false
    $rowMap = $null

    while ($true) {
        # Select-ResumableSessions drops empty (zero-prompt) sessions before the filter runs, and
        # Get-PickerFrame does the exact same thing before rendering - the two must never disagree
        # about which index points at which session.
        $items = Select-SessionMatch -Sessions (Select-ResumableSessions -Sessions $Sessions) -Filter $filter
        if ($index -ge $items.Count) { $index = [Math]::Max(0, $items.Count - 1) }
        $rowMap = & $Draw $Sessions $index $filter
        $key = & $Wait
        if ("$key" -eq 'resize') { continue }

        # Mouse. Deliberately conservative about what opens a session: a single click only MOVES the
        # selection, and only a double click (or Enter) opens one. A stray click that launched a
        # session would be the kind of mistake nobody forgives, and the cost of the caution is one
        # extra click for people who want it.
        if ($key -and $key.Kind -eq 'mouse') {
            $synthetic = $null
            if ($key.WheelUp) { if ($index -gt 0) { $index-- } ; continue }
            if ($key.WheelDown) { if ($index -lt $items.Count - 1) { $index++ } ; continue }
            # Act on the PRESS: the release carries the same position with no button set, and a drag
            # arrives as MOVE-with-button. Both are ignored, or one click would fire twice.
            if ($key.Left -and -not $key.IsMove -and $rowMap) {
                $top = & $GetWindowTop
                $hint = Get-ClaudeFooterHit -RowMap $rowMap -X $key.X -Y $key.Y -WindowTop $top
                if ($hint) {
                    # Becomes the key the footer advertises and falls through to the handlers below,
                    # so clicking 'enter open' is the same code path as pressing Enter.
                    $synthetic = New-SyntheticKey -Key $hint.Key -Char $hint.Char
                } else {
                    $row = Get-ClaudeMouseRow -Y $key.Y -FirstRowY $rowMap.FirstRowY -RowCount $rowMap.RowCount -WindowTop $top
                    if ($null -ne $row) {
                        $target = $rowMap.Start + $row
                        if ($target -ge 0 -and $target -lt $items.Count) {
                            $index = $target
                            if ($key.IsDoubleClick) { return [pscustomobject]@{ Session = $items[$index]; Fork = $false } }
                        }
                    }
                }
            }
            if (-not $synthetic) { continue }
            $key = $synthetic
        }

        $name = "$($key.Key)"

        if ($typing) {
            # Esc here clears the filter rather than leaving: while typing, Esc means "undo the
            # filter", and losing the whole picker to a stray Esc would be infuriating. Ctrl+C
            # matches that same semantics rather than leaving the picker.
            if ($name -eq 'Enter') { $typing = $false }
            elseif ($name -eq 'Escape' -or ($key.Key -eq 'C' -and ($key.Modifiers -band [System.ConsoleModifiers]::Control))) { $typing = $false; $filter = ''; $index = 0 }
            elseif ($name -eq 'Backspace') {
                if ($filter.Length -gt 0) { $filter = $filter.Substring(0, $filter.Length - 1) }
            }
            elseif ($key.KeyChar -and ([char]::IsLetterOrDigit($key.KeyChar) -or $key.KeyChar -eq ' ' -or $key.KeyChar -eq '-')) {
                $filter += $key.KeyChar
                $index = 0
            }
            continue
        }

        if ($name -eq 'UpArrow') { if ($index -gt 0) { $index-- } }
        elseif ($name -eq 'DownArrow') { if ($index -lt $items.Count - 1) { $index++ } }
        elseif ($name -eq 'Enter') { if ($items.Count -gt 0) { return [pscustomobject]@{ Session = $items[$index]; Fork = $false } } }
        elseif ($name -eq 'Escape' -or ($key.Key -eq 'C' -and ($key.Modifiers -band [System.ConsoleModifiers]::Control))) { return $null }
        # Test-ClaudeHotkey (Input.ps1): the character, the virtual key or the Cyrillic letter on
        # the same physical key - and never uppercase or modified, for the same reason the
        # maintenance screen was case-sensitive first: an uppercase F arriving from a terminal that
        # reports the mouse as text would FORK a session, which starts one (hover defect, 2026-08-25).
        elseif (Test-ClaudeHotkey -Key $key -Char 'f') { if ($items.Count -gt 0) { return [pscustomobject]@{ Session = $items[$index]; Fork = $true } } }
        elseif (Test-ClaudeHotkey -Key $key -Char '/') { $typing = $true }
    }
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
    # Returns nothing: this screen acts and comes back. Every action reports through $status rather
    # than printing, so the frame stays the single source of what is on screen.
    #
    # Every action that shells out draws a "running ..." frame first. Without it the screen simply
    # freezes for as long as the child takes - `claude update` downloading ~300 MB is the bad case -
    # with nothing on screen to say anything is happening, which reads as a broken key.
    param(
        [Parameter(Mandatory)][scriptblock]$ReadKey,
        [scriptblock]$Wait = { & $ReadKey },
        [scriptblock]$Draw = {
            param($info, $status)
            $map = $null
            Get-MaintenanceFrame -Info $info -Status $status -RowMap ([ref]$map) -Actions $Actions | ForEach-Object { Write-Host $_ }
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
        [scriptblock]$Drain = $null
    )
    $status = ''
    $rowMap = $null
    # The key whose confirm is armed, $null otherwise. State, never a parse of the status text: a
    # key interpolated into a regex ('.' or '[') either matched everything or threw, and a generic
    # '^confirm' let one key's warning confirm ANOTHER key's action. Any press consumes it.
    $pending = $null
    # One place, so a new action cannot forget it: run the child, then throw away everything the
    # terminal queued while it owned the screen (a replayed hover ran `mcp list` over and over).
    $run = {
        param([scriptblock]$Action)
        $result = & $Action
        if ($Drain) { & $Drain }
        return $result
    }
    while ($true) {
        $info = Get-ClaudeInstallInfo
        $rowMap = & $Draw $info $status
        $key = & $Wait
        if ("$key" -eq 'resize') { continue }

        # This screen has no rows to select - its body is status text - so the mouse does exactly
        # one thing: a click on a footer hint becomes that hint's key. Everything else is ignored.
        #
        # A DOUBLE_CLICK record is excluded along with a move: it carries the button down with
        # MOUSE_MOVED clear, so it used to walk through this guard as a SECOND press - enough to get
        # past the confirm on prune and on the five-minute fleet reindex with one gesture.
        if ($key -and $key.Kind -eq 'mouse') {
            $synthetic = $null
            if ($key.Left -and -not $key.IsMove -and -not $key.IsDoubleClick -and $rowMap) {
                $hint = Get-ClaudeFooterHit -RowMap $rowMap -X $key.X -Y $key.Y -WindowTop (& $GetWindowTop)
                if ($hint) { $synthetic = New-SyntheticKey -Key $hint.Key -Char $hint.Char }
            }
            if (-not $synthetic) { continue }
            $key = $synthetic
        }

        $ch = "$($key.KeyChar)"
        $name = "$($key.Key)"
        # What the menu is about to ACT on, beside the raw record the reader already traced. Reading
        # only one of the two answers "a key arrived"; reading both answers "and this is the branch
        # it took", which is the question the hover diagnosis kept getting wrong.
        # Guarded on the flag, not on Get-Command: a Get-Command per loop iteration is a command
        # lookup on every keypress and every mouse move, which is the kind of cost a diagnostic has
        # no business adding to the thing it observes.
        if ($script:TraceOn) {
            $code = if ($ch.Length -gt 0) { [int][char]$ch[0] } else { 0 }
            Write-ClaudeInputTrace ("ACT  maintenance ch=U+{0:X4} key={1}" -f $code, $name)
        }
        # Test-ClaudeHotkey on every one of these, never -eq. PowerShell's -eq is case-INSENSITIVE,
        # so the uppercase letter that terminates an SGR mouse report (ESC [ < b ; x ; y M) pressed
        # the very key its lowercase hint advertises: a hover over a terminal that delivers mouse
        # reports as text ran `claude mcp list` again and again, with U, R, D, P and I - update,
        # rename swap, doctor, prune and a five-minute fleet reindex - one character away. Caught
        # live in a Rider terminal tab 2026-08-25. The matcher keeps that guard and adds
        # the virtual key and the Cyrillic letter on the same physical key (owner, 2026-09-02: the
        # keys must work on the Russian and Ukrainian layouts).
        if ($name -eq 'Escape' -or ($key.Key -eq 'C' -and ($key.Modifiers -band [System.ConsoleModifiers]::Control))) { return }
        $wasPending = $pending
        $pending = $null
        # An unrelated key cancels an armed confirm AND its text: otherwise "press i again" stays on
        # screen while the next i only re-arms. Branches that re-arm set their own text below.
        if ($wasPending -and -not (Test-ClaudeHotkey -Key $key -Char $wasPending)) { $status = '' }
        if (Test-ClaudeHotkey -Key $key -Char 'u') { $status = 'running claude update...'; & $Draw $info $status; $status = (& $run { (Invoke-ClaudeUpdate).Message }) }
        elseif (Test-ClaudeHotkey -Key $key -Char 'r') { $status = (& $run { (Repair-ClaudeBinaryByRename).Message }) }
        # No Select-Object -Last 3 on either of these. Both reports put what matters at the TOP -
        # doctor's version, path, install method and last update attempt; the mcp list's first
        # servers - so keeping the last three lines showed doctor's closing boilerplate and one
        # arbitrary server, which is why both keys looked like they did nothing.
        elseif (Test-ClaudeHotkey -Key $key -Char 'd') { $status = 'running claude doctor...'; & $Draw $info $status; $status = (& $run { Invoke-ClaudeCommandText -Arguments @('doctor') }) }
        elseif (Test-ClaudeHotkey -Key $key -Char 'm') { $status = 'running claude mcp list...'; & $Draw $info $status; $status = (& $run { Invoke-ClaudeCommandText -Arguments @('mcp', 'list') }) }
        elseif (Test-ClaudeHotkey -Key $key -Char 'p') {
            if ($wasPending -ne 'p') { $pending = 'p'; $status = "confirm: press p again to delete all but the 2 newest builds" }
            else { $r = Remove-OldClaudeVersions -Keep 2; $status = "deleted $($r.Deleted.Count) builds, freed $('{0:N1}' -f ($r.FreedBytes / 1GB)) GB" }
        }
        # Configured actions. ConfirmTwice ones are confirmed first because they are SLOW - a menu
        # that freezes for minutes with no warning reads as a hung launcher; the confirm text says so.
        else {
            foreach ($a in $Actions) {
                if (-not (Test-ClaudeHotkey -Key $key -Char $a.Key)) { continue }
                if ($a.ConfirmTwice -and $wasPending -ne $a.Key) {
                    $pending = $a.Key
                    $status = "confirm: press $($a.Key) again to run $($a.Label) (the screen will sit still while it runs)"
                } else {
                    $status = "running $($a.Label)..."
                    & $Draw $info $status
                    $status = (& $run { (Invoke-MaintenanceScript -ScriptPath $a.Script -Label $a.Label -Runner $Runner).Message })
                }
                break
            }
        }
    }
}
