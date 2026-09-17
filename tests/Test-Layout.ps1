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

# Characters built from code points, never typed: a literal CJK or emoji in this file is at the
# mercy of every editor and shell that touches it, and a normalised character tests nothing.
$script:CJK = [string][char]0x4E2D                    # a Chinese ideograph - two columns
$script:Emoji = [char]::ConvertFromUtf32(0x1F600)     # one code point, TWO UTF-16 code units
$script:Combining = 'e' + [string][char]0x0301        # e + combining acute - two code units, one cell

# The marker is pinned so the suite does not depend on the runner's console code page: Get-Ellipsis
# falls back to Test-AsciiRequired, and a CI runner whose console is cp437 would otherwise flip
# every truncation marker under this line. The ASCII section at the end clears the pin on purpose.
$script:Ellipsis = [string][char]0x2026

function Get-LoneSurrogateCount {
    # A surrogate without its partner is not a character: the console renders a replacement box and
    # some terminals lose the rest of the line. Any cut that produces one is a bug.
    param([string]$Text)
    $n = 0
    for ($i = 0; $i -lt $Text.Length; $i++) {
        if ([char]::IsHighSurrogate($Text[$i])) {
            if (($i + 1) -lt $Text.Length -and [char]::IsLowSurrogate($Text[$i + 1])) { $i++ } else { $n++ }
        } elseif ([char]::IsLowSurrogate($Text[$i])) { $n++ }
    }
    return $n
}

# --- display cells, not code units --------------------------------------------------------
# The console draws CELLS. .Length counts UTF-16 code units, and the two differ for exactly the
# content this launcher shows: a session's last message. Measured 2026-09-08 on the unfixed code,
# a picker rendered at -Width 60 with a Chinese message produced rows of len=60 / cells=100.
Assert-Equal 3 (Get-DisplayWidth -Text 'abc') 'ASCII text measures one cell per character'
Assert-Equal 6 (Get-DisplayWidth -Text ($script:CJK * 3)) 'a CJK ideograph measures two cells'
Assert-Equal 2 (Get-DisplayWidth -Text $script:Emoji) 'an astral emoji measures two cells, not its two code units'
Assert-Equal 1 (Get-DisplayWidth -Text $script:Combining) 'a combining mark adds no cell of its own'
Assert-Equal 0 (Get-DisplayWidth -Text '') 'empty text measures zero cells'
Assert-Equal 1 (Get-DisplayWidth -Text ([string][char]0x0416)) 'Cyrillic stays one cell - only East Asian and emoji are wide'
# The whole C0 range measures nothing, which is what lets Theme.ps1 mark a dim span with two of them
# (DimOpen/DimClose) while the row is still plain text: a marked row lays out exactly like an
# unmarked one, and Limit-Line spends no budget on a marker.
Assert-Equal 0 (Get-CodePointWidth -CodePoint 1) 'a C0 control character measures zero cells'
Assert-Equal 0 (Get-DisplayWidth -Text ([string]$script:DimOpen + [string]$script:DimClose)) 'so the dim-span markers cost a line nothing'
Assert-Equal 2 (Get-DisplayWidth -Text ([string]$script:DimOpen + 'ab' + [string]$script:DimClose)) 'and a marked segment measures only its content'

# --- the width loops decide ASCII and C0 without a call (spec D11) --------------------------
# One frame measures thousands of characters, nearly all of them ASCII or a C0 marker; a function
# call per character is what a resume screen cannot afford. The counters below are the pin: the
# table is reached only by the characters that actually need it. Breakpoint, not a wrapper, so the
# real function under test runs.
$script:cpCalls = 0
$bp = Set-PSBreakpoint -Command Get-CodePointWidth -Action { $script:cpCalls++ }
$null = Get-DisplayWidth -Text ('abc' + [char]1 + 'def')
Remove-PSBreakpoint $bp
Assert-Equal 0 $script:cpCalls 'ASCII and C0 never reach the per-codepoint path'
Assert-Equal 6 (Get-DisplayWidth -Text ('abc' + [char]1 + 'def')) 'and they measure exactly as before: six printable cells, nothing for the control'

$script:cpCalls = 0
$bp = Set-PSBreakpoint -Command Get-DisplayWidth -Action { $script:cpCalls++ }
$null = Limit-Cells -Text 'abcdefgh' -Max 5
$null = Limit-CellsRight -Text 'abcdefgh' -Max 5
Remove-PSBreakpoint $bp
Assert-Equal 0 $script:cpCalls 'the cell-budget loops measure an ASCII character inline, not through a call per character'
Assert-Equal 'abcde' (Limit-Cells -Text 'abcdefgh' -Max 5) 'and Limit-Cells still returns the longest prefix that fits'
Assert-Equal 'defgh' (Limit-CellsRight -Text 'abcdefgh' -Max 5) 'and Limit-CellsRight the longest suffix'

# One string carrying all four cases at once, whose total the fast path and the table must agree on:
# 1 (ASCII) + 2 (CJK) + 1 (ASCII) + 0 (combining mark) + 2 (astral emoji).
Assert-Equal 6 (Get-DisplayWidth -Text ('a' + $script:CJK + 'b' + [string][char]0x0301 + $script:Emoji)) 'a mixed ASCII/CJK/combining/astral string measures the sum of its parts'

# --- the whole-STRING width memo ------------------------------------------------------------
# The character loop above is the most-called piece of a repaint and the same strings arrive over
# and over. A POISONED entry is the pin: a second call that still ran the loop would answer 3.
$script:WidthOfText.Clear()
Assert-Equal 3 (Get-DisplayWidth -Text 'qzx') 'a string measured for the first time still measures its own cells'
Assert-Equal $true ($script:WidthOfText.ContainsKey('qzx')) 'and the answer is kept, so the next row carrying that text costs no loop'
$script:WidthOfText['qzx'] = 99
Assert-Equal 99 (Get-DisplayWidth -Text 'qzx') 'a second call with the same text is answered from the memo, without re-entering the loop'
$script:WidthOfText.Clear()
Assert-Equal 3 (Get-DisplayWidth -Text 'qzx') 'and clearing the memo puts the loop back'
# Bounded: a filter typed character by character is the only thing here that can grow the table, and
# it is dropped whole rather than evicted entry by entry.
for ($memoPad = 0; $memoPad -lt 20000; $memoPad++) { $script:WidthOfText["pad$memoPad"] = 1 }
$null = Get-DisplayWidth -Text 'qzx2'
Assert-Equal 1 $script:WidthOfText.Count 'past 20000 entries the width memo is dropped whole, so it cannot grow without bound'
$script:WidthOfText.Clear()

# --- truncation --------------------------------------------------------------------------
Assert-Equal 'abc' (Limit-Line -Text 'abc' -Max 10) 'a short line is untouched'
Assert-Equal 4 (Limit-Line -Text 'abcdefgh' -Max 4).Length 'a long line is cut to the width'
Assert-Equal $true (Limit-Line -Text 'abcdefgh' -Max 4).EndsWith([string][char]0x2026) 'truncation ends with an ellipsis'
Assert-Equal '' (Limit-Line -Text $null -Max 10) 'null text becomes an empty line'

# Truncation is a CELL budget. `.Length -le $Max` let a 60-cell budget through 60 wide characters.
Assert-Equal $true ((Get-DisplayWidth -Text (Limit-Line -Text ($script:CJK * 50) -Max 20)) -le 20) 'a CJK line is truncated to the cell width, not the code-unit count'
Assert-Equal ($script:CJK * 8) (Limit-Line -Text ($script:CJK * 8) -Max 16) 'a CJK line that exactly fits its cell budget is untouched'
Assert-Equal $true ((Get-DisplayWidth -Text (Limit-Line -Text ($script:CJK * 8) -Max 15)) -le 15) 'one cell short of fitting, a CJK line is still cut to the budget'
# Cutting between a high surrogate and its low half leaves a lone surrogate - U+D83D on its own.
Assert-Equal 0 (Get-LoneSurrogateCount -Text (Limit-Line -Text ('ab' + $script:Emoji + 'cd') -Max 4)) 'truncation never cuts a surrogate pair in half'
Assert-Equal $true ((Get-DisplayWidth -Text (Limit-Line -Text ($script:Emoji * 10) -Max 7)) -le 7) 'an emoji run is truncated to the cell width'

# --- middle truncation for a path (spec D4) ------------------------------------------------
# Cut from the RIGHT, every deep path under one root renders as the identical prefix, so the column
# that is supposed to say WHICH project says nothing. 60 cells into 30 is the shape the project
# screen actually hits at 50 columns.
$deepPath = 'C:\Users\sample\Desktop\Projects\alpha'     # 38 cells
$deeper = 'C:\Users\sample\Desktop\Projects\workspace\services\alpha'
Assert-Equal 57 (Get-DisplayWidth -Text $deeper) 'the deep-path fixture is 57 cells wide - well past the budget below'
$cut30 = Limit-Path -Text $deeper -Max 30
Assert-Equal $true ((Get-DisplayWidth -Text $cut30) -le 30) 'a 60-cell path cut to 30 fits the budget'
Assert-Equal $true ($cut30.EndsWith('\alpha')) 'and keeps its leaf, which is the half that tells two rows apart'
Assert-Equal $true ($cut30.StartsWith('C:\')) 'and still starts with the drive, which is the half that says where it is'
Assert-Equal 1 (@($cut30.ToCharArray() | Where-Object { [int]$_ -eq 0x2026 }).Count) 'with exactly one marker between the two'
# Two paths under the SAME root differ after the cut - the whole point of D4.
$sibA = Limit-Path -Text 'C:\Users\sample\Desktop\Projects\workspace\alpha' -Max 30
$sibB = Limit-Path -Text 'C:\Users\sample\Desktop\Projects\workspace\beta' -Max 30
Assert-Equal $true ($sibA -ne $sibB) 'two deep paths under one root stay distinguishable after the cut'
Assert-Equal $deepPath (Limit-Path -Text $deepPath -Max 40) 'a path that already fits is untouched'
Assert-Equal '' (Limit-Path -Text $null -Max 20) 'null text becomes an empty string'
# A leaf too long for the budget keeps its own END rather than pushing the head out entirely.
$longLeaf = Limit-Path -Text ('C:\root\' + ('n' * 60)) -Max 20
Assert-Equal $true ((Get-DisplayWidth -Text $longLeaf) -le 20) 'a path whose LEAF alone overflows still fits the budget'
Assert-Equal $true ($longLeaf.StartsWith('C')) 'and still starts with the drive'
Assert-Equal 0 (Get-LoneSurrogateCount -Text (Limit-Path -Text ('C:\a\' + ($script:Emoji * 10) + '\' + ($script:Emoji * 10)) -Max 12)) 'middle truncation never cuts a surrogate pair in half'
Assert-Equal $true ((Get-DisplayWidth -Text (Limit-Path -Text ('C:\a\' + ($script:CJK * 30)) -Max 21)) -le 21) 'a CJK path is cut by cells, not code units'

# deferred-minors sweep (17.09.2026) G3: degenerate Limit-Path inputs, read straight off the function rather than
# changed to match a guess - Max <= 3 has no room for a head, a marker AND a tail, so it falls
# back to Limit-Line's own plain right cut.
foreach ($degMax in @(0, 1, 2, 3)) {
    Assert-Equal (Limit-Line -Text $deeper -Max $degMax) (Limit-Path -Text $deeper -Max $degMax) "Limit-Path at Max $degMax falls back to Limit-Line's plain right cut"
}
# No separator at all: LastIndexOfAny finds none, the leaf is empty, and the tail comes from
# Limit-CellsRight of the WHOLE text instead - both halves still fit inside the budget.
# Fix round 1, Minor 5: a DISTINGUISHABLE fixture, not 60 identical 'n's - a uniform string leaves
# the tail's CONTENT unpinned (a mutation that replaced the fallback tail with '' still passed the
# width and marker-count checks below, since nothing here read what the tail actually said).
$noSepText = ('a' * 55) + 'zzzzz'
$noSep = Limit-Path -Text $noSepText -Max 20
Assert-Equal $true ((Get-DisplayWidth -Text $noSep) -le 20) 'a path with no separator still fits the budget'
Assert-Equal 1 (@($noSep.ToCharArray() | Where-Object { [int]$_ -eq 0x2026 }).Count) 'and still carries exactly one marker'
Assert-Equal $true ($noSep.EndsWith('zzzzz')) 'and the tail is the actual END of the text (Limit-CellsRight), not an unpinned fallback'
# A UNC path: the leaf after the last separator survives, and the head still starts with the
# server name rather than being cut into the middle of the \\ prefix.
$unc = Limit-Path -Text '\\server\share\deep\nested\path\leaf' -Max 20
Assert-Equal $true ((Get-DisplayWidth -Text $unc) -le 20) 'a UNC path cut to budget still fits'
Assert-Equal $true ($unc.EndsWith('\leaf')) 'and keeps its leaf'
Assert-Equal $true ($unc.StartsWith('\\')) 'starting with the UNC prefix, not a mid-cut server name'

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

# --- word wrap in cells ---------------------------------------------------------------------
$w = @(Split-TextLines -Text ((($script:CJK * 8) + ' ') * 4) -Width 12)
Assert-Equal 0 (@($w | Where-Object { (Get-DisplayWidth -Text $_) -gt 12 }).Count) 'no wrapped line exceeds the width in cells'
Assert-Equal $true ($w.Count -gt 1) 'CJK text wraps rather than being welded into one over-wide line'

# An over-long token is cut. Cutting on code units puts U+D83D at the end of one line and U+DE00 at
# the start of the next - two broken characters where there was one emoji.
$w = @(Split-TextLines -Text ($script:Emoji * 6) -Width 5)
Assert-Equal 0 (@($w | Where-Object { (Get-LoneSurrogateCount -Text $_) -ne 0 }).Count) 'wrapping never splits a surrogate pair across two lines'
Assert-Equal 0 (@($w | Where-Object { (Get-DisplayWidth -Text $_) -gt 5 }).Count) 'an over-long emoji token is cut on cell boundaries'
Assert-Equal 12 ((@($w | ForEach-Object { Get-DisplayWidth -Text $_ }) | Measure-Object -Sum).Sum) 'no emoji is dropped while cutting - six of them, twelve cells'

$w = @(Split-TextLines -Text ((($script:CJK * 10) + ' ') * 5) -Width 20 -MaxLines 2)
Assert-Equal 2 $w.Count 'MaxLines caps a CJK wrap too'
Assert-Equal 0 (@($w | Where-Object { (Get-DisplayWidth -Text $_) -gt 20 }).Count) 'a capped CJK wrap still fits the width in cells'

# A pane narrower than a single glyph has no correct answer, but it must terminate and it must
# never emit a row wider than the pane - an over-wide row wraps and desynchronises every row below.
$narrow = @(Split-TextLines -Text ($script:CJK * 4) -Width 1)
Assert-Equal 0 (@($narrow | Where-Object { (Get-DisplayWidth -Text $_) -gt 1 }).Count) 'a pane narrower than one glyph emits no over-wide row'

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

# --- boxes in cells ------------------------------------------------------------------------
# The reproduction: a 60-column picker with a CJK last message drew rows of 100 cells. PadRight
# counts code units too, so a box whose content is narrow in cells was padded PAST its own border.
$box = New-Box -Lines @(($script:CJK * 40), 'ascii', ($script:CJK * 3), ($script:Emoji + ' hi')) -Width 60 -Ascii
Assert-Equal 0 (@($box | Where-Object { (Get-DisplayWidth -Text $_) -ne 60 }).Count) 'every box row is exactly the box width in cells, whatever the content'
Assert-Equal 0 (@($box | Where-Object { (Get-LoneSurrogateCount -Text $_) -ne 0 }).Count) 'no box row carries a lone surrogate'

$box = New-Box -Lines @('x') -Width 30 -Title ($script:CJK * 20) -Ascii
Assert-Equal 30 (Get-DisplayWidth -Text $box[0]) 'a CJK title does not push the top border past the box width'

# --- two panes ---------------------------------------------------------------------------
$j = Join-Panes -Left @('a', 'b', 'c') -Right @('1') -LeftWidth 10 -RightWidth 8 -Ascii
Assert-Equal 3 $j.Count 'the joined height is the taller pane'
Assert-Equal 0 (@($j | Where-Object { $_.Length -ne 19 }).Count) 'each joined line is left + divider + right'
Assert-Equal $true ($j[2] -match '\|') 'the divider is drawn on every line, including padded ones'

# The divider must stay in ONE column: measured in code units it drifts right by one per wide glyph.
$j = Join-Panes -Left @(($script:CJK * 9), 'a') -Right @('1', ($script:Emoji * 6)) -LeftWidth 10 -RightWidth 8 -Ascii
Assert-Equal 0 (@($j | Where-Object { (Get-DisplayWidth -Text $_) -ne 19 }).Count) 'each joined line is exactly left + divider + right in cells'
Assert-Equal 0 (@($j | ForEach-Object { Get-DisplayWidth -Text ($_.Substring(0, $_.IndexOf('|'))) } | Where-Object { $_ -ne 10 }).Count) 'the divider sits in the same column on every line'

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

# --- the ASCII fallback covers the truncation marker too -------------------------------------
# -Ascii exists because a console that is not on UTF-8 renders U+2026 as mojibake exactly like the
# box glyphs. Verified 2026-09-08 on the unfixed code with the console at cp866: New-Box -Ascii
# still returned '|a very long line <U+2026>|'. The pin is cleared here so the ambient decision is
# the thing under test.
function Get-NonAsciiCount {
    param([string[]]$Lines)
    return (@($Lines | ForEach-Object { $_.ToCharArray() } | Where-Object { [int]$_ -gt 126 })).Count
}
$script:Ellipsis = $null
$savedAscii = $env:CLAUDE_AUTO_ASCII
$env:CLAUDE_AUTO_ASCII = '1'
$asciiBox = @(New-Box -Lines @('a very long line that will certainly not fit in twenty columns') -Width 20 -Ascii)
Assert-Equal 0 (Get-NonAsciiCount -Lines $asciiBox) 'an ASCII box carries no character above ASCII, the truncation marker included'
Assert-Equal 20 (Get-DisplayWidth -Text $asciiBox[1]) 'the ASCII marker still fills the row to the box width'
Assert-Equal 0 (Get-NonAsciiCount -Lines @(Limit-Line -Text 'abcdefgh' -Max 4)) 'ASCII mode truncates a bare line with an ASCII marker'
# The middle-truncation marker comes from the same Get-Ellipsis, so -Ascii covers it too - one
# glyph set per frame, or a project row is half mojibake on a cp866 console. The ASCII marker is
# Get-Ellipsis's own '~' and stays exactly ONE cell: a three-dot '...' would spend two cells the
# truncation arithmetic never reserved and every row built on it would overflow.
$asciiPath = Limit-Path -Text 'C:\Users\sample\Desktop\Projects\workspace\alpha' -Max 24 -Ascii
Assert-Equal 0 (Get-NonAsciiCount -Lines @($asciiPath)) 'ASCII mode middle-truncates a path with an ASCII marker'
Assert-Equal $true ($asciiPath.Contains('~')) 'which is the one-cell marker Get-Ellipsis hands every other helper'
Assert-Equal $true ($asciiPath.EndsWith('\alpha')) 'and the leaf survives there too'
# deferred-minors sweep (17.09.2026) G3: ASCII mode on the same degenerate shapes tested above (no separator, UNC) - the
# marker is Get-Ellipsis's own one-cell '~', never the two-cell '...' the arithmetic never reserved
# room for. Same DISTINGUISHABLE fixture as the non-ASCII pin above (fix round 1, Minor 5).
$asciiNoSep = Limit-Path -Text $noSepText -Max 20 -Ascii
Assert-Equal 0 (Get-NonAsciiCount -Lines @($asciiNoSep)) 'ASCII mode on a separator-less path uses the ASCII marker'
Assert-Equal $true ($asciiNoSep.EndsWith('zzzzz')) 'and the tail there is still the actual end of the text, not an unpinned fallback'
Assert-Equal 0 (Get-NonAsciiCount -Lines @(Limit-Path -Text '\\server\share\deep\nested\path\leaf' -Max 20 -Ascii)) 'and so does ASCII mode on a UNC path'
Assert-Equal 0 (Get-NonAsciiCount -Lines @(Split-TextLines -Text 'one two three four five six seven' -Width 10 -MaxLines 2)) 'ASCII mode caps a wrap with an ASCII marker'
# The override channel, asserted without asking what the runner's console code page is: with the
# marker pinned, the ambient decision must not win. This is how the launcher hands its own -Ascii
# verdict to helpers that take no switch.
$script:Ellipsis = [string][char]0x2026
Assert-Equal $true ((Limit-Line -Text 'abcdefgh' -Max 4).EndsWith([string][char]0x2026)) 'an explicit marker overrides the ambient ASCII decision'
$script:Ellipsis = $null
# A caller that passes -Ascii explicitly gets the ASCII marker even where the console would allow
# U+2026: the frame and its marker must come from ONE glyph set, or a box is half mojibake.
Assert-Equal 0 (Get-NonAsciiCount -Lines @(New-Box -Lines @('a very long line that will certainly not fit') -Width 20 -Ascii)) 'an explicit -Ascii box keeps its marker in the same glyph set'
if ($null -eq $savedAscii) { [Environment]::SetEnvironmentVariable('CLAUDE_AUTO_ASCII', $null, 'Process') } else { $env:CLAUDE_AUTO_ASCII = $savedAscii }
$script:Ellipsis = [string][char]0x2026

if ($script:Ran -ne 123) { Write-Host "COULD NOT RUN: expected 123 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
