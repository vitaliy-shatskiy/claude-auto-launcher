# Pure geometry, and the one console fact it cannot avoid: whether the terminal can draw non-ASCII,
# which decides the truncation marker exactly as it decides the box glyphs (Get-Ellipsis).

# East Asian Wide and Fullwidth, plus the emoji blocks. Written as ranges rather than looked up per
# character: this runs over every row of every repaint, and the table is what the console actually
# does, not what a general-purpose library thinks it might.
$script:WideRanges = @(
    @(0x1100, 0x115F), @(0x2E80, 0x303E), @(0x3041, 0x33FF), @(0x3400, 0x4DBF),
    @(0x4E00, 0x9FFF), @(0xA000, 0xA4CF), @(0xA960, 0xA97F), @(0xAC00, 0xD7A3),
    @(0xF900, 0xFAFF), @(0xFE10, 0xFE19), @(0xFE30, 0xFE6F), @(0xFF00, 0xFF60),
    @(0xFFE0, 0xFFE6), @(0x1F300, 0x1F64F), @(0x1F680, 0x1F6FF), @(0x1F900, 0x1F9FF),
    @(0x20000, 0x2FFFD), @(0x30000, 0x3FFFD)
)
$script:ZeroWidthCategories = @(
    [Globalization.UnicodeCategory]::NonSpacingMark,
    [Globalization.UnicodeCategory]::EnclosingMark,
    [Globalization.UnicodeCategory]::Format
)

function Get-CodePointWidth {
    param([int]$CodePoint)
    if ($CodePoint -lt 0x20) { return 0 }
    if ($CodePoint -lt 0x7F) { return 1 }
    $s = [char]::ConvertFromUtf32($CodePoint)
    if ([Globalization.CharUnicodeInfo]::GetUnicodeCategory($s, 0) -in $script:ZeroWidthCategories) { return 0 }
    foreach ($r in $script:WideRanges) { if ($CodePoint -ge $r[0] -and $CodePoint -le $r[1]) { return 2 } }
    return 1
}

function Get-DisplayWidth {
    # The console draws CELLS; .Length counts UTF-16 code units. They differ for exactly the content
    # this launcher puts on screen - a session's last message, which is arbitrary text. A CJK
    # ideograph is two cells in one code unit, an emoji two cells in TWO code units, a combining
    # mark none at all. Measured 2026-09-08 before this existed: a picker rendered at -Width 60 with
    # a Chinese message produced rows of 60 code units and 100 cells, which wraps every row and
    # desynchronises every repaint after it.
    param([string]$Text)
    if (-not $Text) { return 0 }
    $w = 0
    for ($i = 0; $i -lt $Text.Length; $i++) {
        $ch = $Text[$i]
        $cp = [int]$ch
        if ([char]::IsHighSurrogate($ch) -and ($i + 1) -lt $Text.Length -and [char]::IsLowSurrogate($Text[$i + 1])) {
            $cp = [char]::ConvertToUtf32($ch, $Text[$i + 1])
            $i++
        }
        $w += Get-CodePointWidth -CodePoint $cp
    }
    return $w
}

function Limit-Cells {
    # The longest prefix of $Text that fits $Max CELLS, cut only between whole characters. Cutting on
    # code units splits a surrogate pair, and half of one is not a character: the console draws a
    # replacement box and some terminals lose the rest of the line.
    param([string]$Text, [int]$Max)
    if (-not $Text -or $Max -le 0) { return '' }
    $w = 0
    $sb = [Text.StringBuilder]::new()
    for ($i = 0; $i -lt $Text.Length; $i++) {
        $len = if ([char]::IsHighSurrogate($Text[$i]) -and ($i + 1) -lt $Text.Length -and [char]::IsLowSurrogate($Text[$i + 1])) { 2 } else { 1 }
        $piece = $Text.Substring($i, $len)
        $cw = Get-DisplayWidth -Text $piece
        if (($w + $cw) -gt $Max) { break }
        $null = $sb.Append($piece)
        $w += $cw
        $i += ($len - 1)
    }
    return $sb.ToString()
}

function Get-Ellipsis {
    # Exactly ONE cell, always: the truncation arithmetic reserves one column for it. A pinned
    # $script:Ellipsis wins over the ambient decision, which is how a caller hands its own -Ascii
    # verdict to a helper that takes no switch. Before this, the marker was U+2026 unconditionally -
    # so a box drawn with -Ascii, on the very code page Test-AsciiRequired exists to detect, came
    # back as ASCII borders around one mojibake character.
    param([switch]$Ascii)
    if ($script:Ellipsis) { return $script:Ellipsis }
    if ($Ascii -or (Test-AsciiRequired)) { return '~' }
    return [string][char]0x2026
}

function Limit-Line {
    # Truncate rather than let the console wrap: a wrapped row breaks the alignment of every row
    # under it and there is no way to redraw out of that.
    param([string]$Text, [int]$Max, [switch]$Ascii)
    if (-not $Text) { return '' }
    if ((Get-DisplayWidth -Text $Text) -le $Max) { return $Text }
    if ($Max -le 1) { return (Limit-Cells -Text $Text -Max ([Math]::Max(0, $Max))) }
    return (Limit-Cells -Text $Text -Max ($Max - 1)) + (Get-Ellipsis -Ascii:$Ascii)
}

function Split-TextLines {
    # Word wrap for preview text. A token longer than the pane is cut rather than dropped, because
    # a pasted url or a stack frame is exactly the case where the first characters still identify it.
    param([string]$Text, [int]$Width, [int]$MaxLines = 0, [switch]$Ascii)
    if ([string]::IsNullOrWhiteSpace($Text) -or $Width -le 0) { return @() }

    $lines = @()
    $current = ''
    foreach ($word in ($Text -split '\s+' | Where-Object { $_ })) {
        $token = $word
        while ((Get-DisplayWidth -Text $token) -gt $Width) {
            if ($current) { $lines += $current; $current = '' }
            $head = Limit-Cells -Text $token -Max $Width
            # Nothing fits: the pane is narrower than a single glyph. Emitting the token anyway
            # produces a row wider than the pane, which wraps and drags every row below it out of
            # line - the one outcome this whole file exists to prevent. Drop it instead, and never
            # loop forever on a prefix that cannot shrink.
            if (-not $head) { $token = ''; break }
            $lines += $head
            $token = $token.Substring($head.Length)
        }
        if (-not $current) { $current = $token }
        elseif (((Get-DisplayWidth -Text $current) + 1 + (Get-DisplayWidth -Text $token)) -le $Width) { $current += ' ' + $token }
        else { $lines += $current; $current = $token }
    }
    if ($current) { $lines += $current }

    if ($MaxLines -gt 0 -and $lines.Count -gt $MaxLines) {
        $kept = @($lines[0..($MaxLines - 1)])
        # Limit-Cells, not Limit-Line: Limit-Line appends an ellipsis of its own when it truncates,
        # and adding ours on top produced '……'. The reader must be able to tell a truncated preview
        # from a complete one with exactly one marker.
        $last = $kept[$MaxLines - 1]
        if ((Get-DisplayWidth -Text $last) -gt ($Width - 1)) { $last = Limit-Cells -Text $last -Max ($Width - 1) }
        $kept[$MaxLines - 1] = $last + (Get-Ellipsis -Ascii:$Ascii)
        return $kept
    }
    return $lines
}

function Split-OutputLines {
    # Captured command output -> box rows. Split-TextLines cannot do this job: it splits on \s+, so
    # a multi-line report is welded into one paragraph and the structure that makes it readable is
    # gone. Here every physical line keeps its identity and is wrapped on its own.
    #
    # The cap keeps the HEAD, not the tail. `claude doctor` puts version, path, install method and
    # the last update attempt at the top and boilerplate at the bottom - the previous handler took
    # the last three lines and so showed nothing but the boilerplate.
    param([string]$Text, [int]$Width, [int]$MaxLines = 0)
    if ([string]::IsNullOrWhiteSpace($Text) -or $Width -le 0) { return @() }

    $rows = @()
    $pendingBlank = $false
    foreach ($raw in ($Text -split "`r?`n")) {
        if (-not $raw.Trim()) {
            # Runs of blank lines collapse to one and leading blanks are dropped: vertical space is
            # the scarcest thing on this screen.
            if ($rows.Count -gt 0) { $pendingBlank = $true }
            continue
        }
        if ($pendingBlank) { $rows += ''; $pendingBlank = $false }
        $rows += @(Split-TextLines -Text $raw -Width $Width)
    }

    if ($MaxLines -gt 0 -and $rows.Count -gt $MaxLines) {
        # -1 leaves room for the marker. `0..($keep - 1)` is guarded because in PowerShell `0..-1`
        # counts DOWN and would silently return two rows instead of none.
        $keep = $MaxLines - 1
        $kept = if ($keep -gt 0) { @($rows[0..($keep - 1)]) } else { @() }
        $kept += Limit-Line -Text "(+$($rows.Count - $keep) more lines)" -Max $Width
        return $kept
    }
    return $rows
}

function New-Box {
    # A rounded (or ASCII) frame of exactly $Width columns. Content is truncated to fit, never
    # wrapped - the caller decides how its text breaks, because only the caller knows what it is.
    param([string[]]$Lines, [int]$Width, [string]$Title = '', [switch]$Ascii)
    $g = Get-Glyphs -Ascii:$Ascii
    $inner = $Width - 2
    if ($inner -lt 1) { return @() }

    $top = "$($g.TL)"
    if ($Title) {
        $label = Limit-Line -Text " $Title " -Max ($inner - 2) -Ascii:$Ascii
        $top += "$($g.H)" + $label
        $top += ([string]$g.H * ($inner - (Get-DisplayWidth -Text $label) - 1))
    } else {
        $top += ([string]$g.H * $inner)
    }
    $top += "$($g.TR)"

    $out = @($top)
    foreach ($line in $Lines) {
        # PadRight counts code units, so a row narrow in CELLS was padded PAST its own right border.
        # The pad is computed from the display width instead.
        $body = Limit-Line -Text $line -Max $inner -Ascii:$Ascii
        $pad = [Math]::Max(0, $inner - (Get-DisplayWidth -Text $body))
        $out += "$($g.V)" + $body + (' ' * $pad) + "$($g.V)"
    }
    $out += "$($g.BL)" + ([string]$g.H * $inner) + "$($g.BR)"
    return $out
}

function Join-Panes {
    # Side by side with a single divider. Both panes are padded to their declared width so the
    # divider stays in one column no matter how ragged the content is.
    param([string[]]$Left, [string[]]$Right, [int]$LeftWidth, [int]$RightWidth, [switch]$Ascii)
    $g = Get-Glyphs -Ascii:$Ascii
    $height = [Math]::Max(@($Left).Count, @($Right).Count)
    $out = @()
    for ($i = 0; $i -lt $height; $i++) {
        $l = if ($i -lt @($Left).Count) { $Left[$i] } else { '' }
        $r = if ($i -lt @($Right).Count) { $Right[$i] } else { '' }
        # Padded in cells, not code units: measured the other way the divider drifts one column to
        # the right for every wide glyph in the left pane.
        $lc = Limit-Line -Text $l -Max $LeftWidth -Ascii:$Ascii
        $rc = Limit-Line -Text $r -Max $RightWidth -Ascii:$Ascii
        $out += $lc + (' ' * [Math]::Max(0, $LeftWidth - (Get-DisplayWidth -Text $lc))) +
                "$($g.V)" +
                $rc + (' ' * [Math]::Max(0, $RightWidth - (Get-DisplayWidth -Text $rc)))
    }
    return $out
}

function Get-Viewport {
    # Keep the selection centred where possible, clamped at both ends, and never ask for more rows
    # than the list has.
    param([int]$Count, [int]$Index, [int]$Visible)
    $v = [Math]::Max(1, [Math]::Min($Visible, $Count))
    $start = [Math]::Min(
        [Math]::Max(0, $Index - [Math]::Floor($v / 2)),
        [Math]::Max(0, $Count - $v))
    return [pscustomobject]@{ Start = [int]$start; Visible = [int]$v }
}
