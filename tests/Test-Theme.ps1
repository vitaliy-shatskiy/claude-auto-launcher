# Assertions for Theme.ps1. Run: pwsh -NoProfile -File Test-Theme.ps1
try { . "$PSScriptRoot\..\claude-auto\Theme.ps1" } catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }
try { . "$PSScriptRoot\..\claude-auto\Layout.ps1" } catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }

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
function Assert-True {
    param([bool]$Actual, [string]$Because)
    $script:Ran++
    if (-not $Actual) {
        Write-Host "FAIL  $Because"
        $script:Failed++
    } else {
        Write-Host "ok    $Because"
    }
}

$E = [char]27

# Percentages carry meaning, not decoration: a 97% week must not look like a 15% one.
Assert-Equal "$E[31m" (Get-PercentColor -Percent 97) '97% is red'
Assert-Equal "$E[33m" (Get-PercentColor -Percent 72) '72% is yellow'
Assert-Equal "$E[32m" (Get-PercentColor -Percent 15) '15% is green'

# Stripping must remove every form of escape this file emits, including the 256-colour accent.
Assert-Equal 'plain' (Remove-AnsiColor -Text ($script:Palette.Accent + 'plain' + $script:Palette.Reset)) 'accent escapes strip cleanly'
Assert-Equal 'ab' (Remove-AnsiColor -Text ("a$E[38;5;209mb")) 'partial 256-colour escape strips cleanly'

# Footer-button palette (Task 6, spec D1), pinned by the LITERAL escape rather than through
# $script:Palette.* - the Test-Ui suite only ever compares $script:Palette.ButtonBg to itself, so a typo in
# one digit here (e.g. 48;5;208m) would still be self-consistent and stay green there.
Assert-Equal "$E[48;5;238m" $script:Palette.ButtonBg 'ButtonBg is the dim inverse background'
Assert-Equal "$E[38;5;250m" $script:Palette.ButtonFg 'ButtonFg is the dim inverse foreground'
Assert-Equal "$E[48;5;209m" $script:Palette.AccentBg 'AccentBg is the warm accent background'
Assert-Equal "$E[38;5;232m" $script:Palette.AccentFg 'AccentFg is the warm accent foreground'

# Bars are plain text of exactly the requested width - they are laid out before colour exists.
$bar = New-Bar -Percent 50 -Width 8
Assert-Equal 8 $bar.Length 'a bar is exactly the requested width'
Assert-Equal 0 (@($bar.ToCharArray() | Where-Object { $_ -eq $E }).Count) 'a bar carries no escapes'
Assert-Equal 8 (New-Bar -Percent 100 -Width 8).Length '100% fills the whole bar'
Assert-Equal 8 (New-Bar -Percent 0 -Width 8).Length '0% still occupies the whole bar'
Assert-Equal (New-Bar -Percent 100 -Width 8) (New-Bar -Percent 140 -Width 8) 'over 100% clamps rather than overflowing'
Assert-Equal (New-Bar -Percent 0 -Width 8) (New-Bar -Percent -5 -Width 8) 'below 0% clamps rather than underflowing'

# The LENGTH assertions above pass even for a bar that ignores Percent entirely and always renders
# empty - mutation-proved: a New-Bar that drops the Percent parameter still returns $Width empty
# glyphs, and every assertion above stays green. These count the actual FILL, which is the one
# thing the bars exist to show - it is how the owner picks which account still has budget.
$barGlyphs = Get-Glyphs
$fullGlyph = $barGlyphs.BarFull; $emptyGlyph = $barGlyphs.BarEmpty
function Count-Glyph([string]$Bar, [string]$Glyph) { @($Bar.ToCharArray() | Where-Object { "$_" -ceq $Glyph }).Count }
Assert-Equal 8 (Count-Glyph (New-Bar -Percent 100 -Width 8) $fullGlyph) '100% is eight full glyphs, not an empty bar of the right length'
Assert-Equal 0 (Count-Glyph (New-Bar -Percent 100 -Width 8) $emptyGlyph) 'and no empty glyphs at all'
Assert-Equal 0 (Count-Glyph (New-Bar -Percent 0 -Width 8) $fullGlyph) '0% is zero full glyphs'
Assert-Equal 8 (Count-Glyph (New-Bar -Percent 0 -Width 8) $emptyGlyph) 'and eight empty glyphs'
Assert-Equal 4 (Count-Glyph (New-Bar -Percent 50 -Width 8) $fullGlyph) '50% of an 8-wide bar is exactly four full glyphs'
Assert-Equal 4 (Count-Glyph (New-Bar -Percent 50 -Width 8) $emptyGlyph) 'and exactly four empty ones - the split, not just the total'
# -Line: the narrow tier's thin bar, the same split in the box-drawing pair.
Assert-Equal (([string]$barGlyphs.BarLineFull * 6) + ([string]$barGlyphs.BarLineEmpty * 2)) (New-Bar -Percent 75 -Width 8 -Line) '-Line draws the same split with the thin full and empty glyphs'

# Both glyph sets must exist and cover the same keys, or a screen renders $null somewhere.
$uni = Get-Glyphs
$asc = Get-Glyphs -Ascii
Assert-Equal ($uni.Keys.Count) ($asc.Keys.Count) 'both glyph sets define the same number of keys'
$missing = @($uni.Keys | Where-Object { -not $asc.ContainsKey($_) })
Assert-Equal 0 $missing.Count 'the ASCII set covers every Unicode key'
$wide = @($asc.Values | Where-Object { $_.Length -ne 1 })
Assert-Equal 0 $wide.Count 'every ASCII glyph is exactly one character wide'
$wideU = @($uni.Values | Where-Object { $_.Length -ne 1 })
Assert-Equal 0 $wideU.Count 'every Unicode glyph is exactly one character wide'

# NO_COLOR=1 disables color output by convention (no-color.org).
$savedNOCOLOR = $env:NO_COLOR
$env:NO_COLOR = '1'
Assert-Equal $false (Test-ColorSupported) 'NO_COLOR=1 disables color output'
if ($null -eq $savedNOCOLOR) { [Environment]::SetEnvironmentVariable('NO_COLOR', $null, 'Process') } else { $env:NO_COLOR = $savedNOCOLOR }

# TERM=dumb signals a terminal that cannot render ANSI escapes.
$savedTERM = $env:TERM
$env:TERM = 'dumb'
Assert-Equal $false (Test-ColorSupported) 'TERM=dumb signals no escape support'
if ($null -eq $savedTERM) { [Environment]::SetEnvironmentVariable('TERM', $null, 'Process') } else { $env:TERM = $savedTERM }

# With neither NO_COLOR nor TERM=dumb, color is supported.
$savedNOCOLOR2 = $env:NO_COLOR
$savedTERM2 = $env:TERM
[Environment]::SetEnvironmentVariable('NO_COLOR', $null, 'Process')
[Environment]::SetEnvironmentVariable('TERM', $null, 'Process')
Assert-Equal $true (Test-ColorSupported) 'color is supported when neither NO_COLOR nor TERM=dumb are set'
if ($null -eq $savedNOCOLOR2) { [Environment]::SetEnvironmentVariable('NO_COLOR', $null, 'Process') } else { $env:NO_COLOR = $savedNOCOLOR2 }
if ($null -eq $savedTERM2) { [Environment]::SetEnvironmentVariable('TERM', $null, 'Process') } else { $env:TERM = $savedTERM2 }

# CLAUDE_AUTO_ASCII=1 forces ASCII glyph rendering for incompatible terminals.
$savedASCII = $env:CLAUDE_AUTO_ASCII
$env:CLAUDE_AUTO_ASCII = '1'
Assert-Equal $true (Test-AsciiRequired) 'CLAUDE_AUTO_ASCII=1 forces ASCII glyphs'
if ($null -eq $savedASCII) { [Environment]::SetEnvironmentVariable('CLAUDE_AUTO_ASCII', $null, 'Process') } else { $env:CLAUDE_AUTO_ASCII = $savedASCII }

# --- every glyph is one cell on Windows Terminal: no emoji, no wide, no emoji-presentation ----------
foreach ($ascii in $false, $true) {
    $g = Get-Glyphs -Ascii:$ascii
    foreach ($k in $g.Keys) {
        $s = [string]$g[$k]
        # deferred-minors sweep (17.09.2026) G3: every UTF-16 code unit, not just $s[0] - a surrogate PAIR (an astral glyph)
        # printed only its first half here, which reads as one BMP character and hides which of the
        # two code units is the one Get-DisplayWidth actually scored.
        $units = (@($s.ToCharArray()) | ForEach-Object { '{0:X4}' -f [int]$_ }) -join ' '
        Assert-Equal 1 (Get-DisplayWidth -Text $s) "glyph $k (hex $units) is exactly one cell (ascii=$ascii)"
    }
}
Assert-True (-not ([string](Get-Glyphs).Bullet).Contains([char]0x23FA)) 'the free-path bullet is no longer U+23FA, which Windows Terminal draws as an emoji'

# --- dim spans (spec D6): the two markers a builder wraps a column in while the line is still plain
# text. C0 controls on purpose - Get-CodePointWidth measures everything under 0x20 as zero cells, so
# the layout arithmetic never sees them, and Add-DimSpanColor turns them into palette escapes (or
# strips them) only after the line is laid out.
Assert-Equal 1 ([int][char]$script:DimOpen) 'the dim-span open marker is U+0001'
Assert-Equal 2 ([int][char]$script:DimClose) 'and its close marker is U+0002'
Assert-Equal ($script:Palette.Dim + 'x' + $script:Palette.Reset) (Add-DimSpanColor -Line ([string]$script:DimOpen + 'x' + [string]$script:DimClose) -Enabled) 'with colour on, a span becomes Dim ... Reset'
Assert-Equal 'x' (Add-DimSpanColor -Line ([string]$script:DimOpen + 'x' + [string]$script:DimClose)) 'and with colour off it is stripped to the bare text'

# Hover-band markers (spec D10). Same C0 contract as the dim pair above - zero cells, wrapped on
# plain text by a builder, resolved once per line - with one difference that is the whole point of a
# second pair: this one is resolved LAST, after the pattern painters, so the band can re-assert its
# background behind every Reset they wove into the row. A single pair could not: the dim spans have
# to be resolved first, before those same painters go looking for glyphs and words.
Assert-Equal 4 ([int][char]$script:HoverOpen) 'the hover-band open marker is U+0004'
Assert-Equal 5 ([int][char]$script:HoverClose) 'and its close marker is U+0005'
Assert-Equal 0 (Get-DisplayWidth -Text ([string]$script:HoverOpen + [string]$script:HoverClose)) 'both cost zero cells, so a banded row lays out by the same numbers as a plain one'
$band = Add-HoverSpanColor -Line ("ab" + (Add-HoverSpan -Text ("cd" + $script:Palette.Dim + "ee" + $script:Palette.Reset + "f")) + "g") -Enabled
Assert-Equal ("ab" + $script:Palette.ButtonBg + "cd" + $script:Palette.Dim + "ee" + $script:Palette.Reset + $script:Palette.ButtonBg + "f" + $script:Palette.Reset + "g") $band 'the band re-asserts its background after every inner Reset'
Assert-Equal 'abcdeefg' (Remove-AnsiColor -Text $band) 'painting never changes the text'
Assert-Equal 'abcdf' (Add-HoverSpanColor -Line ("ab" + (Add-HoverSpan -Text 'cd') + "f")) 'colour off strips the markers'
Assert-Equal ($script:Palette.ButtonBg + 'xy' + $script:Palette.Reset) (Add-HoverSpanColor -Line ([string]$script:HoverOpen + 'xy') -Enabled) 'an unpaired open marker is closed at the end of the line'

# The early-out has NO observable of its own: the regex behind it matches nothing on an unmarked line
# and [regex]::Replace hands back the very instance it was given, so the bytes AND the reference are
# the same whether the guard fires or not (measured: a ReferenceEquals pin passes over a dead guard).
# What can be pinned is the rule it broke - String.IndexOf(String) is culture-sensitive and a C0
# control has zero collation weight, so the [string] overload "finds" a marker at index 0 of EVERY
# line and the guard never fired; every body line of every frame paid the regex for it (0.88 ms/line,
# 44 ms a 50-line frame). The first two name the rule, the third holds the one line that obeys it.
$unmarked = 'an ordinary frame line with no markers at all'
Assert-Equal 0 ($unmarked.IndexOf([string][char]4)) 'the rule under test: IndexOf(STRING) is culture-sensitive and finds a zero-weight C0 marker at index 0 of a marker-FREE line'
Assert-Equal -1 ($unmarked.IndexOf([char]4)) 'while the [char] overload is ordinal - the only form a marker guard may use'
$themeSource = [IO.File]::ReadAllText("$PSScriptRoot\..\claude-auto\Theme.ps1")
Assert-True ($themeSource -match '\$Line\.IndexOf\(\$script:HoverOpen\) -lt 0 -and \$Line\.IndexOf\(\$script:HoverClose\) -lt 0') 'and the band painter''s early-out tests the [char] markers themselves, never the [string] copies it paints with'

# --- palette name pin. The palette was the one-letter `C` at script scope, and PowerShell names are
# case-insensitive: any `$c = ...` at SCRIPT scope in a script that dot-sources Theme.ps1 replaced
# it, and every later frame rendered plain with no error. It is `$script:Palette` now. The pin reads
# every tracked .ps1: no one-letter `$script:`/`$global:` name (the old palette among them),
# `$c.<palette key>` is never read as the palette, and nothing outside Theme.ps1 assigns Palette at
# script scope (the same shadowing, under the new name; a function-local cannot shadow it).
function Find-PaletteShadow {
    param([Parameter(Mandatory)][Management.Automation.Language.Ast]$Ast, [string[]]$Keys = @(), [switch]$IsTheme)
    $hits = [Collections.Generic.List[string]]::new()
    $vars = $Ast.FindAll({ param($n) $n -is [Management.Automation.Language.VariableExpressionAst] }, $true)
    foreach ($v in $vars) {
        $path = $v.VariablePath.UserPath
        $name = $path -replace '^(?i)(script|global|local|private):', ''
        $qualified = $path -imatch '^(script|global):'
        $where = "line $($v.Extent.StartLineNumber): $($v.Extent.Text)"
        if ($path -imatch '^(script|global):C$') { $hits.Add("$where - the old palette name"); continue }
        if ($path -imatch '^(script|global):[a-z]$') { $hits.Add("$where - a one-letter script-scope name, shadowed by any same-letter variable of a dot-sourcing script"); continue }
        $p = $v.Parent
        if ($name -ieq 'c' -and $p -is [Management.Automation.Language.MemberExpressionAst] -and $p -isnot [Management.Automation.Language.InvokeMemberExpressionAst] -and
            [object]::ReferenceEquals($p.Expression, $v) -and $p.Member -is [Management.Automation.Language.StringConstantExpressionAst] -and $Keys -icontains $p.Member.Value) {
            $hits.Add("$where.$($p.Member.Value) - reads `$c as the palette"); continue
        }
        if ($name -ieq 'Palette' -and -not $IsTheme) {
            $target = if ($p -is [Management.Automation.Language.ConvertExpressionAst]) { $p } else { $v }
            $a = $target.Parent
            if ($a -is [Management.Automation.Language.AssignmentStatementAst] -and [object]::ReferenceEquals($a.Left, $target)) {
                $inFunction = $false
                for ($up = $a.Parent; $up; $up = $up.Parent) { if ($up -is [Management.Automation.Language.FunctionDefinitionAst]) { $inFunction = $true; break } }
                if ($qualified -or -not $inFunction) { $hits.Add("$where - assigns Palette at script scope outside Theme.ps1") }
            }
        }
    }
    return , $hits
}
$paletteKeys = @(if ($script:Palette -is [hashtable]) { $script:Palette.Keys })
Assert-True (($script:Palette -is [hashtable]) -and $script:Palette.ContainsKey('Reset') -and $script:Palette.ContainsKey('AccentBg')) 'the palette is $script:Palette, a hashtable holding Reset and AccentBg'
$ctl = { param([string]$Code, [switch]$IsTheme) (Find-PaletteShadow -Ast ([Management.Automation.Language.Parser]::ParseInput($Code, [ref]$null, [ref]$null)) -Keys $paletteKeys -IsTheme:$IsTheme).Count }
$ctlBad = @(
    (& $ctl '$Script:c = @{}'), (& $ctl '$x = $SCRIPT:c.Dim'), (& $ctl '$c.Reset'), (& $ctl '$Palette = 1'), (& $ctl '[hashtable]$script:palette = @{}'),
    (& $ctl '$script:e = [char]27'), (& $ctl 'function f { $script:Palette = 1 }')
)
Assert-Equal '1 1 1 1 1 1 1' ($ctlBad -join ' ') 'bad controls: the pin names the old palette name, a one-letter script-scope name, $c read as the palette and a script-scope Palette assignment outside Theme.ps1'
$ctlGood = @(
    (& $ctl '$c.X; $c.Length'), (& $ctl '$Palette = 1' -IsTheme), (& $ctl '$pal = $script:Palette; $pal.Dim'),
    (& $ctl '$c.Reset()'), (& $ctl 'function f { $palette = 1 }'), (& $ctl 'function f { [hashtable]$Palette = @{} }')
)
Assert-Equal '0 0 0 0 0 0' ($ctlGood -join ' ') 'good controls: a non-palette $c member, a method call on $c, a function-local Palette and the theme itself pass'
$repoRoot = (Resolve-Path "$PSScriptRoot\..").Path
# Tracked files only: an untracked scratch script in a working checkout must not turn this red.
$tracked = @(& git -C $repoRoot ls-files -- '*.ps1' '*.psm1' 2>$null)
if ($LASTEXITCODE -ne 0 -or $tracked.Count -eq 0) { Write-Host "COULD NOT RUN: git ls-files returned nothing under $repoRoot"; exit 2 }
$offenders = [Collections.Generic.List[string]]::new()
$scripts = @($tracked | ForEach-Object { Join-Path $repoRoot $_ } | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf })
foreach ($full in $scripts) {
    $parseErrors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile($full, [ref]$null, [ref]$parseErrors)
    $rel = $full.Substring($repoRoot.Length + 1) -replace '/', '\'
    if ($parseErrors) { $offenders.Add("$rel - does not parse: $($parseErrors[0].Message)"); continue }
    $isTheme = $rel -ieq 'claude-auto\Theme.ps1'
    foreach ($h in (Find-PaletteShadow -Ast $ast -Keys $paletteKeys -IsTheme:$isTheme)) { $offenders.Add("$rel $h") }
}
foreach ($o in $offenders) { Write-Host "      $o" }
Assert-True (($scripts.Count -gt 20) -and ($offenders.Count -eq 0)) "no tracked script uses a one-letter script-scope name, reads `$c as the palette or assigns Palette at script scope outside Theme.ps1 ($($scripts.Count) scripts read, $($offenders.Count) offenders)"

if ($script:Ran -ne 91) { Write-Host "COULD NOT RUN: expected 91 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
