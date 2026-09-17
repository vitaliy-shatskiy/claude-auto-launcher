# Palette, glyphs and the small pure helpers every screen paints with.
#
# Colour is applied AFTER a line is laid out and truncated, never during. An escape sequence adds
# invisible characters to .Length, so colouring first makes truncation cut in the wrong place and
# every width assertion measure the wrong thing.

$script:E = [char]27
$script:C = @{
    Reset        = "$([char]27)[0m"
    Bold         = "$([char]27)[1m"
    Dim          = "$([char]27)[2m"
    # Kept although nothing PAINTS with it: Test-Ui's "never bare reverse video" pin (spec D1) reads
    # it to prove the footer caps use the dim ButtonBg block instead.
    Reverse      = "$([char]27)[7m"
    Red          = "$([char]27)[31m"
    Green        = "$([char]27)[32m"
    Yellow       = "$([char]27)[33m"
    Blue         = "$([char]27)[34m"
    Magenta      = "$([char]27)[35m"
    Cyan         = "$([char]27)[36m"
    BrightCyan   = "$([char]27)[96m"
    BrightYellow = "$([char]27)[93m"
    BrightWhite  = "$([char]27)[97m"
    # Claude Code's own warm accent, as a 256-colour index so it survives terminals without truecolor.
    Accent       = "$([char]27)[38;5;209m"
    # Footer-button caps (Task 6, spec D1): idle is a dim inverse block, hover/selected the same
    # warm accent as Accent above but as a filled background so the button reads as PRESSABLE
    # rather than merely tinted text.
    ButtonBg     = "$([char]27)[48;5;238m"
    ButtonFg     = "$([char]27)[38;5;250m"
    AccentBg     = "$([char]27)[48;5;209m"
    AccentFg     = "$([char]27)[38;5;232m"
}

# Dim-span markers (spec D6). A builder wraps a COLUMN - a path, an age - in these two while the row
# is still plain text, and Complete-PickerFrame resolves them once the line is laid out. C0 controls
# on purpose: Get-CodePointWidth measures anything under 0x20 as zero cells, so a marked row lays out
# exactly like an unmarked one and the rule this file opens with survives a per-column tint that no
# pattern rule could find (a path is not a word, and the age is not always at the end of the line).
$script:DimOpen = [char]1
$script:DimClose = [char]2

function Add-DimSpanColor {
    # Resolves the dim-span markers: to Dim ... Reset with colour on, to nothing with it off. EVERY
    # line goes through it either way - a marker is an internal signal and must never reach a
    # terminal, a check reference or anything that stores what was drawn.
    #
    # An UNPAIRED open marker is closed at the end of the line rather than passed on: the markers
    # cost no cells, so Limit-Line spends the whole budget on visible text and can cut a row between
    # an open marker and its close - and a Dim with no Reset dims everything drawn after it.
    param([string]$Line, [switch]$Enabled)
    if (-not $Line) { return $Line }
    $open = [string]$script:DimOpen
    $close = [string]$script:DimClose
    if (-not $Enabled) { return ($Line -replace "[$open$close]", '') }
    $out = $Line -replace "$open([^$open$close]*)$close", ($script:C.Dim + '$1' + $script:C.Reset)
    if ($out.Contains($open)) { $out = ($out -replace $open, $script:C.Dim) + $script:C.Reset }
    return ($out -replace $close, $script:C.Reset)
}

# Hover-band markers (spec D10). Same contract as the dim-span pair above: zero cells, wrapped by a
# builder around already-plain text, resolved once per line by the painter below. A SECOND pair
# rather than a reuse of the first, because the two are resolved at opposite ends of the pass:
# the dim spans go first, before the pattern painters hunt for glyphs and words, and the band goes
# LAST - only then are the Resets those painters wove into the row there to be painted over, and a
# band that did not re-assert its background behind each of them would end at the first one.
$script:HoverOpen = [char]4
$script:HoverClose = [char]5

function Add-HoverSpan {
    # Marks a finished piece of PLAIN text as hovered. Wrapped after the caller's own width
    # arithmetic, never before it - the markers cost no cells, and that only holds if nothing
    # measured them as text (New-ListRow's dim markers carry the same rule).
    param([string]$Text)
    return ([string]$script:HoverOpen + $Text + [string]$script:HoverClose)
}

function Add-HoverSpanColor {
    # Resolves the hover markers: to a background band with colour on, to nothing with it off. Like
    # Add-DimSpanColor, EVERY body line goes through it either way - a marker is an internal signal
    # and must never reach a terminal, a check reference or anything that stores what was drawn.
    #
    # An UNPAIRED open marker is closed at the end of the line for the reason the dim painter records:
    # markers cost no cells, so Limit-Line spends the whole budget on visible text and can cut a row
    # between an open marker and its close - and a background with no Reset paints the rest of the row.
    param([string]$Line, [switch]$Enabled)
    if (-not $Line) { return $Line }
    $open = [string]$script:HoverOpen
    $close = [string]$script:HoverClose
    # Every body line of every frame passes here; an unmarked one costs two IndexOf and leaves with
    # the identical string it arrived as.
    # The [char] overload, never the [string] one: String.IndexOf(String) is CULTURE-sensitive, and a
    # C0 control has zero collation weight - so it is "found" at index 0 of every line, the early-out
    # never fired, and each body line paid the regex instead (measured 0.88 ms/line, 44 ms a frame).
    # Get-RowBandLength and Complete-PickerFrame already pass a [char] here, which is why only this
    # one line was dead. .Contains(String) is ordinal and needs no such care (measured).
    if ($Line.IndexOf($script:HoverOpen) -lt 0 -and $Line.IndexOf($script:HoverClose) -lt 0) { return $Line }
    if (-not $Enabled) { return ($Line -replace "[$open$close]", '') }
    # A MatchEvaluator rather than a replacement string: the band has to rewrite what it wraps (every
    # inner Reset becomes Reset + background again), which no '$1' replacement can express. It reads
    # $script:C rather than a local for CONSISTENCY with Add-LaunchColor's own evaluators, not out of
    # necessity: a plain scriptblock handed to [regex]::Replace still resolves its free names against
    # the scope that calls it, so the enclosing locals are in reach either way.
    # .Replace, not -replace, for SPEED - an ordinal string swap over an already-matched group, where
    # -replace would put a second regex on every match, and this runs once per banded line of every
    # frame. Not to dodge a '$'-group hazard: the substitution text is two escape sequences and has
    # no '$' in it to be read as a group reference.
    $paint = {
        param($m)
        $bg = $script:C.ButtonBg
        $rs = $script:C.Reset
        return ($bg + $m.Groups[1].Value.Replace($rs, $rs + $bg) + $rs)
    }
    $out = [regex]::Replace($Line, "$open([^$open$close]*)$close", $paint)
    if ($out.Contains($open)) { $out = [regex]::Replace($out + $close, "$open([^$open$close]*)$close", $paint) }
    return ($out -replace $close, '')
}

function Test-ColorSupported {
    # NO_COLOR is the de facto opt-out (no-color.org) and a dumb terminal cannot render escapes.
    if ($env:NO_COLOR) { return $false }
    if ($env:TERM -eq 'dumb') { return $false }
    return $true
}

function Remove-AnsiColor {
    # Used by the tests to prove painting never changes the text underneath.
    param([string]$Text)
    return ($Text -replace "$([char]27)\[[0-9;]*m", '')
}

function Get-PercentColor {
    param([int]$Percent)
    if ($Percent -ge 85) { return $script:C.Red }
    if ($Percent -ge 60) { return $script:C.Yellow }
    return $script:C.Green
}

function Test-AsciiRequired {
    # A console that is not on UTF-8 renders box drawing as mojibake, which is worse than ASCII.
    if ($env:CLAUDE_AUTO_ASCII -eq '1') { return $true }
    try { return ([Console]::OutputEncoding.CodePage -ne 65001) } catch { return $true }
}

function Get-Glyphs {
    # Every glyph is exactly one character wide. A two-cell glyph would break the width arithmetic
    # the same way an escape sequence does, and nothing here is worth that.
    param([switch]$Ascii)
    if ($Ascii) {
        return @{
            Cursor = '>'; On = '*'; Off = '-'; Bullet = '+'; Sparkle = '*'; Worktree = '@'
            Up = '^'; BarFull = '#'; BarEmpty = '.'; Prompt = '>'
            TL = '+'; TR = '+'; BL = '+'; BR = '+'; H = '-'; V = '|'
            LAngle = '<'; RAngle = '>'
            # Deliberately NOT V: a test counts three V glyphs on a line to prove the two-pane
            # layout is running, and a footer built from V made a single-column frame look paneled.
            HintSep = ':'
        }
    }
    return @{
        Cursor = [char]0x276F; On = [char]0x25CF; Off = [char]0x25CB; Bullet = '+'
        Sparkle = [char]0x273B; Worktree = [char]0x2302; Up = [char]0x2191
        BarFull = [char]0x2593; BarEmpty = [char]0x2591; Prompt = [char]0x276F
        TL = [char]0x256D; TR = [char]0x256E; BL = [char]0x2570; BR = [char]0x256F
        H = [char]0x2500; V = [char]0x2502
        LAngle = [char]0x2039; RAngle = [char]0x203A
        # A DASHED vertical, distinct from V by codepoint as well as by eye - see the ASCII note.
        HintSep = [char]0x250A
    }
}

function New-Bar {
    # Plain text of exactly $Width characters. Colour is someone else's job.
    param([int]$Percent, [int]$Width, [switch]$Ascii)
    $g = Get-Glyphs -Ascii:$Ascii
    if ($Width -le 0) { return '' }
    $p = [Math]::Min(100, [Math]::Max(0, $Percent))
    $filled = [int][Math]::Round($Width * $p / 100.0)
    return ([string]$g.BarFull * $filled) + ([string]$g.BarEmpty * ($Width - $filled))
}
