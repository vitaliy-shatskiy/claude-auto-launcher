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
    #
    # Kept here as well as in Open-ClaudeConsoleInput because the two paths are independent: with
    # CLAUDE_AUTO_NO_MOUSE=1 the console is never armed and this is the only thing standing between
    # Ctrl+C and a terminal left on the alternate screen. Nested set-and-restore is safe as long as
    # each side puts back what IT found, which is what the previous hardcoded $false did not do -
    # a process started with Ctrl+C already treated as input had that setting taken away by a
    # launcher that never set it.
    try { $script:AltBufferPrevTcc = [Console]::TreatControlCAsInput; [Console]::TreatControlCAsInput = $true } catch { }
}

function Exit-AltBuffer {
    [Console]::Write("$([char]27)[?25h$([char]27)[?1049l")
    if ($null -ne $script:AltBufferPrevTcc) {
        try { [Console]::TreatControlCAsInput = [bool]$script:AltBufferPrevTcc } catch { }
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
        # WASD navigates alongside the arrows on every screen with a cursor (2026-09-09): w/a/s/d
        # go through Test-ClaudeHotkey (Input.ps1), never -eq, so a shifted or Cyrillic-layout key
        # is judged by the same modifier/case guards as every other hotkey.
        elseif ($name -eq 'UpArrow' -or (Test-ClaudeHotkey -Key $key -Char 'w')) { if ($State.Row -gt 0) { $State.Row-- } }
        elseif ($name -eq 'DownArrow' -or (Test-ClaudeHotkey -Key $key -Char 's')) { if ($State.Row -lt (Get-LaunchRows).Count - 1) { $State.Row++ } }
        # The account row is a tab strip: stepping it is a tab switch, and the five habit rows have
        # to travel with it. Every other row steps and nothing else happens.
        elseif ($name -eq 'LeftArrow' -or $name -eq 'RightArrow' -or
                (Test-ClaudeHotkey -Key $key -Char 'a') -or (Test-ClaudeHotkey -Key $key -Char 'd')) {
            $leaving = $State.Account
            $back = ($name -eq 'LeftArrow') -or (Test-ClaudeHotkey -Key $key -Char 'a')
            $State = Step-LaunchValue -State $State -Delta $(if ($back) { -1 } else { 1 })
            if ((Get-LaunchRows)[$State.Row].Name -eq 'Account') { $State = Switch-LaunchTab -State $State -From $leaving -Prefs $Prefs -Rows (Get-LaunchRows) }
        }
        elseif ($name -eq 'Enter') { return $State }
        # Ctrl+C is read as input (TreatControlCAsInput, set in Enter-AltBuffer) and treated exactly
        # like Escape, so the `finally` that restores the alternate buffer still runs.
        elseif ($name -eq 'Escape' -or ($key.Key -eq 'C' -and ($key.Modifiers -band [System.ConsoleModifiers]::Control))) { return $null }
    }
}

function Invoke-ProjectScreen {
    # Where the session runs and what it does there. Returns @{ Path; Action; Slug } or $null on
    # Escape. Escape at this screen means "back to the launch screen", never "start anyway".
    #
    # Slug rides along because two repositories can share a folder name: the session picker (a
    # later task) scopes sessions by slug, never by path, so an ambiguous name must never silently
    # fall back to the wrong project's sessions.
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][array]$Projects,
        [string]$Cwd = '',
        [string]$Initial = '',
        # What the action field opens on - the account's remembered choice (Prefs.ps1's
        # ProjectAction). Validated here rather than trusted: it arrives from an ordinary text file
        # and decides which flags reach `claude`.
        [string]$InitialAction = 'new',
        [Parameter(Mandatory)][scriptblock]$ReadKey,
        [scriptblock]$Draw = {
            param($p, $i, $f, $t, $h, $n, $a, $oa)
            $map = $null
            Get-ProjectFrame -Projects $p -Index $i -Filter $f -Typing:$t -Hover $h -Notice $n -Action $a -OnAction:$oa -Cwd $Cwd -RowMap ([ref]$map) | ForEach-Object { Write-Host $_ }
            $map
        },
        [scriptblock]$Wait = { & $ReadKey },
        [scriptblock]$GetWindowTop = { try { [Console]::WindowTop } catch { 0 } },
        # Reading a free path is I/O, so it is injected: the suites pass a scriptblock and never
        # block on a console prompt.
        [scriptblock]$ReadPath = { Read-Host '  path' }
    )
    $filter = ''
    $typing = $false
    $hover = -1
    $notice = ''
    $rowMap = $null
    $rows = @()
    $index = 0
    # The action field, and whether the cursor is parked on it. $onAction is a FLAG rather than one
    # more index into $rows on purpose: the field is the last cursor stop but it must not consume
    # the list highlight, or arrowing down to it would silently change which directory Enter commits.
    $action = if ($InitialAction -in (Get-ProjectActions)) { $InitialAction } else { (Get-ProjectActions)[0] }
    $onAction = $false
    # The remembered project starts under the cursor rather than at the top: arriving at this screen
    # and pressing Enter must reproduce the last launch. Compared through ConvertTo-ProjectKey, not
    # raw string equality: -Initial is whatever the caller last stored, which may differ from the
    # registry's own spelling by case or slash direction (fix round 2, reviewer: 'c:/w/beta/' silently
    # preselected the wrong row under a bare [Array]::IndexOf).
    if ($Initial) {
        $initialKey = ConvertTo-ProjectKey $Initial
        $at = [Array]::IndexOf(@($Projects | ForEach-Object { ConvertTo-ProjectKey $_.Path }), $initialKey)
        if ($at -ge 0) { $index = $at }
    }

    # Mirrors Get-ProjectFrame's row assembly. Kept here rather than exported so the frame stays
    # pure; the two are pinned against each other by the RowCount assertion in Test-Ui. Slug rides
    # along on a project row so a pick never has to look the project back up by (ambiguous) name.
    $rowsOf = {
        param($f)
        $items = @(Select-ProjectMatch -Projects $Projects -Filter $f)
        $r = @($items | ForEach-Object { [pscustomobject]@{ Kind = 'project'; Path = $_.Path; Slug = $_.Slug; Slugs = @(if ($_.Slugs) { $_.Slugs } else { $_.Slug }) } })
        $r += [pscustomobject]@{ Kind = 'cwd';  Path = $Cwd; Slug = ''; Slugs = @() }
        $r += [pscustomobject]@{ Kind = 'path'; Path = '';   Slug = ''; Slugs = @() }
        return @($r)
    }

    # The pinned rows carry no slug of their own - the directory they resolve to may still be a
    # known project (the cwd IS one, or a typed path resolves to one), and the session picker needs
    # to know that exactly. ConvertTo-ProjectKey (Projects.ps1) is the one shared normaliser - see
    # its own comment for why a second, ad-hoc one here would eventually drift from it.
    # Returns EVERY slug of that directory: Get-ProjectRegistry merges the slug folders of one real
    # directory onto one row, and the picker must reach all of them.
    $slugOf = {
        param([string]$Path)
        if (-not $Path) { return @() }
        $key = ConvertTo-ProjectKey $Path
        $hit = @($Projects | Where-Object { (ConvertTo-ProjectKey $_.Path) -eq $key })
        if ($hit.Count -gt 0) { return @($hit | ForEach-Object { if ($_.Slugs) { $_.Slugs } else { $_.Slug } }) }
        return @()
    }

    # Resolves the current row into the result the caller returns. Hoisted out of the loop (fix
    # round 2, minor: it does not close over anything the loop body does not already hold, and a
    # scriptblock literal re-evaluated every iteration was pointless allocation) - $rows/$index still
    # resolve to whatever the loop most recently set, since this is an ordinary scriptblock, not a
    # closure snapshot.
    #
    # EVERY row kind is checked for existence now (fix round 2, IMPORTANT 1): Prefs.ps1's remembered-
    # project guard makes the identical call the other way ("this value becomes a Set-Location
    # target"), and this screen's lifetime is a second window on top of that - long enough for
    # `git worktree remove` in another terminal to invalidate a row the registry still lists. Only
    # the free-path row additionally resolves the path: a registry or cwd path is already in its
    # canonical form, and resolving it here would be pointless.
    $pick = {
        param([string]$Action)
        $r = $rows[$index]
        $path = $r.Path
        $slugs = @($r.Slugs)
        if ($r.Kind -eq 'path') { $path = ("$(& $ReadPath)").Trim('"', ' ') }
        # A NUL in a typed path makes Test-Path raise a non-terminating ArgumentException instead of
        # answering $false, and at the default $ErrorActionPreference a four-line red dump lands on
        # the screen in place of this loop's own "path not found" notice (adversarial review
        # 2026-09-16, A6). The decision was always right; only the output was wrong.
        if (-not $path -or $path.IndexOf([char]0) -ge 0) { return $null }
        if (-not (Test-Path -LiteralPath $path -PathType Container)) { return $null }
        if ($r.Kind -eq 'path') {
            $path = (Resolve-Path -LiteralPath $path).Path
            $slugs = @(& $slugOf $path)
        } elseif ($r.Kind -eq 'cwd') {
            $slugs = @(& $slugOf $path)
        }
        return [pscustomobject]@{ Path = $path; Action = $Action; Slug = $(if ($slugs.Count -gt 0) { $slugs[0] } else { '' }); Slugs = $slugs }
    }

    # Hover is a REDUCTION, not a cost: a plain top-of-loop draw (as every mouse move already forces
    # on the launch and session screens) would redraw on every motion event regardless of this flag,
    # so the flag has to gate the draw call itself. $true means "the coming top-of-loop draw may
    # run"; only a mouse move that lands on the SAME footer button as last time ever clears it -
    # every other path (including a move that changes the hovered button) leaves it set, so a real
    # hover change still redraws exactly once.
    $needDraw = $true

    while ($true) {
        $rows = & $rowsOf $filter
        if ($index -ge $rows.Count) { $index = [Math]::Max(0, $rows.Count - 1) }
        if ($needDraw) { $rowMap = & $Draw $Projects $index $filter $typing $hover $notice $action $onAction }
        $needDraw = $true
        $key = & $Wait
        if ("$key" -eq 'resize') { continue }

        if ($key -and $key.Kind -eq 'mouse') {
            $synthetic = $null
            if ($key.WheelUp)   { if ($index -gt 0) { $index-- } ; continue }
            if ($key.WheelDown) { if ($index -lt $rows.Count - 1) { $index++ } ; continue }
            $top = & $GetWindowTop
            # Hover: only a CHANGE of the hovered button is worth a frame. A move inside the same
            # button - the overwhelming majority of motion events - skips the NEXT draw entirely,
            # which is strictly less work than this loop did before hover existed.
            if ($key.IsMove) {
                $hit = Get-ClaudeFooterHit -RowMap $rowMap -X $key.X -Y $key.Y -WindowTop $top
                $now = if ($hit) { [Array]::IndexOf(@($rowMap.Footer), $hit) } else { -1 }
                if ($now -eq $hover) { $needDraw = $false; continue }
                $hover = $now
                continue
            }
            # -not IsMove (fix round 2, IMPORTANT 2): a drag is Left set WITH IsMove, and without
            # this guard it fell through as a press on every position it passed over. IsDoubleClick
            # is excluded from the FOOTER-hit branch only (mirrors Invoke-MaintenanceScreen: a
            # physical double click reaches this loop as TWO records, a plain press then one flagged
            # IsDoubleClick, and treating the second one as a second footer press fired the action -
            # and, if it read the free-path row, called -ReadPath - a second time).
            if ($key.Left -and -not $key.IsMove -and $rowMap) {
                $hint = if ($key.IsDoubleClick) { $null } else { Get-ClaudeFooterHit -RowMap $rowMap -X $key.X -Y $key.Y -WindowTop $top }
                if ($hint) { $synthetic = New-SyntheticKey -Key $hint.Key -Char $hint.Char }
                # The action field sits below the last list row and is NOT in RowCount, so
                # Get-ClaudeMouseRow answers $null for it - which is what lets this branch own it
                # without a special case inside the row hit test. A click anywhere on the field
                # focuses it; a click on a cap also steps it, through the SAME stepper the arrows
                # use, because a click that assigned a value directly would be a second
                # implementation of the field waiting to drift (the launch screen's own rule).
                elseif ($rowMap.Action -and ($key.Y - $top) -eq $rowMap.Action.Y) {
                    $onAction = $true
                    $cell = @($rowMap.Action.Cells | Where-Object { $key.X -ge $_.Start -and $key.X -le $_.End })
                    if ($cell.Count -gt 0) { $action = Step-ProjectAction -Action $action -Delta $cell[0].Delta }
                }
                else {
                    $row = Get-ClaudeMouseRow -Y $key.Y -FirstRowY $rowMap.FirstRowY -RowCount $rowMap.RowCount -WindowTop $top
                    if ($null -ne $row) {
                        $target = $rowMap.Start + $row
                        # A single click only MOVES. Starting a session on a stray click is the one
                        # mistake nobody forgives - the same rule the session picker follows. A
                        # DOUBLE click is the session picker's own exception to that rule: two
                        # presses close enough to register as one gesture are unambiguous intent.
                        if ($target -ge 0 -and $target -lt $rows.Count) {
                            $index = $target
                            $onAction = $false
                            if ($key.IsDoubleClick) {
                                # The FIELD, not a hardcoded 'new': the gesture means "this row,
                                # that action", and two answers to "what does a commit do here"
                                # would disagree the first time one of them changed.
                                $r = & $pick $action
                                if ($r) { return $r } else { $notice = 'path not found' }
                            }
                        }
                    }
                }
            }
            if (-not $synthetic) { continue }
            $key = $synthetic
        }

        # A new KEY event retires the previous rejection notice - it explains the press that just
        # happened, not every press after it. Set again below if THIS key also fails a pick. This
        # point is reached only by a genuine keyboard event or a mouse click that just became a
        # synthetic one (a footer hit): every purely mouse path above it - a move, the wheel, a
        # plain row-select click, a double click handled inline - already `continue`d without
        # passing through here, so hovering away from a shown notice cannot wipe it before it is
        # read (fix round 3, coordinator ruling).
        $notice = ''
        $name = "$($key.Key)"

        if ($typing) {
            if ($name -eq 'Enter') { $typing = $false }
            elseif ($name -eq 'Escape' -or ($key.Key -eq 'C' -and ($key.Modifiers -band [System.ConsoleModifiers]::Control))) { $typing = $false; $filter = ''; $index = 0 }
            elseif ($name -eq 'Backspace') { if ($filter.Length -gt 0) { $filter = $filter.Substring(0, $filter.Length - 1) } }
            # \ / : added (fix round 2, minor): Select-ProjectMatch's documented purpose is matching
            # a PASTED path literally, and a path is not a path without its separators and drive
            # colon.
            elseif ($key.KeyChar -and ([char]::IsLetterOrDigit($key.KeyChar) -or $key.KeyChar -in @(' ', '-', '.', '_', '\', '/', ':'))) {
                $filter += $key.KeyChar
                $index = 0
            }
            continue
        }

        # Up/Down walk the list and then the action field, which is the last stop. Reaching it does
        # not move $index: the list keeps its own mark and the frame shows it, so Enter there commits
        # exactly the row the screen still points at.
        if ($name -eq 'UpArrow'   -or (Test-ClaudeHotkey -Key $key -Char 'w')) {
            if ($onAction) { $onAction = $false } elseif ($index -gt 0) { $index-- }
        }
        elseif ($name -eq 'DownArrow' -or (Test-ClaudeHotkey -Key $key -Char 's')) {
            if (-not $onAction) { if ($index -lt $rows.Count - 1) { $index++ } else { $onAction = $true } }
        }
        # Left/Right cycle the field from ANY row - the owner asked for arrows to be enough, and
        # walking down to the field first would be two more keystrokes for the commonest choice.
        # a/d only while the field HAS focus: on a list row they keep whatever they mean there
        # (nothing, on this screen), so adding them cannot shadow a key this screen already uses.
        elseif ($name -eq 'LeftArrow' -or $name -eq 'RightArrow' -or
                ($onAction -and ((Test-ClaudeHotkey -Key $key -Char 'a') -or (Test-ClaudeHotkey -Key $key -Char 'd')))) {
            $back = ($name -eq 'LeftArrow') -or (Test-ClaudeHotkey -Key $key -Char 'a')
            $action = Step-ProjectAction -Action $action -Delta $(if ($back) { -1 } else { 1 })
        }
        elseif ($name -eq 'Escape' -or ($key.Key -eq 'C' -and ($key.Modifiers -band [System.ConsoleModifiers]::Control))) { return $null }
        elseif ($name -eq 'Enter') { $r = & $pick $action;    if ($r) { return $r } else { $notice = 'path not found' } }
        # The hotkeys still fire immediately AND set the field: the press is the answer, and the
        # screen has to say what just happened - which matters most exactly when the pick is
        # REJECTED and the loop draws again with the field the press left behind.
        elseif (Test-ClaudeHotkey -Key $key -Char 'c') { $action = 'continue'; $r = & $pick $action; if ($r) { return $r } else { $notice = 'path not found' } }
        elseif (Test-ClaudeHotkey -Key $key -Char 'r') { $action = 'resume';   $r = & $pick $action; if ($r) { return $r } else { $notice = 'path not found' } }
        elseif (Test-ClaudeHotkey -Key $key -Char 't') { $action = 'worktree'; $r = & $pick $action; if ($r) { return $r } else { $notice = 'path not found' } }
        elseif (Test-ClaudeHotkey -Key $key -Char '/') { $typing = $true }
    }
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
    catch { $page = @() }
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
        [scriptblock]$Draw = {
            param($s, $i, $f, $sc, $pn)
            $map = $null
            Get-PickerFrame -Sessions $s -Index $i -Filter $f -Scope $sc -ProjectName $pn -RowMap ([ref]$map) | ForEach-Object { Write-Host $_ }
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
        [scriptblock]$FetchMore = $null
    )
    $index = 0
    $filter = ''
    $typing = $false
    $rowMap = $null
    # No slug AND no name means there is nothing to scope to (an unrecognised cwd, or a caller that
    # never learned a project at all) - the picker then behaves exactly as it always has, and Tab
    # does nothing (guarded below), because there is no "other" scope to widen from or narrow to.
    # 'none' (fix round 1, IMPORTANT 1), not 'all': Get-PickerFrame renders 'none' with NO tab hint
    # at all, where 'all' would show one labelled "this project" that Tab could never act on - a
    # button that always does nothing, wrapping the footer at 80 columns for every user who has
    # never even seen the project screen.
    # Empties filtered out: a caller passing -ProjectSlug '' means "no slug", and a one-element array
    # holding '' would otherwise read as a scope that matches nothing.
    $slugSet = @($ProjectSlug | Where-Object { $_ })
    $hasScope = ($slugSet.Count -gt 0) -or [bool]$ProjectName
    $scope = if ($hasScope) { 'project' } else { 'none' }

    # One page bucket PER SCOPE. The page the launcher handed in was fetched for the scope the
    # picker opens in; Tab is a different question of disk ("every session of this account", not
    # "the next ten of this project") and gets its own page 1 and its own paging offset. Keeping
    # both means Tab back and forth costs one fetch each way, not one per press.
    $slugsFor = { param([string]$S) if ($S -eq 'project') { $slugSet } else { @() } }
    $newBucket = {
        param([string]$S)
        $b = [pscustomobject]@{ Sessions = @($Sessions); Fetched = @($Sessions).Count; Exhausted = $true }
        if ($FetchMore) {
            $b = [pscustomobject]@{ Sessions = @(); Fetched = 0; Exhausted = $false }
            $g = Expand-SessionPage -Sessions @() -FetchMore $FetchMore -Fetched 0 -Scope (& $slugsFor $S)
            $b.Sessions = $g.Sessions; $b.Fetched = $g.Fetched; $b.Exhausted = $g.Exhausted
        }
        return $b
    }
    $pages = @{}
    $pages[$scope] = [pscustomobject]@{ Sessions = @($Sessions); Fetched = @($Sessions).Count; Exhausted = (-not $FetchMore) }

    while ($true) {
        if (-not $pages.ContainsKey($scope)) { $pages[$scope] = & $newBucket $scope }
        $bucket = $pages[$scope]
        # Scoped BEFORE Select-ResumableSessions/Select-SessionMatch run, so $items - and therefore
        # $index - only ever ranges over the sessions the current scope actually shows. $pool (not
        # $Sessions) is what gets handed to $Draw too, so Get-PickerFrame's own hiddenCount and "N
        # sessions" title reflect the scoped pool, never the full account.
        # @() wraps the WHOLE if/else, not just its branches: `$x = if (...) {...} else { @() }`
        # unwraps an empty-array branch to $null on assignment regardless of how that branch built
        # it - the exact trap the comment below already warns about, now one line earlier.
        $pool = @(
            if ($scope -eq 'project' -and $hasScope) {
                if ($slugSet.Count -gt 0) { $bucket.Sessions | Where-Object { $_.Slug -in $slugSet } }
                else { $bucket.Sessions | Where-Object { $_.Project -eq $ProjectName } }
            } else { $bucket.Sessions }
        )
        # Select-ResumableSessions drops empty (zero-prompt) sessions before the filter runs, and
        # Get-PickerFrame does the exact same thing before rendering - the two must never disagree
        # about which index points at which session.
        # @() is load-bearing even though Select-ResumableSessions already wraps ITS OWN return in
        # @(): a function returning zero items unrolls to $null on the pipeline regardless of how it
        # built that array internally, and Select-SessionMatch's -Sessions is Mandatory - an account
        # with every session filtered out (or none at all) crashed the picker here with a raw
        # PowerShell binding error. Found via tests\check-preview.ps1's empty-fixture-account run.
        $resumable = @(Select-ResumableSessions -Sessions $pool)
        $items = Select-SessionMatch -Sessions $resumable -Filter $filter
        # Paging stops only when the fetcher is out of rows, or when the filter can see NOTHING at
        # all - the case where every Down was a synchronous cold disk page that could not change the
        # frame (adversarial review 2026-09-16, D4). Gating on "the filter hides nothing" instead
        # (`$items.Count -ge $resumable.Count`) went too far: one hidden row stopped every fetch, so
        # a session matching the filter one page deeper was unreachable (re-review, W1).
        $canPage = [bool]$FetchMore -and -not $bucket.Exhausted -and -not ($items.Count -eq 0 -and $resumable.Count -gt 0)
        if ($index -ge $items.Count) { $index = [Math]::Max(0, $items.Count - 1) }
        $rowMap = & $Draw $pool $index $filter $scope $ProjectName
        $key = & $Wait
        if ("$key" -eq 'resize') { continue }

        # Mouse. Deliberately conservative about what opens a session: a single click only MOVES the
        # selection, and only a double click (or Enter) opens one. A stray click that launched a
        # session would be the kind of mistake nobody forgives, and the cost of the caution is one
        # extra click for people who want it.
        if ($key -and $key.Kind -eq 'mouse') {
            $synthetic = $null
            if ($key.WheelUp) { if ($index -gt 0) { $index-- } ; continue }
            if ($key.WheelDown) {
                if ($index -lt $items.Count - 1) { $index++ }
                elseif ($canPage) {
                    $grown = Expand-SessionPage -Sessions $bucket.Sessions -FetchMore $FetchMore -Fetched $bucket.Fetched -Scope (& $slugsFor $scope)
                    $bucket.Sessions = $grown.Sessions; $bucket.Fetched = $grown.Fetched; $bucket.Exhausted = $grown.Exhausted
                    if ($grown.Added -gt 0 -and $items.Count -gt 0) { $index++ }
                }
                continue
            }
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

        # WASD alongside the arrows here too - reached only outside the $typing branch above, so
        # letters typed into an open filter are never read as navigation.
        if ($name -eq 'UpArrow' -or (Test-ClaudeHotkey -Key $key -Char 'w')) { if ($index -gt 0) { $index-- } }
        elseif ($name -eq 'DownArrow' -or (Test-ClaudeHotkey -Key $key -Char 's')) {
            if ($index -lt $items.Count - 1) { $index++ }
            elseif ($canPage) {
                # The cursor is on the last row and there may be more behind it. $index is bumped
                # past the end on purpose: the appended rows still have to pass the scope and the
                # filter, and the loop re-clamps $index against $items before drawing, so this lands
                # on the first NEW visible row or stays put when the page added nothing visible.
                # Not bumped when the list was EMPTY: there was no row under the cursor to step off,
                # so the first fetched row would be skipped over (adversarial review 2026-09-16, D3).
                $grown = Expand-SessionPage -Sessions $bucket.Sessions -FetchMore $FetchMore -Fetched $bucket.Fetched -Scope (& $slugsFor $scope)
                $bucket.Sessions = $grown.Sessions; $bucket.Fetched = $grown.Fetched; $bucket.Exhausted = $grown.Exhausted
                if ($grown.Added -gt 0 -and $items.Count -gt 0) { $index++ }
            }
        }
        # Tab, not a letter: Test-ClaudeHotkey is for the Latin letters a footer hint advertises via
        # -Char, and every named key on this screen (Enter, Escape) is already matched on $name the
        # same way this is - never -eq alone, but through the SAME $name string every other named key
        # here uses. Guarded on $hasScope: with nothing to scope to, there is no "other" scope to
        # widen from or narrow to, so the key does nothing rather than toggling between two labels
        # that would both mean "everything".
        elseif ($name -eq 'Tab' -and $hasScope) {
            $scope = if ($scope -eq 'project') { 'all' } else { 'project' }
            $index = 0
        }
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
        [scriptblock]$Drain = $null,
        # When the terminal delivered the record now being handled. From the INPUT RECORD, never
        # from this loop's own clock: a redraw plus Get-ClaudeInstallInfo between two real presses
        # easily outlasts any threshold worth setting, while two characters of one paste arrive
        # microseconds apart however slow the screen is. $null - the keyboard-only ReadKey path -
        # leaves the guard inert rather than blocking a confirm the reader really did press twice.
        [scriptblock]$RecordTime = { try { Get-ClaudeInputRecordTime } catch { $null } },
        [int]$ConfirmMinMs = 150
    )
    $status = ''
    $rowMap = $null
    # The key whose confirm is armed, $null otherwise. State, never a parse of the status text: a
    # key interpolated into a regex ('.' or '[') either matched everything or threw, and a generic
    # '^confirm' let one key's warning confirm ANOTHER key's action. Any press consumes it.
    $pending = $null
    # When that confirm was armed. A PASTE is not two presses: this screen acts on every character
    # it is handed and ConfirmTwice was the only brake, so a pasted string containing 'pp' deleted
    # builds and 'ii' started a five-minute fleet reindex with nobody touching the keyboard.
    $pendingAt = $null
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
        $wasPendingAt = $pendingAt
        $pending = $null
        $pendingAt = $null
        # Too fast to be a second press. The armed key is RE-armed rather than cancelled, so a long
        # paste of the same letter is a stream of re-arms and never an action.
        $now = & $RecordTime
        $tooFast = ($null -ne $now -and $null -ne $wasPendingAt -and ($now - $wasPendingAt) -lt $ConfirmMinMs)
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
            if ($wasPending -ne 'p' -or $tooFast) { $pending = 'p'; $pendingAt = $now; $status = "confirm: press p again to delete all but the 2 newest builds" }
            else { $r = Remove-OldClaudeVersions -Keep 2; $status = "deleted $($r.Deleted.Count) builds, freed $('{0:N1}' -f ($r.FreedBytes / 1GB)) GB" }
        }
        # Configured actions. ConfirmTwice ones are confirmed first because they are SLOW - a menu
        # that freezes for minutes with no warning reads as a hung launcher; the confirm text says so.
        else {
            foreach ($a in $Actions) {
                if (-not (Test-ClaudeHotkey -Key $key -Char $a.Key)) { continue }
                if ($a.ConfirmTwice -and ($wasPending -ne $a.Key -or $tooFast)) {
                    $pending = $a.Key
                    $pendingAt = $now
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
