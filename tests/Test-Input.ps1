# Assertions for Input.ps1. Run: pwsh -File Test-Input.ps1
#
# Two halves. The PURE half - hit-testing maths and record translation - runs anywhere and must
# always pass. The LIVE half arms the real console and injects records with WriteConsoleInputW,
# which needs a console to exist.
#
# The process this suite usually runs in has NO console (an agent's shell, the nightly audit), so
# the live half would be reported as not-verified forever - assertions that exist and never run.
# Instead it re-launches ITSELF hidden with -LiveOnly, which gives the child its own console, and
# uses the child's exit code. "The suite was green" then means the same thing everywhere.
param([switch]$LiveOnly, [switch]$Live)

try { . "$PSScriptRoot\..\claude-auto\Input.ps1" } catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }

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
    } else { Write-Host "ok    $Because" }
}

# ---------------------------------------------------------------- hit-testing

# A click is only meaningful if it maps to the row the owner pointed at. Every boundary is asserted
# because an off-by-one here selects the wrong session, which is worse than ignoring the click.
Assert-Equal 0    (Get-ClaudeMouseRow -Y 5  -FirstRowY 5 -RowCount 4) 'a click on the first row is row 0'
Assert-Equal 3    (Get-ClaudeMouseRow -Y 8  -FirstRowY 5 -RowCount 4) 'a click on the last row is the last index'
Assert-Equal ''   (Get-ClaudeMouseRow -Y 4  -FirstRowY 5 -RowCount 4) 'a click above the rows selects nothing'
Assert-Equal ''   (Get-ClaudeMouseRow -Y 9  -FirstRowY 5 -RowCount 4) 'a click below the rows selects nothing'
Assert-Equal ''   (Get-ClaudeMouseRow -Y 5  -FirstRowY 5 -RowCount 0) 'no rows means no selection'
# Buffer coordinates, not window coordinates: the same screen position means a different buffer Y
# once the terminal has scrolled, and forgetting WindowTop is how a click lands rows away.
Assert-Equal 1    (Get-ClaudeMouseRow -Y 106 -FirstRowY 5 -RowCount 4 -WindowTop 100) 'the window top is subtracted before mapping'
Assert-Equal ''   (Get-ClaudeMouseRow -Y 6   -FirstRowY 5 -RowCount 4 -WindowTop 100) 'a stale buffer coordinate does not wrap into range'

# ---------------------------------------------------------------- record translation

if (-not (Initialize-ClaudeConsoleInput)) {
    Write-Host "COULD NOT RUN: the interop assembly would not build or load"
    exit 2
}

function New-KeyRec([int]$Down, [uint16]$Vk, [uint16]$Char, [uint32]$Cks = 0) {
    $k = New-Object 'ClaudeAuto.ConsoleInput+KEY_EVENT_RECORD'
    $k.bKeyDown = $Down; $k.wRepeatCount = 1; $k.wVirtualKeyCode = $Vk; $k.wVirtualScanCode = 0
    $k.UnicodeChar = $Char; $k.dwControlKeyState = $Cks
    $r = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD'
    $r.EventType = [ClaudeAuto.ConsoleInput]::KEY_EVENT
    $r.KeyEvent = $k
    return $r
}
function New-MouseRec([int16]$X, [int16]$Y, [uint32]$Buttons, [uint32]$Flags) {
    # Built bottom-up and assigned whole. Chained assignment through a struct field
    # ($r.MouseEvent.dwMousePosition.X = 1) silently mutates a boxed temporary in PowerShell and
    # never reaches the record - no error, every field left at zero. That trap cost the prototype
    # a hung test.
    $c = New-Object 'ClaudeAuto.ConsoleInput+COORD'; $c.X = $X; $c.Y = $Y
    $m = New-Object 'ClaudeAuto.ConsoleInput+MOUSE_EVENT_RECORD'
    $m.dwMousePosition = $c; $m.dwButtonState = $Buttons; $m.dwControlKeyState = 0; $m.dwEventFlags = $Flags
    $r = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD'
    $r.EventType = [ClaudeAuto.ConsoleInput]::MOUSE_EVENT
    $r.MouseEvent = $m
    return $r
}
function New-ResizeRec {
    $r = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD'
    $r.EventType = [ClaudeAuto.ConsoleInput]::WINDOW_BUFFER_SIZE_EVENT
    return $r
}

$k = ConvertTo-ClaudeInputEvent -Record (New-KeyRec 1 0x51 0x71)
Assert-Equal 'q'  "$($k.KeyChar)"  'a key press becomes a ConsoleKeyInfo carrying its character'
Assert-Equal 'Q'  "$($k.Key)"      'and its ConsoleKey'
Assert-Equal ''   (ConvertTo-ClaudeInputEvent -Record (New-KeyRec 0 0x51 0x71)) 'a key RELEASE is not an action'
Assert-Equal ''   (ConvertTo-ClaudeInputEvent -Record (New-KeyRec 1 16 0))      'a bare Shift is not a key press'
Assert-Equal ''   (ConvertTo-ClaudeInputEvent -Record (New-KeyRec 1 17 0))      'a bare Ctrl is not a key press'
$ctrlC = ConvertTo-ClaudeInputEvent -Record (New-KeyRec 1 0x43 0x03 ([ClaudeAuto.ConsoleInput]::LEFT_CTRL_PRESSED))
# [bool] is load bearing: -band on a flags enum returns the ENUM member, not $true, so comparing it
# to $true fails while the code is perfectly correct.
Assert-Equal $true ([bool]($ctrlC.Modifiers -band [System.ConsoleModifiers]::Control)) 'Ctrl+C keeps its Control modifier, which is how every screen exits'

Assert-Equal 'resize' (ConvertTo-ClaudeInputEvent -Record (New-ResizeRec)) 'a buffer-size event keeps the string the old reader returned'

$move = ConvertTo-ClaudeInputEvent -Record (New-MouseRec 12 34 0 ([ClaudeAuto.ConsoleInput]::MOUSE_MOVED))
Assert-Equal 'mouse' $move.Kind    'a mouse record becomes a mouse event'
Assert-Equal 12 $move.X            'the column survives'
Assert-Equal 34 $move.Y            'the row survives'
Assert-Equal $true $move.IsMove    'a move is flagged as a move'
Assert-Equal $false $move.Left     'a move with no button held reports no button'

$click = ConvertTo-ClaudeInputEvent -Record (New-MouseRec 5 7 ([ClaudeAuto.ConsoleInput]::FROM_LEFT_1ST_BUTTON_PRESSED) 0)
Assert-Equal $true  $click.Left          'a left press is reported'
Assert-Equal $false $click.IsMove        'a press is not a move'
Assert-Equal $false $click.IsDoubleClick 'a single press is not a double click'
$dbl = ConvertTo-ClaudeInputEvent -Record (New-MouseRec 5 7 ([ClaudeAuto.ConsoleInput]::FROM_LEFT_1ST_BUTTON_PRESSED) ([ClaudeAuto.ConsoleInput]::DOUBLE_CLICK))
Assert-Equal $true $dbl.IsDoubleClick    'a double click is distinguishable from a press'

# THE assertion for the wheel. The amount is a SIGNED 16-bit value in the HIGH word of
# dwButtonState; read as unsigned, every scroll looks like "up" and a scrollable menu only ever
# moves one way. Windows Terminal was measured emitting +-128 rather than the classic +-120, so the
# sign is the only part safe to act on.
#
# The value is BUILT, not typed as a literal: in PowerShell a hex literal above 0x7FFFFFFF is a
# NEGATIVE Int32, so `0xFF800000` is -8388608 and will not convert to the uint32 the API takes. The
# first version of this assertion failed on exactly that and looked like a bug in the decoder.
$wheelDown = [Convert]::ToUInt32('FF800000', 16)
$wheelUp = [Convert]::ToUInt32('00800000', 16)
Assert-Equal 128  ([ClaudeAuto.ConsoleInput]::WheelDelta($wheelUp))   'a positive wheel amount decodes as positive'
Assert-Equal -128 ([ClaudeAuto.ConsoleInput]::WheelDelta($wheelDown)) 'a NEGATIVE wheel amount decodes as negative, not as a huge positive'
$up = ConvertTo-ClaudeInputEvent -Record (New-MouseRec 1 1 $wheelUp ([ClaudeAuto.ConsoleInput]::MOUSE_WHEELED))
$down = ConvertTo-ClaudeInputEvent -Record (New-MouseRec 1 1 $wheelDown ([ClaudeAuto.ConsoleInput]::MOUSE_WHEELED))
Assert-Equal $true $up.WheelUp     'scrolling up is reported as up'
Assert-Equal $true $down.WheelDown 'scrolling down is reported as DOWN'
Assert-Equal $false $up.WheelDown  'up is not also down'

# ---------------------------------------------------------------- live console

if ($Live -and -not $LiveOnly) {
    # ALWAYS in a child, never in the ambient console. Measured: injecting a record with
    # WriteConsoleInputW and reading it back works in a fresh hidden console but silently returns
    # nothing in the console this suite usually runs under - something else there drains the input
    # queue first. That is an artefact of the surroundings, not of the code, and the way to keep
    # the assertion meaningful is to give it a console nobody else is reading.
    Write-Host "…  running the live half in a hidden child with its own console"
    $child = Start-Process -FilePath (Get-Process -Id $PID).Path `
        -ArgumentList @('-NoProfile', '-File', $PSCommandPath, '-LiveOnly') `
        -WindowStyle Hidden -Wait -PassThru
    if ($child.ExitCode -eq 0) {
        Write-Host "ok    the live console path passed in a child process with its own console"
    } else {
        Write-Host "FAIL  the live console path failed in the child process (exit $($child.ExitCode))"
        Write-Host "      re-run to see it: pwsh -NoProfile -File `"$PSCommandPath`" -LiveOnly"
        $script:Failed++
    }
} elseif (-not $LiveOnly) {
    Write-Host 'skip  live console half (pass -Live to run it in a hidden child console)'
} else {
    $state = Open-ClaudeConsoleInput
    if (-not $state) {
        Write-Host "COULD NOT RUN: -LiveOnly was asked for but this process has no console"
        exit 2
    }
    try {
        Assert-Equal $false ([bool]($state.ArmedMode -band [ClaudeAuto.ConsoleInput]::ENABLE_QUICK_EDIT_MODE)) 'arming clears QuickEdit, or the terminal keeps the mouse for selection'
        Assert-Equal $true  ([bool]($state.ArmedMode -band [ClaudeAuto.ConsoleInput]::ENABLE_MOUSE_INPUT)) 'arming enables mouse input'
        Assert-Equal $true  ([bool]($state.ArmedMode -band [ClaudeAuto.ConsoleInput]::ENABLE_WINDOW_INPUT)) 'arming enables resize events'

        # Inject a click and read it back: the only way to exercise the real reader without a hand
        # on the mouse. It proves the struct marshalling and the wait/read loop together.
        $rec = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD[]' 1
        $rec[0] = New-MouseRec 42 9 ([ClaudeAuto.ConsoleInput]::FROM_LEFT_1ST_BUTTON_PRESSED) 0
        [uint32]$written = 0
        $wh = [ClaudeAuto.ConsoleInput]::CreateFileW('CONIN$',
            [ClaudeAuto.ConsoleInput]::GENERIC_READ -bor [ClaudeAuto.ConsoleInput]::GENERIC_WRITE,
            [ClaudeAuto.ConsoleInput]::FILE_SHARE_READ -bor [ClaudeAuto.ConsoleInput]::FILE_SHARE_WRITE,
            [IntPtr]::Zero, [ClaudeAuto.ConsoleInput]::OPEN_EXISTING, 0, [IntPtr]::Zero)
        $injected = [ClaudeAuto.ConsoleInput]::WriteConsoleInputW($wh, $rec, 1, [ref]$written)
        [void][ClaudeAuto.ConsoleInput]::CloseHandle($wh)
        Assert-Equal $true $injected 'a synthetic record can be injected'

        $got = $null
        for ($i = 0; $i -lt 20 -and -not $got; $i++) { $got = Read-ClaudeInputEvent -State $state -TimeoutMs 60 }
        Assert-Equal 'mouse' "$($got.Kind)" 'the reader returns the injected mouse event'
        Assert-Equal 42 "$($got.X)"         'with the column it was given'
        Assert-Equal 9  "$($got.Y)"         'and the row'

        # A timeout must return $null quickly rather than blocking - that is what keeps the menu
        # responsive to resize and Ctrl+C.
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $idle = Read-ClaudeInputEvent -State $state -TimeoutMs 60
        $sw.Stop()
        Assert-Equal '' "$idle" 'an idle read returns nothing'
        Assert-Equal $true ($sw.Elapsed.TotalMilliseconds -lt 500) 'and returns promptly rather than blocking'

        # The last unverified link: what the LAUNCHER actually calls. Wait-KeyOrResize with a mouse
        # state must return the event rather than fall through to [Console]::KeyAvailable, which
        # would eat it. Everything else here tests the reader; this tests the seam the menu uses.
        . "$PSScriptRoot\..\claude-auto\Ui.ps1"
        $rec2 = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD[]' 1
        $rec2[0] = New-MouseRec 7 3 ([ClaudeAuto.ConsoleInput]::FROM_LEFT_1ST_BUTTON_PRESSED) 0
        [uint32]$w2 = 0
        $wh2 = [ClaudeAuto.ConsoleInput]::CreateFileW('CONIN$',
            [ClaudeAuto.ConsoleInput]::GENERIC_READ -bor [ClaudeAuto.ConsoleInput]::GENERIC_WRITE,
            [ClaudeAuto.ConsoleInput]::FILE_SHARE_READ -bor [ClaudeAuto.ConsoleInput]::FILE_SHARE_WRITE,
            [IntPtr]::Zero, [ClaudeAuto.ConsoleInput]::OPEN_EXISTING, 0, [IntPtr]::Zero)
        [void][ClaudeAuto.ConsoleInput]::WriteConsoleInputW($wh2, $rec2, 1, [ref]$w2)
        [void][ClaudeAuto.ConsoleInput]::CloseHandle($wh2)
        # A size that never changes, and a KeyAvailable that would THROW if it were consulted -
        # proving the mouse branch replaces the keyboard poll rather than running beside it.
        $viaWait = Wait-KeyOrResize -ReadKey { throw 'ReadKey must not be called while the mouse is armed' } `
            -Width 80 -Height 24 -GetSize { @(80, 24) } `
            -KeyAvailable { throw 'KeyAvailable must not be called while the mouse is armed' } `
            -MouseState $state -MaxLoops 40
        Assert-Equal 'mouse' "$($viaWait.Kind)" 'Wait-KeyOrResize returns the mouse event the launcher will act on'
        Assert-Equal 7 "$($viaWait.X)" 'with its coordinates intact through the seam'

        # --- a mouse report delivered as TEXT is one event, never N keystrokes (2026-08-25) -------
        # A real console hands the menu MOUSE_EVENT records. Rider's terminal tab hands it the VT
        # report instead, character by character, and the menu executed every character as a menu
        # key. Case was never the defence: the classic encoding writes the byte 32+coordinate, so
        # column 85 arrives as 'u' - the update key - and column 82 as 'r', the rename swap. Both
        # forms are pinned here because they end differently: SGR terminates on M/m after digits,
        # X10 puts M FIRST and follows it with three raw bytes that are letters at ordinary
        # terminal widths.
        function Send-Records([array]$Recs) {
            $arr = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD[]' $Recs.Count
            for ($i = 0; $i -lt $Recs.Count; $i++) { $arr[$i] = $Recs[$i] }
            [uint32]$n = 0
            $wh = [ClaudeAuto.ConsoleInput]::CreateFileW('CONIN$',
                [ClaudeAuto.ConsoleInput]::GENERIC_READ -bor [ClaudeAuto.ConsoleInput]::GENERIC_WRITE,
                [ClaudeAuto.ConsoleInput]::FILE_SHARE_READ -bor [ClaudeAuto.ConsoleInput]::FILE_SHARE_WRITE,
                [IntPtr]::Zero, [ClaudeAuto.ConsoleInput]::OPEN_EXISTING, 0, [IntPtr]::Zero)
            [void][ClaudeAuto.ConsoleInput]::WriteConsoleInputW($wh, $arr, $arr.Count, [ref]$n)
            [void][ClaudeAuto.ConsoleInput]::CloseHandle($wh)
        }
        function Read-AllChars {
            # Everything the reader is willing to hand the menu over the next few reads.
            $seen = ''
            for ($i = 0; $i -lt 40; $i++) {
                $e = Read-ClaudeInputEvent -State $state -TimeoutMs 30
                if ($null -eq $e) { continue }
                if ($e -is [System.ConsoleKeyInfo]) { $seen += "$($e.KeyChar)" }
                elseif ("$e" -eq 'resize') { $seen += '~' }
                else { $seen += '@' }
            }
            return $seen
        }
        function New-TextRec([char]$C) { New-KeyRec 1 0 ([uint16][char]$C) 0 }

        # SGR 1006, a hover at column 85: ESC [ < 3 5 ; 8 5 ; 1 2 M
        Send-Records @((New-TextRec ([char]27)), (New-TextRec '['), (New-TextRec '<'),
                       (New-TextRec '3'), (New-TextRec '5'), (New-TextRec ';'),
                       (New-TextRec '8'), (New-TextRec '5'), (New-TextRec ';'),
                       (New-TextRec '1'), (New-TextRec '2'), (New-TextRec 'M'))
        Assert-Equal '@' (Read-AllChars) 'an SGR report reaches the menu as a mouse event - never as the characters it is made of'

        # X10 / normal tracking: ESC [ M then three RAW bytes. 32+85 is 'u' - the update key. It is
        # decoded into a mouse event, so what reaches the menu is a click, never those letters.
        Send-Records @((New-TextRec ([char]27)), (New-TextRec '['), (New-TextRec 'M'),
                       (New-TextRec ([char]32)), (New-TextRec 'u'), (New-TextRec 'r'))
        Assert-Equal '@' (Read-AllChars) 'an X10 report reaches the menu as a mouse event, never as its coordinate letters'

        # The real queue interleaves key-UP records with the sequence: the 2026-08-25 trace of the
        # Rider terminal reads M, C, C-up, f. Counting RECORDS instead of key-downs consumed the up
        # and left the last coordinate byte - a lowercase letter - to press a menu key.
        function New-UpRec([char]$C) { New-KeyRec 0 0 ([uint16][char]$C) }
        Send-Records @((New-TextRec ([char]27)), (New-TextRec '['), (New-TextRec 'M'),
                       (New-TextRec 'C'), (New-UpRec 'C'),
                       (New-TextRec ([char]32)), (New-UpRec ([char]32)),
                       (New-TextRec 'u'), (New-UpRec 'u'))
        Assert-Equal '@' (Read-AllChars) 'key-up records inside the sequence do not shift a coordinate byte into the menu'

        # --- SS3 (application-keypad mode) blind-reads one record instead of counting key-downs ---
        # ESC O is the SS3 introducer; exactly one final byte follows. The queue can interleave the
        # O key's own key-UP before that byte arrives - ESC, O-down, O-up, u-down is the real shape -
        # and a blind single read after the introducer consumes the O-up instead of the final byte,
        # leaking it to the menu as an ordinary hotkey. Application-keypad finals run p-y, so
        # keypad-5 is ESC O u (Invoke-ClaudeUpdate) and keypad-2 is ESC O r (the rename swap).
        Send-Records @((New-KeyRec 1 27 27 0), (New-TextRec ([char]0x4F)), (New-UpRec ([char]0x4F)), (New-TextRec 'u'))
        Assert-Equal '' (Read-AllChars) 'SS3 with an interleaved key-up does not leak the final byte as a menu key'

        # --- and a report the menu can ACT on, which is why Claude Code has a mouse in that tab ---
        # Swallowing keeps the terminal from pressing menu keys; decoding is what gives the click
        # back. Same bytes, read as a report instead of as keystrokes.
        function Read-FirstEvent {
            for ($i = 0; $i -lt 40; $i++) {
                $e = Read-ClaudeInputEvent -State $state -TimeoutMs 30
                if ($null -ne $e) { return $e }
            }
            return $null
        }
        # SGR press: ESC [ < 0 ; 41 ; 13 M - left button, column 41, row 13, both 1-based.
        Send-Records @((New-TextRec ([char]27)), (New-TextRec '['), (New-TextRec '<'),
                       (New-TextRec '0'), (New-TextRec ';'), (New-TextRec '4'), (New-TextRec '1'),
                       (New-TextRec ';'), (New-TextRec '1'), (New-TextRec '3'), (New-TextRec 'M'))
        $ev = Read-FirstEvent
        Assert-Equal 'mouse' "$($ev.Kind)" 'an SGR press is decoded into a mouse event, not discarded'
        Assert-Equal 40 "$($ev.X)" 'its column is zero-based, as every screen expects'
        Assert-Equal 12 "$($ev.Y)" 'and so is its row'
        Assert-Equal $true "$($ev.Left)" 'the left button is down'
        Assert-Equal $false "$($ev.IsMove)" 'a press is not a move - the press guard depends on it'

        # --- the ESC introducer peek must wait for a report that has not fully arrived yet --------
        # A real terminal delivers a mouse report byte by byte (this machine's own input traces show
        # 1-6ms between bytes of one report), not as one atomic write. The introducer peek used to be
        # a single non-blocking PeekConsoleInputW: it ran in the gap right after the ESC was read and
        # concluded "nothing follows", so the rest of the report leaked to the menu as the literal
        # keys it is made of (a click at column 41 leaks '<0;41;13M' - 10 stray keys). Simulated here
        # with a SEPARATE runspace of this same process: it can Start-Sleep without blocking the
        # foreground reader, unlike a second WriteConsoleInputW call from this thread which would
        # simply queue the payload before the read even happens.
        # The expensive part (spinning up a Runspace and opening it) happens BEFORE the ESC is
        # written, so none of that setup cost counts against the timing this test is pinning: only
        # BeginInvoke's dispatch plus a short busy-wait (never Start-Sleep - Windows' ~15ms timer
        # quantum would make a "2ms" sleep swallow the whole point) run after the ESC is on the wire.
        function New-DelayedSender([array]$Chars, [int]$DelayMs) {
            $rs = [runspacefactory]::CreateRunspace()
            $rs.Open()
            $ps = [powershell]::Create()
            $ps.Runspace = $rs
            [void]$ps.AddScript({
                param($Chars, $DelayMs)
                $sw = [Diagnostics.Stopwatch]::StartNew()
                while ($sw.Elapsed.TotalMilliseconds -lt $DelayMs) { }
                $arr = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD[]' $Chars.Count
                for ($i = 0; $i -lt $Chars.Count; $i++) {
                    $k = New-Object 'ClaudeAuto.ConsoleInput+KEY_EVENT_RECORD'
                    $k.bKeyDown = 1; $k.wRepeatCount = 1; $k.wVirtualKeyCode = 0; $k.wVirtualScanCode = 0
                    $k.UnicodeChar = [uint16][char]$Chars[$i]; $k.dwControlKeyState = 0
                    $r = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD'
                    $r.EventType = [ClaudeAuto.ConsoleInput]::KEY_EVENT
                    $r.KeyEvent = $k
                    $arr[$i] = $r
                }
                [uint32]$n = 0
                $wh = [ClaudeAuto.ConsoleInput]::CreateFileW('CONIN$',
                    [ClaudeAuto.ConsoleInput]::GENERIC_READ -bor [ClaudeAuto.ConsoleInput]::GENERIC_WRITE,
                    [ClaudeAuto.ConsoleInput]::FILE_SHARE_READ -bor [ClaudeAuto.ConsoleInput]::FILE_SHARE_WRITE,
                    [IntPtr]::Zero, [ClaudeAuto.ConsoleInput]::OPEN_EXISTING, 0, [IntPtr]::Zero)
                [void][ClaudeAuto.ConsoleInput]::WriteConsoleInputW($wh, $arr, $arr.Count, [ref]$n)
                [void][ClaudeAuto.ConsoleInput]::CloseHandle($wh)
            }).AddArgument($Chars).AddArgument($DelayMs)
            return [pscustomobject]@{ PS = $ps; Runspace = $rs; Handle = $null }
        }
        function Start-DelayedSender($Sender) { $Sender.Handle = $Sender.PS.BeginInvoke() }
        function Wait-DelayedSend($Sender) {
            $null = $Sender.PS.EndInvoke($Sender.Handle)
            $Sender.PS.Dispose()
            $Sender.Runspace.Close()
        }
        # ESC is written and read FIRST. The rest of an SGR press (<0;41;13M) is queued only after a
        # short delay from a background runspace - the payload has not arrived when the ESC is read.
        #
        # Warm up the background-runspace machinery once, throwaway: the FIRST BeginInvoke on a
        # fresh runspace in this harness costs an extra 10-20ms of thread-pool/JIT cold start that
        # has nothing to do with the defect under test, and would starve the 5ms peek window before
        # the real attempt even begins.
        $warm = New-DelayedSender -Chars @('X') -DelayMs 0
        Start-DelayedSender $warm
        Start-Sleep -Milliseconds 20
        Wait-DelayedSend $warm
        Clear-ClaudeInputQueue -State $state | Out-Null

        # Retried up to 5x: this harness's background runspace occasionally wakes 10-20ms late under
        # scheduler load (measured), which the fix cannot be expected to out-wait at its production
        # budget (~5ms, matching real terminals' 1-6ms byte gaps). The OLD code fails this EVERY
        # attempt regardless of timing, since its peek is instant and never catches a delayed
        # payload - only the harness's jitter is being retried around, not the defect.
        $delayedOk = $false
        $lastEv = $null
        $lastLeftover = $null
        for ($attempt = 0; $attempt -lt 5 -and -not $delayedOk; $attempt++) {
            $sender = New-DelayedSender -Chars @('[','<','0',';','4','1',';','1','3','M') -DelayMs 2
            Send-Records @((New-TextRec ([char]27)))
            Start-DelayedSender $sender
            $ev = Read-FirstEvent
            Wait-DelayedSend $sender
            $leftover = Read-AllChars
            $lastEv = $ev; $lastLeftover = $leftover
            if ("$($ev.Kind)" -eq 'mouse' -and "$($ev.X)" -eq '40' -and "$($ev.Y)" -eq '12' -and $leftover -eq '') {
                $delayedOk = $true
            } else {
                Clear-ClaudeInputQueue -State $state | Out-Null
            }
        }
        Assert-Equal 'mouse' "$($lastEv.Kind)" 'a report whose payload arrives after the ESC was already read still decodes as one mouse event'
        Assert-Equal 40 "$($lastEv.X)" 'with the column intact'
        Assert-Equal 12 "$($lastEv.Y)" 'and the row'
        Assert-Equal '' $lastLeftover 'no stray keys are left for the menu once the delayed payload is decoded'

        # The release carries the same position with no button: ignored by every screen, and it must
        # NOT arrive looking like a second press.
        Send-Records @((New-TextRec ([char]27)), (New-TextRec '['), (New-TextRec '<'),
                       (New-TextRec '0'), (New-TextRec ';'), (New-TextRec '4'), (New-TextRec '1'),
                       (New-TextRec ';'), (New-TextRec '1'), (New-TextRec '3'), (New-TextRec 'm'))
        $ev = Read-FirstEvent
        Assert-Equal 'mouse' "$($ev.Kind)" 'a release is decoded too'
        Assert-Equal $false "$($ev.Left)" 'with the button UP, so it cannot read as a second press'

        # Wheel: bit 64 set, low bits 0 = up, 1 = down. The picker scrolls on the SIGN.
        Send-Records @((New-TextRec ([char]27)), (New-TextRec '['), (New-TextRec '<'),
                       (New-TextRec '6'), (New-TextRec '4'), (New-TextRec ';'), (New-TextRec '5'),
                       (New-TextRec ';'), (New-TextRec '5'), (New-TextRec 'M'))
        $ev = Read-FirstEvent
        Assert-Equal $true "$($ev.WheelUp)" 'wheel up decodes as up'
        Send-Records @((New-TextRec ([char]27)), (New-TextRec '['), (New-TextRec '<'),
                       (New-TextRec '6'), (New-TextRec '5'), (New-TextRec ';'), (New-TextRec '5'),
                       (New-TextRec ';'), (New-TextRec '5'), (New-TextRec 'M'))
        $ev = Read-FirstEvent
        Assert-Equal $true "$($ev.WheelDown)" 'wheel down decodes as DOWN, not as up'

        # Motion sets bit 32. It must decode as a MOVE, because that flag is the only thing standing
        # between a hover and a footer press.
        Send-Records @((New-TextRec ([char]27)), (New-TextRec '['), (New-TextRec '<'),
                       (New-TextRec '3'), (New-TextRec '5'), (New-TextRec ';'), (New-TextRec '9'),
                       (New-TextRec ';'), (New-TextRec '9'), (New-TextRec 'M'))
        $ev = Read-FirstEvent
        Assert-Equal $true "$($ev.IsMove)" 'a motion report is a move, never a press'
        Assert-Equal $false "$($ev.Left)" 'and carries no button'

        # The guard must not eat the key it is named after: a bare Escape with nothing behind it is
        # still how the owner leaves a screen.
        Send-Records @((New-KeyRec 1 27 27 0))
        Assert-Equal ([string][char]27) (Read-AllChars) 'a bare Escape still reaches the menu'

        # And an ordinary keypress that merely FOLLOWS an Escape is not swallowed with it.
        Send-Records @((New-KeyRec 1 27 27 0), (New-TextRec 'u'))
        Assert-Equal "$([char]27)u" (Read-AllChars) 'Escape then u is two keys, not a sequence'
    } finally {
        $restored = Close-ClaudeConsoleInput -State $state
        Assert-Equal $true $restored 'the console mode is restored bit-exact, verified by reading it back'
        Assert-Equal $true (Close-ClaudeConsoleInput -State $state) 'closing twice is harmless — it runs from a finally that can unwind twice'
    }

    # ------------------------------------------------------------------ launcher composition
    #
    # Every assertion above tests a PIECE. This one tests the order the launcher actually puts them
    # in, which no suite covers otherwise: Enter-AltBuffer sets TreatControlCAsInput, THEN the
    # console is armed, and on the way out the mode is restored BEFORE Exit-AltBuffer clears
    # TreatControlCAsInput again.
    #
    # Why it matters, and why reading the code was not enough. The launch screen exits on Ctrl+C by
    # reading it AS A KEY. Under ENABLE_PROCESSED_INPUT a real Ctrl+C is swallowed by the system and
    # raises a signal instead, which would kill the launcher rather than return to the screen. What
    # makes that safe is that TreatControlCAsInput CLEARS that flag, and it does so before the mode
    # is captured - so the armed mode inherits it. That chain is asserted here rather than assumed.
    $preEverything = 0
    $probeH = [ClaudeAuto.ConsoleInput]::CreateFileW('CONIN$',
        [ClaudeAuto.ConsoleInput]::GENERIC_READ -bor [ClaudeAuto.ConsoleInput]::GENERIC_WRITE,
        [ClaudeAuto.ConsoleInput]::FILE_SHARE_READ -bor [ClaudeAuto.ConsoleInput]::FILE_SHARE_WRITE,
        [IntPtr]::Zero, [ClaudeAuto.ConsoleInput]::OPEN_EXISTING, 0, [IntPtr]::Zero)
    [void][ClaudeAuto.ConsoleInput]::GetConsoleMode($probeH, [ref]$preEverything)

    try { [Console]::TreatControlCAsInput = $true } catch { }
    $armed2 = Open-ClaudeConsoleInput
    if (-not $armed2) {
        Assert-Equal $true $false 'arming after TreatControlCAsInput should still work'
    } else {
        Assert-Equal $false ([bool]($armed2.ArmedMode -band [ClaudeAuto.ConsoleInput]::ENABLE_PROCESSED_INPUT)) `
            'with TreatControlCAsInput set first, the armed mode has PROCESSED_INPUT cleared — so Ctrl+C arrives as a KEY and the launch screen can treat it as Escape'

        # And prove it end to end: a real Ctrl+C key record must survive the seam with its modifier.
        $ctrlRec = New-Object 'ClaudeAuto.ConsoleInput+INPUT_RECORD[]' 1
        $ctrlRec[0] = New-KeyRec 1 0x43 0x03 ([ClaudeAuto.ConsoleInput]::LEFT_CTRL_PRESSED)
        [uint32]$cw = 0
        $ch = [ClaudeAuto.ConsoleInput]::CreateFileW('CONIN$',
            [ClaudeAuto.ConsoleInput]::GENERIC_READ -bor [ClaudeAuto.ConsoleInput]::GENERIC_WRITE,
            [ClaudeAuto.ConsoleInput]::FILE_SHARE_READ -bor [ClaudeAuto.ConsoleInput]::FILE_SHARE_WRITE,
            [IntPtr]::Zero, [ClaudeAuto.ConsoleInput]::OPEN_EXISTING, 0, [IntPtr]::Zero)
        [void][ClaudeAuto.ConsoleInput]::WriteConsoleInputW($ch, $ctrlRec, 1, [ref]$cw)
        [void][ClaudeAuto.ConsoleInput]::CloseHandle($ch)
        $viaCtrl = Wait-KeyOrResize -ReadKey { throw 'ReadKey must not be called' } -Width 80 -Height 24 `
            -GetSize { @(80, 24) } -KeyAvailable { throw 'KeyAvailable must not be called' } `
            -MouseState $armed2 -MaxLoops 40
        Assert-Equal 'C' "$($viaCtrl.Key)" 'a Ctrl+C key record reaches the launcher through the armed seam'
        Assert-Equal $true ([bool]($viaCtrl.Modifiers -band [System.ConsoleModifiers]::Control)) 'carrying the Control modifier the exit test looks for'

        # Unwind in the launcher's order: restore the mode, THEN undo TreatControlCAsInput. Doing it
        # the other way round would leave PROCESSED_INPUT off for the rest of the terminal's life.
        $null = Close-ClaudeConsoleInput -State $armed2
        try { [Console]::TreatControlCAsInput = $false } catch { }
        $after = 0
        [void][ClaudeAuto.ConsoleInput]::GetConsoleMode($probeH, [ref]$after)
        Assert-Equal ('0x{0:X4}' -f $preEverything) ('0x{0:X4}' -f $after) `
            'after the launcher unwinds in its own order the console is exactly as it was found'
    }
    [void][ClaudeAuto.ConsoleInput]::CloseHandle($probeH)
}

# --- the loaded build must BE the source, not a cached ancestor of it -------------------------
# 2026-08-25: with two older launchers running, the DLL on disk could not be replaced (Windows
# refuses to overwrite a loaded assembly, and -Force does not help), Move-Item's failure was
# swallowed, and Initialize-ClaudeConsoleInput fell back to Add-Type -Path on the STALE file while
# returning $true. The session was then missing the P/Invoke the source had just gained, so the
# input drain silently did nothing. Enumerating the imports from the .cs rather than naming one
# keeps this honest when the next one is added.
$srcText = Get-Content -LiteralPath "$PSScriptRoot\..\claude-auto\ConsoleInput.cs" -Raw
$declared = @([regex]::Matches($srcText, 'public\s+static\s+extern\s+\S+\s+(\w+)\s*\(') | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
Assert-Equal $true ($declared.Count -ge 8) "the source declares the imports this asserts on (found $($declared.Count))"
$missing = @($declared | Where-Object { $null -eq [ClaudeAuto.ConsoleInput].GetMethod($_) })
Assert-Equal '' ($missing -join ',') 'every P/Invoke in ConsoleInput.cs is present on the loaded type - a stale cached DLL is not a pass'

# -LiveOnly (the hidden child -Live spawns) runs the live-console branch's own assertions too, so
# its count is not the bare-run count. That branch also has a genuinely environment-dependent tail
# (arming a SECOND time after TreatControlCAsInput can fail, in which case only 1 assertion runs
# there instead of 4 - see 'arming after TreatControlCAsInput should still work'), so its count is
# not a single fixed number either: it is bounded below by the smaller of the two, measured in a
# genuine hidden console, never guessed. The bare count (30) IS exact - checkpoint.ps1 only ever
# runs this suite bare, and that path has no such branching.
if ($LiveOnly) {
    if ($script:Ran -lt 66) { Write-Host "COULD NOT RUN: expected at least 66 assertions (the live-console branch has an environment-dependent tail), ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
} elseif ($script:Ran -ne 30) {
    Write-Host "COULD NOT RUN: expected 30 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2
}
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
