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
            Cursor = '>'; On = '*'; Off = '-'; Bullet = '*'; Sparkle = '*'; Worktree = '@'
            Up = '^'; BarFull = '#'; BarEmpty = '.'; Prompt = '>'
            TL = '+'; TR = '+'; BL = '+'; BR = '+'; H = '-'; V = '|'
            LAngle = '<'; RAngle = '>'
            # Deliberately NOT V: a test counts three V glyphs on a line to prove the two-pane
            # layout is running, and a footer built from V made a single-column frame look paneled.
            HintSep = ':'
        }
    }
    return @{
        Cursor = [char]0x276F; On = [char]0x25CF; Off = [char]0x25CB; Bullet = [char]0x23FA
        Sparkle = [char]0x273B; Worktree = [char]0x2302; Up = [char]0x2B06
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
