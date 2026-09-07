# Assertions for Theme.ps1. Run: pwsh -NoProfile -File Test-Theme.ps1
try { . "$PSScriptRoot\..\claude-auto\Theme.ps1" } catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }

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

$E = [char]27

# Percentages carry meaning, not decoration: a 97% week must not look like a 15% one.
Assert-Equal "$E[31m" (Get-PercentColor -Percent 97) '97% is red'
Assert-Equal "$E[33m" (Get-PercentColor -Percent 72) '72% is yellow'
Assert-Equal "$E[32m" (Get-PercentColor -Percent 15) '15% is green'

# Stripping must remove every form of escape this file emits, including the 256-colour accent.
Assert-Equal 'plain' (Remove-AnsiColor -Text ($script:C.Accent + 'plain' + $script:C.Reset)) 'accent escapes strip cleanly'
Assert-Equal 'ab' (Remove-AnsiColor -Text ("a$E[38;5;209mb")) 'partial 256-colour escape strips cleanly'

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

if ($script:Ran -ne 25) { Write-Host "COULD NOT RUN: expected 25 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
