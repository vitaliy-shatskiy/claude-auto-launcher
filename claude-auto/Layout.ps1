# Pure geometry. Nothing here reads the console, emits colour, or has an opinion about content.

function Limit-Line {
    # Truncate rather than let the console wrap: a wrapped row breaks the alignment of every row
    # under it and there is no way to redraw out of that.
    param([string]$Text, [int]$Max)
    if (-not $Text) { return '' }
    if ($Text.Length -le $Max) { return $Text }
    if ($Max -le 1) { return $Text.Substring(0, [Math]::Max(0, $Max)) }
    return $Text.Substring(0, $Max - 1) + [char]0x2026
}

function Split-TextLines {
    # Word wrap for preview text. A token longer than the pane is cut rather than dropped, because
    # a pasted url or a stack frame is exactly the case where the first characters still identify it.
    param([string]$Text, [int]$Width, [int]$MaxLines = 0)
    if ([string]::IsNullOrWhiteSpace($Text) -or $Width -le 0) { return @() }

    $lines = @()
    $current = ''
    foreach ($word in ($Text -split '\s+' | Where-Object { $_ })) {
        $token = $word
        while ($token.Length -gt $Width) {
            if ($current) { $lines += $current; $current = '' }
            $lines += $token.Substring(0, $Width)
            $token = $token.Substring($Width)
        }
        if (-not $current) { $current = $token }
        elseif (($current.Length + 1 + $token.Length) -le $Width) { $current += ' ' + $token }
        else { $lines += $current; $current = $token }
    }
    if ($current) { $lines += $current }

    if ($MaxLines -gt 0 -and $lines.Count -gt $MaxLines) {
        $kept = @($lines[0..($MaxLines - 1)])
        # Plain Substring, not Limit-Line: Limit-Line appends an ellipsis of its own when it
        # truncates, and adding ours on top produced '……'. The reader must be able to tell a
        # truncated preview from a complete one with exactly one marker.
        $last = $kept[$MaxLines - 1]
        if ($last.Length -gt ($Width - 1)) { $last = $last.Substring(0, $Width - 1) }
        $kept[$MaxLines - 1] = $last + [string][char]0x2026
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
        $label = Limit-Line -Text " $Title " -Max ($inner - 2)
        $top += "$($g.H)" + $label
        $top += ([string]$g.H * ($inner - $label.Length - 1))
    } else {
        $top += ([string]$g.H * $inner)
    }
    $top += "$($g.TR)"

    $out = @($top)
    foreach ($line in $Lines) {
        $body = Limit-Line -Text $line -Max $inner
        $out += "$($g.V)" + $body.PadRight($inner) + "$($g.V)"
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
        $out += (Limit-Line -Text $l -Max $LeftWidth).PadRight($LeftWidth) +
                "$($g.V)" +
                (Limit-Line -Text $r -Max $RightWidth).PadRight($RightWidth)
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
