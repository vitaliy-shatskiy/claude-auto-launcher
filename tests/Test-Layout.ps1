# Assertions for Layout.ps1. Run: pwsh -NoProfile -File Test-Layout.ps1
try {
    . "$PSScriptRoot\..\claude-auto\Theme.ps1"
    . "$PSScriptRoot\..\claude-auto\Layout.ps1"
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

# --- truncation --------------------------------------------------------------------------
Assert-Equal 'abc' (Limit-Line -Text 'abc' -Max 10) 'a short line is untouched'
Assert-Equal 4 (Limit-Line -Text 'abcdefgh' -Max 4).Length 'a long line is cut to the width'
Assert-Equal $true (Limit-Line -Text 'abcdefgh' -Max 4).EndsWith([string][char]0x2026) 'truncation ends with an ellipsis'
Assert-Equal '' (Limit-Line -Text $null -Max 10) 'null text becomes an empty line'

# --- word wrap ---------------------------------------------------------------------------
$w = Split-TextLines -Text 'the quick brown fox jumps over the lazy dog' -Width 12
Assert-Equal 0 (@($w | Where-Object { $_.Length -gt 12 }).Count) 'no wrapped line exceeds the width'
Assert-Equal $true ($w.Count -gt 1) 'a long sentence wraps onto several lines'
Assert-Equal 'the quick' $w[0] 'wrapping breaks on spaces, not mid-word'

# A token longer than the pane has nowhere to break: it is cut, not dropped.
$w = Split-TextLines -Text 'aaaaaaaaaaaaaaaaaaaa short' -Width 8
Assert-Equal 0 (@($w | Where-Object { $_.Length -gt 8 }).Count) 'an over-long token is cut to the width'
Assert-Equal $true ($w.Count -ge 2) 'text after an over-long token is not lost'

# EndsWith(ellipsis) is true for both one ellipsis and a doubled '……' - it cannot tell a correct
# truncation from Limit-Line's own ellipsis plus a second one appended on top. Count occurrences
# instead: that is a predicate only the correct answer satisfies.
function Get-EllipsisCount {
    param([string]$Text)
    $mark = [char]0x2026
    return (@($Text.ToCharArray() | Where-Object { $_ -eq $mark })).Count
}

$w = Split-TextLines -Text 'one two three four five six seven' -Width 10 -MaxLines 2
Assert-Equal 2 $w.Count 'MaxLines caps the number of lines'
Assert-Equal $true $w[1].EndsWith([string][char]0x2026) 'a capped wrap ends with an ellipsis'
Assert-Equal 1 (Get-EllipsisCount -Text $w[1]) 'a full-width final kept line carries exactly one ellipsis, not a doubled one'
Assert-Equal $false ($w[1].Contains(' x')) 'a full-width final kept line never leaks the literal x fallback'
Assert-Equal 0 (@($w | Where-Object { $_.Length -gt 10 }).Count) 'a full-width final kept line never exceeds the width'

# A short final kept line must not leak a literal 'x' - the ellipsis is unconditional, not left to
# Limit-Line's own overflow check. Measured 2026-08-10: 'aa bb cc' + ' x' is exactly 10 characters,
# so the old code returned it unchanged and the reader saw a bare 'x' instead of a truncation marker.
$w = @(Split-TextLines -Text 'aa bb cc dd ee' -Width 10 -MaxLines 1)
Assert-Equal $true $w[0].EndsWith([string][char]0x2026) 'a short final kept line still ends with the ellipsis'
Assert-Equal 1 (Get-EllipsisCount -Text $w[0]) 'a short final kept line carries exactly one ellipsis, not a doubled one'
Assert-Equal $false ($w[0].Contains(' x')) 'a short final kept line never leaks the literal x fallback'
Assert-Equal 0 (@($w | Where-Object { $_.Length -gt 10 }).Count) 'a short final kept line never exceeds the width'

Assert-Equal 0 (Split-TextLines -Text '' -Width 10).Count 'empty text wraps to nothing'

# --- boxes -------------------------------------------------------------------------------
$box = New-Box -Lines @('hello', 'world') -Width 20 -Ascii
Assert-Equal 4 $box.Count 'a two-line box is four lines tall'
Assert-Equal 0 (@($box | Where-Object { $_.Length -ne 20 }).Count) 'every box line is exactly the box width'
Assert-Equal $true $box[0].StartsWith('+') 'the ASCII box opens with a corner'

$box = New-Box -Lines @('x') -Width 30 -Title 'resume' -Ascii
Assert-Equal $true ($box[0] -match 'resume') 'the title is drawn into the top border'
Assert-Equal 30 $box[0].Length 'a titled top border is still exactly the box width'

# Content wider than the box is truncated, never wrapped into the border.
$box = New-Box -Lines @('0123456789012345678901234567890123456789') -Width 20 -Ascii
Assert-Equal 0 (@($box | Where-Object { $_.Length -ne 20 }).Count) 'over-long content is truncated to fit the box'

# --- two panes ---------------------------------------------------------------------------
$j = Join-Panes -Left @('a', 'b', 'c') -Right @('1') -LeftWidth 10 -RightWidth 8 -Ascii
Assert-Equal 3 $j.Count 'the joined height is the taller pane'
Assert-Equal 0 (@($j | Where-Object { $_.Length -ne 19 }).Count) 'each joined line is left + divider + right'
Assert-Equal $true ($j[2] -match '\|') 'the divider is drawn on every line, including padded ones'

# --- viewport ----------------------------------------------------------------------------
$v = Get-Viewport -Count 100 -Index 0 -Visible 10
Assert-Equal 0 $v.Start 'at the top the viewport starts at zero'
$v = Get-Viewport -Count 100 -Index 99 -Visible 10
Assert-Equal 90 $v.Start 'at the bottom the viewport stops at the last full page'
$v = Get-Viewport -Count 100 -Index 50 -Visible 10
Assert-Equal 45 $v.Start 'in the middle the selection is centred'
$v = Get-Viewport -Count 3 -Index 0 -Visible 10
Assert-Equal 3 $v.Visible 'a short list never asks for more rows than it has'

# --- command output -> box rows -----------------------------------------------------------
# Content assertions on purpose. "No row exceeds the width" cannot fail here: the caller pipes
# every row through Limit-Line anyway, so a row that was silently mangled would still pass it.
$o = @(Split-OutputLines -Text "alpha`nbeta`ngamma" -Width 40)
Assert-Equal 3 $o.Count 'each physical line becomes its own row'
Assert-Equal 'alpha' $o[0] 'the first line keeps its identity'
Assert-Equal 'gamma' $o[2] 'the last line keeps its identity'
Assert-Equal 0 (@($o | Where-Object { $_ -match "`n" }).Count) 'no row carries a newline into the layout'

# CRLF is what a native Windows process actually emits. These two assertions do NOT discriminate
# the "`r?" in the split regex: measured 2026-08-13, Split-TextLines splits on \s+ and so eats a
# trailing CR by itself, and "`r".Trim() is empty so a CRLF blank line is recognised either way -
# splitting on "`n" alone passes both. They are kept as a contract guard: the day the wrapper is
# replaced by one that preserves a raw line, the CR becomes visible and these fire.
$crlf = @(Split-OutputLines -Text "alpha`r`nbeta" -Width 40)
Assert-Equal 2 $crlf.Count 'CRLF output splits into two rows'
Assert-Equal 'alpha' $crlf[0] 'no carriage return reaches the row (enforced downstream, not by the split)'

$blanks = @(Split-OutputLines -Text "`n`nalpha`n`n`nbeta`n`n" -Width 40)
Assert-Equal 3 $blanks.Count 'runs of blank lines collapse to one and the outer ones are dropped'
Assert-Equal 'alpha' $blanks[0] 'leading blank lines do not push the content down'
Assert-Equal '' $blanks[1] 'one blank line survives as a separator'

$wrapped = @(Split-OutputLines -Text 'aaa bbb ccc ddd eee' -Width 8)
Assert-Equal $true ($wrapped.Count -gt 1) 'a line longer than the width wraps rather than being cut'
Assert-Equal $true (($wrapped -join ' ') -match 'eee') 'the tail of a wrapped line is still on screen'

# The cap keeps the HEAD. `claude doctor` puts version, path and install method at the top and
# boilerplate at the bottom, which is why keeping the last three lines showed nothing useful.
$capped = @(Split-OutputLines -Text ((1..10 | ForEach-Object { "line $_" }) -join "`n") -Width 40 -MaxLines 4)
Assert-Equal 4 $capped.Count 'the cap is honoured exactly'
Assert-Equal 'line 1' $capped[0] 'the cap keeps the head of the report'
Assert-Equal $true ($capped[3] -match '\(\+7 more lines\)') 'the marker counts the rows that were dropped'
Assert-Equal 0 (@($capped | Where-Object { $_ -eq 'line 10' }).Count) 'the tail beyond the cap is not shown'

# MaxLines 1 is the edge that bites: `0..($keep - 1)` with keep 0 is `0..-1`, which in PowerShell
# counts DOWN and would hand back two rows where none were asked for.
$one = @(Split-OutputLines -Text ((1..10 | ForEach-Object { "line $_" }) -join "`n") -Width 40 -MaxLines 1)
Assert-Equal 1 $one.Count 'a cap of one row returns exactly one row'
Assert-Equal $true ($one[0] -match '\(\+10 more lines\)') 'that single row is the marker, not a line pretending to be the whole report'

Assert-Equal 0 @(Split-OutputLines -Text '' -Width 40).Count 'empty output produces no rows'
Assert-Equal 0 @(Split-OutputLines -Text "  `n `n" -Width 40).Count 'whitespace-only output produces no rows'

if ($script:Ran -ne 51) { Write-Host "COULD NOT RUN: expected 51 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
