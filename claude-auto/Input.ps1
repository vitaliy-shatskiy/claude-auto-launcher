# Console input for the menu: keyboard, mouse and resize off ONE queue.
#
# Nothing here runs unless a real console is attached. Every entry point degrades to $null or
# $false rather than throwing, because the launcher must still start when there is no console at
# all - the nightly audit runs it with stdin redirected from NUL, and a menu that cannot draw is a
# far better outcome than a launcher that cannot launch.
#
# Why not [Console]::ReadKey: it cannot see the mouse by design (.NET filters every input record
# and discards anything that is not a KEY_EVENT), and [Console]::KeyAvailable silently DEQUEUES a
# mouse record while scanning for a key. So a hybrid is impossible - see ConsoleInput.cs.

# --- input trace ------------------------------------------------------------------------------
# Every raw record the menu reads, one line per record, BEFORE anything interprets or discards it.
# It exists because the hover-presses-a-key defect (2026-08-25) was diagnosed three times from
# screenshots and process trees and got it wrong twice: what a terminal actually delivers is not
# inferable from what the menu did with it.
#
# On by default and per-PID, because up to four launchers run at once here and the interesting
# session is always the one nobody thought to switch tracing on for. Capped, because a mouse move
# is a record and hovering produces thousands. `CLAUDE_AUTO_INPUT_TRACE=0` turns it off.
#
# SILENT, like Write-LauncherLog and for the same reason: check-launcher-regression.ps1 compares the
# launcher's console output against a stored reference, so one stray Write-Host reddens it.
# OFF unless CLAUDE_AUTO_INPUT_TRACE=1, and BUFFERED when on. Both were paid for: the first version
# was on by default and wrote one AppendAllText per record - open, write, close. A hover is a record,
# thousands a second, and the menu froze solid. A diagnostic that changes the behaviour it is meant
# to observe is worse than none, so tracing is now something the owner switches on for one run.
$script:TraceOn = ("$env:CLAUDE_AUTO_INPUT_TRACE" -eq '1')
$script:TracePath = $null
$script:TraceCount = 0
$script:TraceMax = 20000
$script:TraceBuf = $null
$script:TraceUtf8 = New-Object System.Text.UTF8Encoding($false)

function Get-ClaudeInputTracePath {
    if ($null -ne $script:TracePath) { return $script:TracePath }
    $script:TracePath = ''
    if ("$env:CLAUDE_AUTO_INPUT_TRACE" -ne '1') { return '' }
    try {
        $dir = Join-Path $HOME '.claude\claude-auto-logs'
        if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
        $script:TracePath = Join-Path $dir ('input-{0:yyyy-MM-dd}-{1}.log' -f (Get-Date), $PID)
        $script:TraceBuf = [System.Collections.Generic.List[string]]::new()
    } catch { $script:TracePath = '' }
    return $script:TracePath
}

function Flush-ClaudeInputTrace {
    # Called when the menu is IDLE (a read timed out), so the cost never lands between a keypress
    # and the frame that answers it.
    if (-not $script:TraceBuf -or $script:TraceBuf.Count -eq 0) { return }
    try {
        [IO.File]::AppendAllText($script:TracePath, (($script:TraceBuf -join "`r`n") + "`r`n"), $script:TraceUtf8)
    } catch { }
    $script:TraceBuf.Clear()
}

function Write-ClaudeInputTrace {
    param([Parameter(Mandatory)][string]$Line)
    if ($script:TraceCount -ge $script:TraceMax) { return }
    $p = Get-ClaudeInputTracePath
    if (-not $p) { return }
    try {
        $script:TraceBuf.Add(('{0:HH:mm:ss.fff} {1}' -f (Get-Date), $Line))
        $script:TraceCount++
        if ($script:TraceCount -ge $script:TraceMax) { $script:TraceBuf.Add('-- trace cap reached --') }
        if ($script:TraceBuf.Count -ge 256) { Flush-ClaudeInputTrace }
    } catch { }
}

function Format-ClaudeInputRecord {
    # A record as one greppable line. The character is shown as a code point AND as a glyph: the
    # whole question this file exists to answer is which BYTES arrive, and 'M' versus 'm' versus a
    # raw 0x20 is exactly the distinction a pretty-printer would lose.
    param($Record)
    switch ($Record.EventType) {
        ([ClaudeAuto.ConsoleInput]::KEY_EVENT) {
            $k = $Record.KeyEvent
            $u = [int]$k.UnicodeChar
            $glyph = if ($u -ge 33 -and $u -lt 127) { [char]$u } elseif ($u -eq 27) { 'ESC' } elseif ($u -eq 32) { 'SP' } else { '.' }
            return ('KEY down={0} vk={1,-3} ch=U+{2:X4} [{3}] cks=0x{4:X4}' -f $k.bKeyDown, $k.wVirtualKeyCode, $u, $glyph, $k.dwControlKeyState)
        }
        ([ClaudeAuto.ConsoleInput]::MOUSE_EVENT) {
            $m = $Record.MouseEvent
            return ('MOU x={0,-3} y={1,-3} btn=0x{2:X8} fl=0x{3:X4}' -f $m.dwMousePosition.X, $m.dwMousePosition.Y, $m.dwButtonState, $m.dwEventFlags)
        }
        ([ClaudeAuto.ConsoleInput]::WINDOW_BUFFER_SIZE_EVENT) {
            return ('SIZE {0}x{1}' -f $Record.WindowBufferSizeEvent.dwSize.X, $Record.WindowBufferSizeEvent.dwSize.Y)
        }
    }
    return ('OTHER type={0}' -f $Record.EventType)
}

$script:InputSourcePath = Join-Path $PSScriptRoot 'ConsoleInput.cs'
$script:InputAssemblyPath = Join-Path $PSScriptRoot 'ConsoleInput.dll'
$script:InputReady = $null

function Initialize-ClaudeConsoleInput {
    # Compile once, load by path forever after. Measured in a fresh pwsh: Add-Type from source
    # 184 ms, Add-Type -Path 31 ms, and the launcher is under a second in total - so the build is
    # cached on disk and only redone when the source is newer. The DLL is gitignored, which is why
    # the .cs is committed and this can always rebuild it.
    #
    # Up to four launchers can start at once here, so the build writes a temp file and MOVES it
    # into place: a half-written DLL would fail Add-Type -Path, and the fallback below would then
    # compile from source rather than leave the menu without input.
    if ($null -ne $script:InputReady) { return $script:InputReady }
    $script:InputReady = $false
    if (-not (Test-Path -LiteralPath $script:InputSourcePath)) { return $false }

    if (([System.Management.Automation.PSTypeName]'ClaudeAuto.ConsoleInput').Type) {
        $script:InputReady = $true
        return $true
    }

    $src = Get-Item -LiteralPath $script:InputSourcePath
    $dll = Get-Item -LiteralPath $script:InputAssemblyPath -ErrorAction SilentlyContinue
    $fresh = $dll -and $dll.LastWriteTimeUtc -ge $src.LastWriteTimeUtc

    if ($fresh) {
        try {
            Add-Type -Path $script:InputAssemblyPath -ErrorAction Stop
            $script:InputReady = $true
            return $true
        } catch { $fresh = $false }   # stale or truncated: fall through and rebuild
    }

    try {
        $text = Get-Content -LiteralPath $script:InputSourcePath -Raw
        $tmp = Join-Path ([IO.Path]::GetTempPath()) ("ConsoleInput-{0}.dll" -f [guid]::NewGuid().ToString('N').Substring(0, 8))
        Add-Type -TypeDefinition $text -Language CSharp -OutputAssembly $tmp -ErrorAction Stop
        # The move can FAIL for a reason that has nothing to do with this launcher: another one is
        # running and has the DLL loaded, so Windows refuses to replace it - `-Force` does not help,
        # the error is "Cannot create a file when that file already exists".
        $moved = $false
        try { Move-Item -LiteralPath $tmp -Destination $script:InputAssemblyPath -Force -ErrorAction Stop; $moved = $true } catch { }
        if (-not ([System.Management.Automation.PSTypeName]'ClaudeAuto.ConsoleInput').Type) {
            # Load the build we just MADE, never the file on disk when the move did not land.
            # Measured 2026-08-25 with two older launchers running: the old code fell back to the
            # path, loaded a ten-day-old assembly and returned $true, so the session was missing a
            # P/Invoke the source had just gained and the input drain became a silent no-op. A
            # cache that can serve a stale build while reporting success is worse than no cache.
            $load = if ($moved) { $script:InputAssemblyPath } else { $tmp }
            Add-Type -Path $load -ErrorAction Stop
        }
        $script:InputReady = $true
    } catch {
        $script:InputReady = $false
    }
    return $script:InputReady
}

function Open-ClaudeConsoleInput {
    # Arms the console for mouse input and returns the state needed to restore it. $null means
    # "no console here" - the caller keeps its keyboard-only path and says nothing.
    #
    # QUICK_EDIT must be CLEARED or the terminal keeps the mouse for text selection and the app
    # never sees a click. The cost is that the terminal's own selection is unavailable while the
    # menu is up; it comes back on restore, which is the reason restore is not optional.
    if (-not (Initialize-ClaudeConsoleInput)) { return $null }
    # The escape hatch, and the reason it exists: on a terminal that reports the mouse as TEXT the
    # menu is better off never asking for the mouse at all. $null here is the documented
    # keyboard-only path every screen already handles, so this costs a feature and never a launch.
    if ("$env:CLAUDE_AUTO_NO_MOUSE" -eq '1') { return $null }
    try {
        # GENERIC_WRITE is required, and not for writing input: SetConsoleMode on the INPUT buffer
        # fails on a read-only handle. Opened read-only, CreateFileW and GetConsoleMode both
        # succeed and only the arming step fails - which reads exactly like "no console here" and
        # sent the first version of this down the wrong path entirely.
        $h = [ClaudeAuto.ConsoleInput]::CreateFileW(
            'CONIN$',
            [ClaudeAuto.ConsoleInput]::GENERIC_READ -bor [ClaudeAuto.ConsoleInput]::GENERIC_WRITE,
            [ClaudeAuto.ConsoleInput]::FILE_SHARE_READ -bor [ClaudeAuto.ConsoleInput]::FILE_SHARE_WRITE,
            [IntPtr]::Zero, [ClaudeAuto.ConsoleInput]::OPEN_EXISTING, 0, [IntPtr]::Zero)
        if ($h -eq [IntPtr]::Zero -or $h -eq [IntPtr]::new(-1)) { return $null }

        [uint32]$mode = 0
        if (-not [ClaudeAuto.ConsoleInput]::GetConsoleMode($h, [ref]$mode)) {
            [void][ClaudeAuto.ConsoleInput]::CloseHandle($h)
            return $null
        }
        # PROCESSED_INPUT cleared here, and NOT in Enter-AltBuffer where it used to live. The alt
        # buffer is skipped whenever output is redirected (Test-AltBufferSupported), so
        # `claude-auto > log` armed QuickEdit-off with Ctrl+C still live - and Ctrl+C tears the
        # process down before the `finally` that restores this very console mode can run, leaving
        # the owner's terminal without its own text selection for the rest of the day. Arming and
        # neutralising Ctrl+C are one decision and now live in one place.
        # Read BEFORE arming. [Console]::TreatControlCAsInput is not a separate setting - it IS the
        # PROCESSED_INPUT bit of this same console mode, which the arming below clears. Read after,
        # the getter reports the value this function has just imposed, and the restore then puts
        # that back instead of what the process was found with.
        $tcc = $null
        try { $tcc = [Console]::TreatControlCAsInput } catch { }

        $armed = ($mode -bor [ClaudeAuto.ConsoleInput]::ENABLE_WINDOW_INPUT `
                        -bor [ClaudeAuto.ConsoleInput]::ENABLE_MOUSE_INPUT `
                        -bor [ClaudeAuto.ConsoleInput]::ENABLE_EXTENDED_FLAGS) `
                 -band (-bnot ([ClaudeAuto.ConsoleInput]::ENABLE_QUICK_EDIT_MODE -bor [ClaudeAuto.ConsoleInput]::ENABLE_PROCESSED_INPUT))
        if (-not [ClaudeAuto.ConsoleInput]::SetConsoleMode($h, $armed)) {
            [void][ClaudeAuto.ConsoleInput]::CloseHandle($h)
            return $null
        }
        # The keyboard-only [Console]::ReadKey path reads this flag rather than the console mode, so
        # both have to agree; $tcc above is what the restore puts back.
        try { [Console]::TreatControlCAsInput = $true } catch { }
        return [pscustomobject]@{
            Handle = $h; OriginalMode = $mode; ArmedMode = $armed; Closed = $false
            OriginalTreatControlC = $tcc
            # Set the first time a genuine MOUSE_EVENT record arrives. A console that sends those
            # must never be downgraded to the text protocol - see Switch-ClaudeMouseToText.
            SawConsoleMouse = $false
        }
    } catch { return $null }
}

function Close-ClaudeConsoleInput {
    # Idempotent on purpose: it is called from a finally block that can run twice on an unwind, and
    # restoring a mode twice is harmless while failing to restore it once leaves the owner's
    # terminal behaving differently for the rest of the day.
    param($State)
    if (-not $State -or $State.Closed) { return $true }
    $ok = $true
    # If we asked the TERMINAL for the mouse, we have to say goodbye in its language too - the
    # console mode restore below cannot reach a tracking mode we requested through the stream, and
    # leaving it on means the owner's shell receives mouse reports as garbage for the rest of the day.
    if ($State.TextMouse) {
        try { [Console]::Write("$([char]27)[?1006l$([char]27)[?1000l$([char]27)[?1002l$([char]27)[?1003l") } catch { }
    }
    try {
        $ok = [ClaudeAuto.ConsoleInput]::SetConsoleMode($State.Handle, $State.OriginalMode)
        # Verify the restore rather than trusting the return value - the whole reason this module
        # can be trusted is that the prototype asserted the mode came back bit-exact.
        [uint32]$now = 0
        if ([ClaudeAuto.ConsoleInput]::GetConsoleMode($State.Handle, [ref]$now)) { $ok = $ok -and ($now -eq $State.OriginalMode) }
        [void][ClaudeAuto.ConsoleInput]::CloseHandle($State.Handle)
    } catch { $ok = $false }
    if ($null -ne $State.OriginalTreatControlC) {
        try { [Console]::TreatControlCAsInput = [bool]$State.OriginalTreatControlC } catch { }
    }
    # Closed ONLY on success. It used to be set either way, which turned the second call - the one
    # the `finally` block makes on an unwind - into a no-op, so a failed restore was both silent and
    # unretryable. What it leaves behind is the reader's terminal with QuickEdit off for the rest of
    # the day, and nothing said so.
    if ($ok) { $State.Closed = $true }
    else {
        try { [Console]::Error.WriteLine('claude-auto: the console mode could not be restored; QuickEdit may still be off in this terminal') } catch { }
    }
    return $ok
}

function ConvertTo-ClaudeInputEvent {
    # One INPUT_RECORD -> what the menu loop consumes. A key becomes a real ConsoleKeyInfo so every
    # existing screen keeps working unchanged; a resize becomes the literal string the old
    # Wait-KeyOrResize already returned; a mouse event becomes an object the screens that do not
    # understand it will simply not match on.
    param($Record)
    switch ($Record.EventType) {
        ([ClaudeAuto.ConsoleInput]::KEY_EVENT) {
            $k = $Record.KeyEvent
            if ($k.bKeyDown -eq 0) { return $null }            # key-up is not an action
            if ($k.wVirtualKeyCode -in 16, 17, 18) { return $null }  # bare Shift/Ctrl/Alt is not a key press
            $cks = $k.dwControlKeyState
            $shift = [bool]($cks -band [ClaudeAuto.ConsoleInput]::SHIFT_PRESSED)
            $alt = [bool]($cks -band ([ClaudeAuto.ConsoleInput]::LEFT_ALT_PRESSED -bor [ClaudeAuto.ConsoleInput]::RIGHT_ALT_PRESSED))
            $ctrl = [bool]($cks -band ([ClaudeAuto.ConsoleInput]::LEFT_CTRL_PRESSED -bor [ClaudeAuto.ConsoleInput]::RIGHT_CTRL_PRESSED))
            return [System.ConsoleKeyInfo]::new([char]$k.UnicodeChar, [System.ConsoleKey]$k.wVirtualKeyCode, $shift, $alt, $ctrl)
        }
        ([ClaudeAuto.ConsoleInput]::WINDOW_BUFFER_SIZE_EVENT) { return 'resize' }
        ([ClaudeAuto.ConsoleInput]::MOUSE_EVENT) {
            $m = $Record.MouseEvent
            $flags = $m.dwEventFlags
            $wheel = if ($flags -band ([ClaudeAuto.ConsoleInput]::MOUSE_WHEELED -bor [ClaudeAuto.ConsoleInput]::MOUSE_HWHEELED)) {
                [ClaudeAuto.ConsoleInput]::WheelDelta($m.dwButtonState)
            } else { 0 }
            return [pscustomobject]@{
                Kind          = 'mouse'
                X             = [int]$m.dwMousePosition.X
                Y             = [int]$m.dwMousePosition.Y
                Buttons       = [uint32]$m.dwButtonState
                Left          = [bool]($m.dwButtonState -band [ClaudeAuto.ConsoleInput]::FROM_LEFT_1ST_BUTTON_PRESSED)
                Right         = [bool]($m.dwButtonState -band [ClaudeAuto.ConsoleInput]::RIGHTMOST_BUTTON_PRESSED)
                IsMove        = [bool]($flags -band [ClaudeAuto.ConsoleInput]::MOUSE_MOVED)
                IsDoubleClick = [bool]($flags -band [ClaudeAuto.ConsoleInput]::DOUBLE_CLICK)
                # Windows Terminal reports +-128, not the classic +-120, so only the SIGN is safe
                # to act on. Anything comparing against WHEEL_DELTA silently does nothing here.
                Wheel         = [int]$wheel
                WheelUp       = ($wheel -gt 0)
                WheelDown     = ($wheel -lt 0)
            }
        }
    }
    return $null
}

function Peek-ClaudeKeyChar {
    # The code point of the next PENDING key-down record, or $null when the next thing waiting is
    # not one (or nothing is waiting after a short bounded wait). Peeks only - the caller decides
    # whether to consume.
    #
    # Polls PeekConsoleInputW for about 5ms before giving up. PeekConsoleInputW is a single
    # non-blocking snapshot: a terminal that delivers a mouse report byte by byte - the real shape;
    # this machine's own input traces show 1-6ms between bytes of one report - lands the ESC in the
    # gap before the rest arrives, and a single instant peek right then reads an empty queue and
    # calls it a bare Escape. The payload loops below already wait (-TimeoutMs 2/4); this keeps the
    # introducer check no more trigger-happy than they are.
    #
    # No Start-Sleep in the loop: Windows' default timer resolution is commonly ~15.6ms, so a single
    # "sleep 1ms" iteration can itself burn the whole budget before the loop condition is even
    # re-checked - one coarse sleep and out, exactly the instant-peek bug this exists to fix. A tight
    # re-peek costs a few microseconds per iteration and is bounded by the Stopwatch regardless of
    # OS timer granularity.
    # SIXTEEN records, not one, and releases are SKIPPED rather than treated as an answer. A
    # one-record peek that bailed on record[0] being a release could not see past the ESC's OWN
    # key-up - and every payload loop in this file counts key-DOWNS precisely because the queue
    # interleaves releases. So the introducer sitting behind that release was invisible, the report
    # was not recognised as a sequence, and its characters reached the menu as keys: an X10 report
    # at column 85 is the byte 32+85 = 'u', which runs `claude update`, and column 82 is 'r', the
    # rename swap. For SGR the leading ESC reached the launch screen as Escape, so a click quit the
    # launcher. Measured 2026-09-08: the menu received '[M ur'.
    param($State, [int]$TimeoutMs = 5)
    try {
        $sw = [Diagnostics.Stopwatch]::StartNew()
        do {
            $buf = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD[]' 16
            [uint32]$n = 0
            if (-not [ClaudeAuto.ConsoleInput]::PeekConsoleInputW($State.Handle, $buf, 16, [ref]$n)) { return $null }
            for ($i = 0; $i -lt [int]$n; $i++) {
                if ($buf[$i].EventType -ne [ClaudeAuto.ConsoleInput]::KEY_EVENT) { continue }
                if ($buf[$i].KeyEvent.bKeyDown -eq 0) { continue }
                return [int]$buf[$i].KeyEvent.UnicodeChar
            }
        } while ($sw.Elapsed.TotalMilliseconds -lt $TimeoutMs)
        return $null
    } catch { return $null }
}

function Read-ClaudeKeyDown {
    # Consume records until a key-DOWN is taken, and return it. The counterpart of the peek above:
    # everything it skips to find the next key-down, this has to skip to consume the same one.
    # Bounded, so a queue of nothing but releases costs a few reads rather than the menu.
    param($State, [int]$TimeoutMs = 0, [int]$Max = 16)
    for ($i = 0; $i -lt $Max; $i++) {
        $rec = Read-ClaudeRawRecord -State $State -TimeoutMs $TimeoutMs
        if ($null -eq $rec) { return $null }
        if ($rec.EventType -eq [ClaudeAuto.ConsoleInput]::KEY_EVENT -and $rec.KeyEvent.bKeyDown -ne 0) { return $rec }
    }
    return $null
}

function ConvertFrom-ClaudeVtKey {
    # A cursor or navigation key delivered as VT TEXT -> the ConsoleKeyInfo every screen already
    # consumes. Nothing else maps: a mouse report has its own decoder, and a focus or device report
    # is not a key at all and stays swallowed.
    #
    # Both VT branches below used to consume to the final byte and return $true, which
    # Read-ClaudeInputEvent turns into $null - so on the very terminal the text-mouse path exists
    # for, up/down/left/right were dead keys while the footer advertised them as the only way to
    # change a value.
    #
    # $Sequence carries the introducer: '[A' (CSI) or 'OA' (SS3, which is what a terminal in
    # application-cursor mode sends for the same key).
    param([string]$Sequence)
    $map = @{
        'A' = [ConsoleKey]::UpArrow; 'B' = [ConsoleKey]::DownArrow
        'C' = [ConsoleKey]::RightArrow; 'D' = [ConsoleKey]::LeftArrow
        'H' = [ConsoleKey]::Home; 'F' = [ConsoleKey]::End
    }
    $key = $null
    if ($Sequence -match '^[\[O]([A-DHF])$') { $key = $map[$Matches[1]] }
    elseif ($Sequence -eq '[1~') { $key = [ConsoleKey]::Home }
    elseif ($Sequence -eq '[4~') { $key = [ConsoleKey]::End }
    if ($null -eq $key) { return }
    # KeyChar 0 on purpose: a cursor key carries no character, and one that did would be matched by
    # every hotkey comparison on the way up.
    return [System.ConsoleKeyInfo]::new([char]0, $key, $false, $false, $false)
}

function Read-ClaudeRawRecord {
    # One record off the queue, traced, or $null. The trace happens HERE so it records what the
    # terminal delivered, before any interpretation or discarding downstream.
    param($State, [int]$TimeoutMs)
    $w = [ClaudeAuto.ConsoleInput]::WaitForSingleObject($State.Handle, [uint32]$TimeoutMs)
    if ($w -ne [ClaudeAuto.ConsoleInput]::WAIT_OBJECT_0) { return $null }
    $buf = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD[]' 1
    [uint32]$read = 0
    if (-not [ClaudeAuto.ConsoleInput]::ReadConsoleInputW($State.Handle, $buf, 1, [ref]$read)) { return $null }
    if ($read -lt 1) { return $null }
    # The formatting is inside the guard, not only the write: building the line is a switch plus a
    # -f on every record, and "every record" means every pixel of mouse movement.
    if ($script:TraceOn) { Write-ClaudeInputTrace (Format-ClaudeInputRecord -Record $buf[0]) }
    # When the TERMINAL delivered it. The maintenance screen's confirm gate rests on this and not on
    # its own loop clock: a paste arrives as one burst however slow the screen is, while a redraw
    # plus Get-ClaudeInstallInfo between two real presses easily outlasts any threshold worth
    # setting. Stamped here, where the record actually arrives.
    $script:LastRecordMs = [Environment]::TickCount64
    # Caps Lock travels beside the record, not inside the ConsoleKeyInfo, because that type has no
    # room for it - it carries Shift, Alt and Ctrl only. The hotkey matcher reads it in the same
    # loop iteration the record was read in, which is the only moment it means anything.
    if ($buf[0].EventType -eq [ClaudeAuto.ConsoleInput]::KEY_EVENT) {
        $script:LastRecordCapsLock = [bool]($buf[0].KeyEvent.dwControlKeyState -band [ClaudeAuto.ConsoleInput]::CAPSLOCK_ON)
    }
    # A genuine MOUSE_EVENT record proves this console does not need the text protocol, and the
    # downgrade to it is one-way.
    if ($buf[0].EventType -eq [ClaudeAuto.ConsoleInput]::MOUSE_EVENT -and $State) {
        try { $State.SawConsoleMouse = $true } catch { }
    }
    return $buf[0]
}

function Get-ClaudeInputCapsLock {
    # Whether the last key record arrived with Caps Lock on. $false until one has.
    return [bool]$script:LastRecordCapsLock
}

function Get-ClaudeInputRecordTime {
    # $null until the first record is read, so a confirm gate with nothing to compare against stays
    # inert rather than inventing a stamp.
    return $script:LastRecordMs
}

function ConvertFrom-ClaudeMouseReport {
    # A decoded VT mouse report, in exactly the shape ConvertTo-ClaudeInputEvent produces for a
    # MOUSE_EVENT record, so every screen consumes it without knowing which terminal it came from.
    #
    # This is the whole reason Claude Code has a working mouse in a terminal where this launcher did
    # not: it reads the report, we asked Win32 for a record that never comes.
    #
    # $Button is the protocol's button byte. Low two bits pick the button, 32 is motion, 64 is the
    # wheel (low bit 0 = up, 1 = down). Coordinates are 1-BASED in the protocol and every screen
    # here works in 0-based buffer columns.
    param([int]$Button, [int]$X, [int]$Y, [switch]$Released)
    $wheel = if ($Button -band 64) { if ($Button -band 1) { -1 } else { 1 } } else { 0 }
    $isMove = [bool]($Button -band 32)
    $low = $Button -band 3
    $down = (-not $Released) -and ($wheel -eq 0)
    return [pscustomobject]@{
        Kind          = 'mouse'
        X             = [int]($X - 1)
        Y             = [int]($Y - 1)
        Buttons       = [uint32]$Button
        Left          = ($down -and $low -eq 0)
        Right         = ($down -and $low -eq 2)
        IsMove        = $isMove
        IsDoubleClick = $false      # the protocol has no double-click; a screen that wants one counts
        Wheel         = $wheel
        WheelUp       = ($wheel -gt 0)
        WheelDown     = ($wheel -lt 0)
    }
}

function Switch-ClaudeMouseToText {
    # Take the mouse off the CONSOLE and ask the TERMINAL for it directly.
    #
    # The volume is the whole problem. `ENABLE_MOUSE_INPUT` is what makes the pty request any-event
    # tracking, so every pixel of movement became six to twelve records; at ~0.5 ms per record
    # through this reader a moving hand fills the queue faster than the menu drains it, and the
    # launcher sat frozen with a backlog that only grew. Clearing that flag stops the flood at its
    # source, and `?1000h` asks for presses and releases ONLY - a click is two reports, not two
    # hundred. `?1006h` adds SGR encoding, which is also what makes a click past column 95 readable
    # at all: X10 spends a single byte on the coordinate.
    #
    # Idempotent: the read path sees many reports before the first switch lands.
    # Two guards, because the downgrade is permanent for the session and one report-shaped sequence
    # is enough to trigger it - a paste of `ESC[<0;1;1M` will do. A console that has already
    # delivered real MOUSE_EVENT records does not need the text protocol and must keep its mouse.
    # And with stdout redirected the compensating ?1000h/?1006h lands in the redirect FILE, so the
    # terminal is left with no mouse at all and no request to give it one.
    param(
        $State,
        [bool]$OutputRedirected = [Console]::IsOutputRedirected,
        [bool]$SawConsoleMouse = $(if ($State) { [bool]$State.SawConsoleMouse } else { $false })
    )
    if (-not $State -or $State.Closed -or $State.TextMouse) { return $false }
    if ($OutputRedirected -or $SawConsoleMouse) { return $false }
    try {
        $mode = $State.ArmedMode -band (-bnot [ClaudeAuto.ConsoleInput]::ENABLE_MOUSE_INPUT)
        $null = [ClaudeAuto.ConsoleInput]::SetConsoleMode($State.Handle, $mode)
        $State | Add-Member -NotePropertyName TextMouse -NotePropertyValue $true -Force
        # Motion off first, then ask for presses in SGR. Order matters only in that leaving 1003 on
        # would keep the flood alive whatever else is requested.
        [Console]::Write("$([char]27)[?1003l$([char]27)[?1002l$([char]27)[?1000h$([char]27)[?1006h")
        # Whatever the hand queued BEFORE the first report was read is still sitting there, and at
        # ~0.5 ms a record a couple of seconds of hovering is thousands of them - the menu would
        # spend that long chewing a backlog from the mode it has just left. Turning the tap down
        # does not empty the bucket.
        $null = [ClaudeAuto.ConsoleInput]::FlushConsoleInputBuffer($State.Handle)
        if ($script:TraceOn) { Write-ClaudeInputTrace 'MOUSE via TEXT - console mouse off, SGR press/release requested' }
        return $true
    } catch { return $false }
}

function Skip-ClaudeVtSequence {
    # An escape sequence is ONE event, never N keystrokes. A terminal that reports the mouse as TEXT
    # rather than as MOUSE_EVENT records (Rider's terminal tab does; a real console does not) sends
    # ESC [ < b ; x ; y M for every hover, and the menu used to execute each character as a menu key.
    # Case-sensitivity was NOT the defence: the classic X10 encoding writes the byte 32+coordinate,
    # so column 85 arrives as a literal 'u' - the update key - and column 82 as 'r', the rename swap.
    # The second reading of the hover defect, after -ceq alone left `u` still firing.
    #
    # Swallowing is only half the answer, though: it stops the terminal pressing menu keys and takes
    # the mouse away with it. A mouse REPORT is decoded and handed back as an ordinary mouse event,
    # which is how Claude Code has a working mouse in the very same tab - it reads the report where
    # this launcher was asking Win32 for a record that never comes.
    #
    # Called with the ESC already consumed. Returns $false when the ESC was a real Escape and
    # nothing was touched (which is why the lookahead PEEKS), $true when a sequence was swallowed,
    # or a decoded mouse event when the sequence was a mouse report.
    param($State)
    $next = Peek-ClaudeKeyChar -State $State
    if ($null -eq $next) { return $false }
    # CSI (0x5B '[') and SS3 (0x4F 'O') are the two introducers a terminal uses here.
    if ($next -ne 0x5B -and $next -ne 0x4F) { return $false }
    $eaten = @()
    # Consume THE INTRODUCER, not "one record". The peek above looks past key-ups and stray mouse
    # records to find it; a blind single read then ate the ESC's own key-up and left the introducer
    # sitting in the queue, so the scan below started one byte late and the report leaked into the
    # menu exactly as before. Peek and consume have to agree about what they are skipping.
    $null = Read-ClaudeKeyDown -State $State
    $eaten += [char]$next
    if ($next -eq 0x4F) {
        # SS3: exactly one byte follows. Counted the same way the X10 branch below counts to four -
        # the queue can interleave a key-UP with the final byte (ESC, O-down, O-up, u-down is a real
        # shape: application-keypad mode maps SS3 finals to p-y, so keypad-5 is ESC O u and
        # keypad-2 is ESC O r), and a blind single read after the introducer can consume that O-up
        # instead of the real final byte, leaving it to reach the menu as an ordinary hotkey.
        $downs = 0
        $final = 0
        for ($i = 0; $i -lt 24 -and $downs -lt 1; $i++) {
            $rec = Read-ClaudeRawRecord -State $State -TimeoutMs 4
            if ($null -eq $rec) { break }
            if ($rec.EventType -eq [ClaudeAuto.ConsoleInput]::KEY_EVENT -and $rec.KeyEvent.bKeyDown -ne 0) {
                $downs++
                $final = [int]$rec.KeyEvent.UnicodeChar
            }
        }
        # An arrow in application-cursor mode arrives exactly here. Swallowing it was the whole of
        # the old behaviour, which left the four keys the footer advertises doing nothing at all.
        $vtKey = ConvertFrom-ClaudeVtKey -Sequence ('O' + [char]$final)
        if ($vtKey) {
            if ($script:TraceOn) { Write-ClaudeInputTrace ("VT KEY ESC O$([char]$final)") }
            return $vtKey
        }
        if ($script:TraceOn) { Write-ClaudeInputTrace 'SWALLOW vt ESC O + 1 byte' }
        return $true
    }
    # X10 / normal tracking puts the final byte FIRST and follows it with three RAW coordinate
    # bytes. Stopping at 'M' the way a parameter scan would leaves exactly the three bytes that are
    # letters at ordinary terminal widths - which is the shape that kept pressing update.
    $after = Peek-ClaudeKeyChar -State $State
    if ($after -eq 0x4D) {
        # Count key-DOWNS, not records. The queue interleaves key-up records with the sequence -
        # the 2026-08-25 trace shows M, C, C-up, f - so a blind read of four records consumed one
        # up and left a coordinate byte behind. That byte is a lowercase letter at ordinary widths,
        # which is the entire defect. Four downs: the final M plus the three coordinate bytes.
        $bytes = @()
        for ($i = 0; $i -lt 24 -and $bytes.Count -lt 4; $i++) {
            $rec = Read-ClaudeRawRecord -State $State -TimeoutMs 4
            if ($null -eq $rec) { break }
            if ($rec.EventType -eq [ClaudeAuto.ConsoleInput]::KEY_EVENT -and $rec.KeyEvent.bKeyDown -ne 0) {
                $bytes += [int]$rec.KeyEvent.UnicodeChar
            }
        }
        if ($script:TraceOn) { Write-ClaudeInputTrace 'MOUSE X10 report' }
        # A mouse report arriving as text is proof this terminal will never send MOUSE_EVENT
        # records. Ask it for SGR press/release instead: the button-and-motion flood ConPTY was
        # requesting is what the menu could not drain, and SGR is unambiguous above column 95 where
        # X10's single byte runs out. AFTER the report is decoded - the switch flushes the queue.
        $decoded = if ($bytes.Count -eq 4) {
            ConvertFrom-ClaudeMouseReport -Button ($bytes[1] - 32) -X ($bytes[2] - 32) -Y ($bytes[3] - 32)
        } else { $null }
        $null = Switch-ClaudeMouseToText -State $State
        if ($decoded) { return $decoded }
        return $true
    }
    # Scan parameters to a final byte in 0x40-0x7E. Bounded, so a truncated sequence
    # costs a few discarded characters rather than the menu.
    for ($i = 0; $i -lt 64; $i++) {
        $rec = Read-ClaudeRawRecord -State $State -TimeoutMs 2
        if ($null -eq $rec) { break }
        if ($rec.EventType -ne [ClaudeAuto.ConsoleInput]::KEY_EVENT -or $rec.KeyEvent.bKeyDown -eq 0) { continue }
        $c = [int]$rec.KeyEvent.UnicodeChar
        $eaten += [char]$c
        if ($c -ge 0x40 -and $c -le 0x7E) { break }
    }
    $seq = ($eaten -join '')
    # SGR: `<b;x;yM` (press or motion) or `<b;x;ym` (release). Anything else - an arrow key, a
    # focus event, a device report - is not ours and stays swallowed.
    # $seq carries the introducer, so the match starts at `[` - `$eaten` was seeded with it.
    $m = [regex]::Match($seq, '^\[<(\d+);(\d+);(\d+)([Mm])$')
    if ($m.Success) {
        if ($script:TraceOn) { Write-ClaudeInputTrace ("MOUSE SGR $seq") }
        # AFTER the report is parsed, never before: the switch flushes the input queue, and doing it
        # first threw away the parameters of the very report being read.
        $null = Switch-ClaudeMouseToText -State $State
        return (ConvertFrom-ClaudeMouseReport -Button ([int]$m.Groups[1].Value) `
                    -X ([int]$m.Groups[2].Value) -Y ([int]$m.Groups[3].Value) `
                    -Released:($m.Groups[4].Value -ceq 'm'))
    }
    # Not a mouse report: it may still be a KEY the terminal chose to spell out. Anything that maps
    # to nothing - a focus report, a device attributes answer - stays swallowed, which is what keeps
    # the bounded scan from becoming a leak.
    $vtKey = ConvertFrom-ClaudeVtKey -Sequence $seq
    if ($vtKey) {
        if ($script:TraceOn) { Write-ClaudeInputTrace ("VT KEY ESC $seq") }
        return $vtKey
    }
    if ($script:TraceOn) { Write-ClaudeInputTrace ('SWALLOW vt ESC ' + $seq) }
    return $true
}

function Read-ClaudeInputEvent {
    # Waits up to $TimeoutMs and returns ONE event, or $null on timeout. Never blocks in native
    # code for longer than the timeout, so the caller keeps its periodic tick - which is what makes
    # a resize visible and Ctrl+C responsive.
    param($State, [int]$TimeoutMs = 60)
    if (-not $State -or $State.Closed) { return $null }
    try {
        $rec = Read-ClaudeRawRecord -State $State -TimeoutMs $TimeoutMs
        if ($null -eq $rec) { return $null }
        # An ESC that introduces a sequence is not the Escape key. Decided by PEEKING at what waits
        # behind it, so a bare Escape - still how the owner leaves every screen - is untouched.
        if ($rec.EventType -eq [ClaudeAuto.ConsoleInput]::KEY_EVENT -and
            $rec.KeyEvent.bKeyDown -ne 0 -and [int]$rec.KeyEvent.UnicodeChar -eq 27) {
            $vt = Skip-ClaudeVtSequence -State $State
            # Three outcomes, and $false must NOT be confused with "swallowed nothing useful":
            # $false means this was a real Escape and the record below is the key to return.
            if ($vt -is [bool]) { if ($vt) { return $null } }
            else { return $vt }     # a decoded mouse report
        }
        return ConvertTo-ClaudeInputEvent -Record $rec
    } catch { return $null }
}

function Clear-ClaudeInputQueue {
    # Everything the terminal queued WHILE a child command owned the screen is noise behind a process
    # that took ten seconds - not an instruction to this menu. Replaying it is how a single hover
    # became `claude mcp list` over and over: an SGR mouse report ends in an uppercase letter, and
    # PowerShell's -eq matched it against the lowercase footer hint (live 2026-08-25).
    # Returns $true when the queue was flushed, $false when there was nothing to flush it on - never
    # throws, because a menu that cannot drain is still a menu.
    param($State)
    if (-not $State -or $State.Closed) { return $false }
    try { return [ClaudeAuto.ConsoleInput]::FlushConsoleInputBuffer($State.Handle) } catch { return $false }
}

function New-SyntheticKey {
    # A click on a footer hint becomes the KEY that hint advertises, and then takes the ordinary
    # keyboard path through the screen's own handlers. Performing the action here instead would be
    # a second implementation of every screen's semantics, and the two would drift the first time
    # one of them changed - which is the same reason a clicked option cell is reached by stepping
    # rather than by assignment.
    param([string]$Key, [string]$Char)
    $ck = [System.ConsoleKey]0
    if ($Key) { $null = [enum]::TryParse([System.ConsoleKey], $Key, [ref]$ck) }
    $c = if ($Char) { [char]$Char } else { [char]0 }
    return [System.ConsoleKeyInfo]::new($c, $ck, $false, $false, $false)
}

function Get-ClaudeFooterHit {
    # Which footer hint, if any, a click landed on. Returns $null for a click anywhere else, so the
    # caller can fall through to its row handling without a special case.
    #
    # A footer may wrap onto several lines on a narrow terminal (New-HintFooter -Width): FooterY is
    # the FIRST footer line and every span says which line it sits on. A span without a Line
    # (hand-built row maps) is on the first one.
    param($RowMap, [int]$X, [int]$Y, [int]$WindowTop = 0)
    if (-not $RowMap -or $null -eq $RowMap.FooterY) { return $null }
    $line = ($Y - $WindowTop) - $RowMap.FooterY
    if ($line -lt 0) { return $null }
    $hit = @($RowMap.Footer | Where-Object {
        $onLine = if ($null -ne $_.Line) { [int]$_.Line } else { 0 }
        $onLine -eq $line -and $X -ge $_.Start -and $X -le $_.End
    })
    if ($hit.Count -eq 0) { return $null }
    return $hit[0]
}

# The lowercase letter the SAME physical key produces on the Cyrillic (Russian and Ukrainian)
# layouts, per Latin hotkey. Both layouts agree on every key used here. Code points, not literals:
# the file stays ASCII, and a literal does not survive a re-encoding round trip reliably.
$script:HotkeyLayoutChars = @{
    'u' = [char]0x0433   # CYRILLIC SMALL LETTER GHE
    'r' = [char]0x043A   # CYRILLIC SMALL LETTER KA
    'd' = [char]0x0432   # CYRILLIC SMALL LETTER VE
    'm' = [char]0x044C   # CYRILLIC SMALL LETTER SOFT SIGN
    'p' = [char]0x0437   # CYRILLIC SMALL LETTER ZE
    'i' = [char]0x0448   # CYRILLIC SMALL LETTER SHA
    'f' = [char]0x0430   # CYRILLIC SMALL LETTER A
    '/' = [char]0x002E   # the slash key is '.' unshifted on both layouts
}
# Hotkeys whose virtual key is not the uppercase Latin letter.
$script:HotkeyVirtualKeys = @{ '/' = [System.ConsoleKey]::Oem2 }

function Test-ClaudeHotkey {
    # Does this key press mean the Latin hotkey a footer hint advertises, on ANY keyboard layout?
    # Three ways to say yes: the character itself (Latin layout, a footer click, a scripted key);
    # the VIRTUAL key - the physical key is the same whatever the layout paints on it, which is what
    # a real console reports; or the Cyrillic letter that key produces, for input that carries no
    # virtual key at all (VK_PACKET: an RDP client's soft keyboard, a paste).
    #
    # Two guards, and BOTH stay: a modifier (Shift, Ctrl, Alt) or an UPPERCASE character is never a
    # hotkey. -ceq alone was the first hover defence - the 'M' that ends an SGR mouse report reaches
    # the queue as vk=M with SHIFT set through ConPTY, and the case check catches a terminal that
    # forgets the flag. A virtual-key match without those guards would reopen that hole.
    #
    # CAPS LOCK is the exception, and it is not a hole. Reported live 2026-09-09: with Caps Lock on
    # the maintenance screen would not open, because `u` arrives as 'U' with NO Shift and the
    # uppercase guard rejected the owner's real keypress before the virtual-key match could see it.
    # A genuine press carries CAPSLOCK_ON in the record's control-key state; a byte from a mouse
    # report does not. So the flag lifts the CASE guard only - the modifier guard above still runs
    # first, which is what actually stops the shifted 'M'.
    param([Parameter(Mandatory)]$Key, [Parameter(Mandatory)][string]$Char, [bool]$CapsLock = (Get-ClaudeInputCapsLock))
    if ($null -eq $Key -or $Key -is [string]) { return $false }
    if ($Key.Kind -eq 'mouse') { return $false }
    $ch = "$($Key.KeyChar)"
    if ($ch -ceq $Char) { return $true }
    $mods = [int]$Key.Modifiers
    $blocked = [int][System.ConsoleModifiers]::Shift -bor [int][System.ConsoleModifiers]::Control -bor [int][System.ConsoleModifiers]::Alt
    if ($mods -band $blocked) { return $false }
    if (-not $CapsLock -and $ch.Length -gt 0 -and [char]::IsUpper($ch[0])) { return $false }
    $vk =
        if ($script:HotkeyVirtualKeys.ContainsKey($Char)) { $script:HotkeyVirtualKeys[$Char] }
        elseif ($Char -cmatch '^[a-z]$') { [System.ConsoleKey]([int][char]$Char.ToUpperInvariant()) }
        # Digit maintenance action keys (Config.ps1 accepts [a-z0-9]): the top row's virtual keys are
        # ConsoleKey.D0..D9. No Cyrillic fallback is needed the way letters get one - both the
        # Russian and Ukrainian layouts type the same 0-9 characters on that row unshifted, so the
        # character-only match above already covers a Cyrillic-layout keypress.
        elseif ($Char -cmatch '^[0-9]$') { [System.ConsoleKey]"D$Char" }
        else { $null }
    if ($null -ne $vk -and $Key.Key -eq $vk) { return $true }
    $layout = $script:HotkeyLayoutChars[$Char]
    if ($null -ne $layout -and $ch -ceq "$layout") { return $true }
    return $false
}

function Get-ClaudeMouseRow {
    # A click lands in SCREEN-BUFFER coordinates, not window-relative ones, and the menu is drawn
    # relative to the visible window. Subtracting the window's top is what makes a click on the
    # third visible row mean the third row, whatever the buffer has scrolled to.
    #
    # Returns $null when the click is outside the rows, so a caller can ignore it rather than
    # selecting something the owner did not point at.
    param([int]$Y, [int]$FirstRowY, [int]$RowCount, [int]$WindowTop = 0)
    if ($RowCount -le 0) { return $null }
    $row = $Y - $WindowTop - $FirstRowY
    if ($row -lt 0 -or $row -ge $RowCount) { return $null }
    return $row
}
