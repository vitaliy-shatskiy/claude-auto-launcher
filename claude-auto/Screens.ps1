# Pure frame builders. Every function here returns string[] and takes its geometry as parameters:
# nothing reads [Console], so all three screens are assertable from a session that has no terminal.

# Order: account, remote and action are chosen on almost every launch; model, effort and
# permission are now remembered between launches (see Prefs.ps1) and usually left alone. The
# three live choices sit at the top so the owner does not arrow past settled preferences to
# reach them. Reordered 2026-08-11 - was account, model, effort, permission, remote, action, mode.
#
# The model row's Values are internal keys, not `--model` aliases: 'default' means no flag at
# all, and the aliases for the 1M-context variants (`opus[1m]`, `sonnet[1m]`) are not valid
# PowerShell hashtable-friendly identifiers to type as bare values anyway. Labels holds the
# human-readable text shown on screen (missing for 'default', whose label is resolved at render
# time from settings.json - see Get-DefaultModelLabel in Env.ps1 and Get-RowOptionText below).
# Args holds the literal string passed to `--model` (missing for 'default', which passes no flag
# at all). opusplan and best were dropped - not offered - per the owner's 2026-08-10 decision.
$script:Rows = @(
    # Placeholder only: Set-LaunchRoster replaces the Values with the config's visible account keys
    # (short keys - this row is the width-critical one at 100 columns) and inserts the Remote row.
    @{ Name = 'Account';    Label = 'account';    Values = @('work') }
    @{ Name = 'Action';     Label = 'action';     Values = @('new', 'continue', 'resume', 'worktree') }
    @{ Name = 'Model';      Label = 'model';      Values = @('default', 'fable', 'opus1m', 'sonnet1m', 'haiku')
       Labels = @{ fable = 'Fable 5.1'; opus1m = 'Opus 5[1M]'; sonnet1m = 'Sonnet 5[1M]'; haiku = 'Haiku 4.5' }
       Args   = @{ fable = 'fable';   opus1m = 'opus[1m]';   sonnet1m = 'sonnet[1m]';   haiku = 'haiku' } }
    # 'ultracode' added 2026-09-04: 2.1.260 accepts it silently, where a bogus --effort value warns
    # on stderr. That warning is the only test the CLI offers, so the row is what it accepts and not
    # a guess - an option on this screen reads as a recommendation.
    @{ Name = 'Effort';     Label = 'effort';     Values = @('default', 'low', 'medium', 'high', 'xhigh', 'max', 'ultracode') }
    # advisor (2026-09-04, code.claude.com/docs/en/advisor.md). 'default' has no static label for the
    # same reason the model row's has none - what it resolves to lives in settings.json
    # (advisorModel) and is passed in already formatted (Get-DefaultAdvisorLabel in Env.ps1).
    # 'off' is NOT a flag: the CLI has none, and the launcher sets CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1
    # before exec instead. No launcher-side check of model x advisor - the CLI rejects a weaker
    # advisor itself, and a second opinion here would eventually disagree with it.
    @{ Name = 'Advisor';    Label = 'advisor';    Values = @('default', 'fable', 'opus', 'off') }
    # Verified against `claude --help` 2026-08-11: --permission-mode accepts acceptEdits, auto,
    # bypassPermissions, manual, dontAsk, plan. 'auto' was missing. 'manual' and 'dontAsk' are
    # deliberately not offered - nobody has established what they do on this setup, and a menu entry
    # reads as a recommendation.
    @{ Name = 'Permission'; Label = 'permission'; Values = @('default', 'plan', 'auto', 'acceptEdits', 'bypass') }
    @{ Name = 'Mode';       Label = 'mode';       Values = @('normal', 'safe') }
)

$script:RemoteRow = @{ Name = 'Remote'; Label = 'remote'; Values = @('on', 'off', 'on+QR', 'stop server') }
$script:AccountTints = @{ work = 'Green' }
$script:DefaultAccount = 'work'

function Set-LaunchRoster {
    # The account row and the tints come from the config; this file stays pure (no file reads) by
    # taking the roster as a parameter. The Remote row only exists when the feature is on.
    # -Default is the account a bare Enter launches (the launcher passes the canonical one): it wins
    # when visible, whatever its position in the roster; otherwise the first visible key does.
    param([Parameter(Mandatory)][object[]]$Accounts, [switch]$Remote, [string]$Default)
    $visible = @($Accounts | Where-Object { -not $_.Hidden } | ForEach-Object Key)
    if ($visible.Count -eq 0) { $visible = @($Accounts[0].Key) }
    $script:DefaultAccount = if ($Default -and $Default -in $visible) { $Default } else { $visible[0] }
    $rows = @($script:Rows | Where-Object { $_.Name -ne 'Remote' })
    ($rows | Where-Object { $_.Name -eq 'Account' }).Values = $visible
    if ($Remote) { $rows = @($rows[0], $script:RemoteRow) + @($rows[1..($rows.Count - 1)]) }
    $script:Rows = $rows
    $script:AccountTints = @{}
    foreach ($a in $Accounts) { $script:AccountTints[$a.Key] = "$($a.Tint)" }
}

# 50 columns since 2026-09-02: the owner launches over RDP from a phone, where 50x50 is what the
# screen gives. Every frame must survive it - footers wrap (New-HintFooter -Width), option rows
# collapse to the selected value, the maintenance verdicts fit 34 characters.
# Height 21, RE-MEASURED 2026-09-04 when the account row became a tab strip and the bars moved off
# it: at 50 columns the worst launch frame is 20 lines - 3 box + 1 blank + 8 rows + 3 bars (five
# hour, seven day, model bucket) + 1 blank + 1 separator + 1 restored + 2 wrapped footer lines -
# plus the headroom row Write-Frame needs. It was 18+1 before the advisor row and the third bar.
# Never guess this number: Test-Ui renders that exact frame at an unrefusable height, counts it and
# asserts this constant is the count plus one, so it re-measures itself on every run.
$script:MinWidth = 50
$script:MinHeight = 21
$script:TwoPaneWidth = 100

function Get-LaunchRows { return $script:Rows }

function Get-LaunchDefaultAccount {
    # The one place both the UI and the fallback (no-UI) prompt read the bare-Enter account from,
    # so a hidden canonical account yields to the first visible key in exactly one place instead of
    # being re-derived (and risking disagreement) at each call site.
    return $script:DefaultAccount
}

function New-LaunchState {
    # Defaults are today's launcher defaults on purpose: pressing Enter immediately must do what
    # pressing Enter twice does now, and must add no arguments at all.
    [pscustomobject]@{
        Account = $script:DefaultAccount; Model = 'default'; Effort = 'default'; Advisor = 'default'
        Permission = 'default'; Remote = 'on'; Action = 'new'; Mode = 'normal'; Row = 0
        # Per-tab state (Prefs.ps1). Profiles is this session's stash of the five habit rows per
        # account; Restored/RestoredAge are recomputed on every tab switch, which is why the frame
        # reads them from here rather than from a copy the launcher captured before the screen.
        Profiles = @{}; Restored = @(); RestoredAge = ''
    }
}

function Step-LaunchValue {
    # Values wrap in both directions; with two- and six-item rows, wrapping is fewer keystrokes.
    param($State, [int]$Delta)
    $row = $script:Rows[$State.Row]
    $values = $row.Values
    $i = [Array]::IndexOf($values, $State.($row.Name))
    if ($i -lt 0) { $i = 0 }
    $i = ($i + $Delta) % $values.Count
    if ($i -lt 0) { $i += $values.Count }
    $State.($row.Name) = $values[$i]
    return $State
}

function Get-RowOptionText {
    # The single place that turns a row's internal key into what the owner reads on screen. Most
    # rows have no Labels table at all, so the key IS the label (work, personal, high, ...). The
    # model row's 'default' key has no static label - what "default" means depends on
    # settings.json - so the caller supplies it already resolved.
    param($Row, [string]$Key, [string]$DefaultModelLabel = 'default', [string]$DefaultAdvisorLabel = 'default')
    if ($Row.Name -eq 'Model' -and $Key -eq 'default') { return $DefaultModelLabel }
    if ($Row.Name -eq 'Advisor' -and $Key -eq 'default') { return $DefaultAdvisorLabel }
    if ($Row.Labels -and $Row.Labels.ContainsKey($Key)) { return $Row.Labels[$Key] }
    return $Key
}

function Get-LaunchArgs {
    # The whole point of this function is its first line of behaviour: a default state yields
    # NOTHING. The nightly audit and the Rider plugin both depend on the launcher adding no flags
    # of its own, so every branch below is gated on a non-default value.
    param($State, [string]$ResumeId, [switch]$Fork)
    $a = @()
    # The model row stores an internal key (e.g. 'sonnet1m'); Args maps it to the literal
    # --model value ('sonnet[1m]'). 'default' has no Args entry, so the lookup is $null and no
    # flag is added - same shape as every other row's "-ne 'default'" gate, just via a table
    # instead of a literal comparison, because the key and the CLI value are no longer the same
    # string.
    $modelRow = $script:Rows | Where-Object { $_.Name -eq 'Model' } | Select-Object -First 1
    $modelArg = $modelRow.Args[$State.Model]
    if ($modelArg) { $a += @('--model', $modelArg) }
    if ($State.Effort -ne 'default') { $a += @('--effort', $State.Effort) }
    # Only the two model values produce a flag. 'default' means "say nothing" like every other row;
    # 'off' means the OPPOSITE of saying nothing - the CLI has no --advisor off, so the launcher
    # sets CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1 instead. Gated on the value list rather than on
    # `-ne 'default'`, or 'off' would reach the command line as a model name.
    if ($State.Advisor -in @('fable', 'opus')) { $a += @('--advisor', $State.Advisor) }
    if ($State.Permission -ne 'default') {
        # The UI says 'bypass' because the full name does not fit and reads like a sentence.
        $mode = if ($State.Permission -eq 'bypass') { 'bypassPermissions' } else { $State.Permission }
        $a += @('--permission-mode', $mode)
    }
    if ($State.Mode -eq 'safe') { $a += '--safe-mode' }
    switch ($State.Action) {
        'continue' { $a += '-c' }
        'worktree' { $a += '-w' }
        'resume'   { if ($ResumeId) { $a += @('--resume', $ResumeId); if ($Fork) { $a += '--fork-session' } } }
    }
    return $a
}

function Get-TooSmallFrame {
    # The size line comes first, not last: Limit-Line truncates from the right, so whatever line
    # carries the required dimensions must lead the explanatory prose, or a narrow terminal cuts the
    # numbers away before the reader ever sees them. The same ordering protects it from the height
    # budget below - on a very short terminal it is the one line guaranteed to survive.
    param([int]$Width, [int]$Height)
    $lines = @(
        "  need $($script:MinWidth)x$($script:MinHeight), have ${Width}x${Height}"
        '  terminal too small'
        ''
        '  resize the window, or press esc'
    )
    $lines = @($lines | ForEach-Object { Limit-Line -Text $_ -Max $Width })
    $budget = [Math]::Max(1, $Height)
    if ($lines.Count -gt $budget) { $lines = @($lines[0..($budget - 1)]) }
    return $lines
}

function Get-AccountTint {
    # ONE table for the two painters below - the option-cell pass and the tab-strip pass. Written
    # twice they drift, and a drift here means an account painted like another one, which is exactly
    # how a session gets spent against the wrong limit.
    #
    # The table is filled by Set-LaunchRoster from the config (key -> colour NAME in $script:C);
    # an unknown key or an unknown colour falls back to Green.
    param([string]$Account)
    $name = $script:AccountTints[$Account]
    if ($name -and $script:C.ContainsKey($name)) { return $script:C[$name] }
    return $script:C.Green
}

function Add-LaunchColor {
    # Painted AFTER layout. Brackets are matched in ONE pass with '[' and ']' excluded from the
    # group: an escape sequence itself contains '[', so a looser pattern swallows half an escape
    # and leaves '2m' behind once the colour is stripped. That guard alone is not enough once a
    # label can carry its own literal bracket, though ('Opus 5[1M]', and the resolved default
    # label 'default (Fable 5.1[1M])'): a bare '\[([^\[\]]+)\]' matches whichever bracket pair it
    # meets first, which on the selected cell "[Opus 5[1M]]" is the INNER '[1M]' only - and the
    # same pattern goes on to paint an UNselected cell's own literal '[1M]' text (e.g. 'Sonnet
    # 5[1M]' shown but not chosen) as if it were selected too, because nothing in that pattern
    # cares whether a '[' is preceded by the selection marker.
    #
    # Fix: anchor on "On-glyph + space" via a zero-width lookbehind, so only the '[' that actually
    # opens the selected cell's own bracket can start a match, and allow exactly one nested
    # bracket pair inside via "(?:[^\[\]]|\[[^\[\]]*\])*" before the matching outer close - enough
    # for every label this row renders (one nesting level, never two). The non-bracket alternative
    # keeps excluding '[' and ']', so a stray '[' belonging to another pass's escape code still
    # cannot be swallowed - this fix narrows WHERE a bracket pair may start, it does not loosen
    # what counts as one.
    param([string]$Line, [switch]$Enabled, [hashtable]$Glyphs)
    if (-not $Enabled -or -not $Line) { return $Line }
    $c = $script:C
    $out = $Line
    # Measured on the ORIGINAL line, before any pass below weaves escapes into it: every ANSI code
    # is ESC + '[', so a prefix test run later would be reading colour codes instead of text.
    $isAccountRow = $Line -match '^.{3}account[\*\s]'

    # Must run first: once later substitutions below weave BrightYellow/Accent/Dim escapes into
    # $out, those escapes contain their own literal '[' (every ANSI code is ESC + '['), and a
    # bracket-matching pass running after that point would be matching against colour codes
    # instead of label text - the exact way the '2m' leak in the comment above happens.
    $onGlyph = [regex]::Escape("$($Glyphs.On)")
    $out = [regex]::Replace($out, "(?<=$onGlyph )\[((?:[^\[\]]|\[[^\[\]]*\])*)\]", {
        param($m)
        $value = $m.Groups[1].Value
        $tint =
            if ($value -in @('off', 'stop server', 'safe')) { $script:C.Yellow }
            else { Get-AccountTint -Account $value }
        $script:C.Bold + $tint + $m.Value + $script:C.Reset
    })

    # The account row is a tab strip: no On glyph, so the pass above cannot see it. Anchored on the
    # row LABEL instead - three prefix characters then 'account' - so the option-cell pattern above
    # stays exactly as it was. Only the active tab is bracketed, so the first (and only) bracket
    # pair on this line is the one to paint; the tint keys off the account NAME, not the whole cell,
    # because the cell also carries that account's percentage ('[work 40%]').
    if ($isAccountRow) {
        $out = [regex]::Replace($out, '\[([^\[\]]+)\]', {
            param($m)
            $value = ($m.Groups[1].Value -split ' ')[0]
            $tint = Get-AccountTint -Account $value
            $script:C.Bold + $tint + $m.Value + $script:C.Reset
        })
    }

    $out = [regex]::Replace($out, '(\d+)%', {
        param($m)
        (Get-PercentColor -Percent ([int]$m.Groups[1].Value)) + $m.Value + $script:C.Reset
    })
    $out = $out -replace '\b(account|model|effort|advisor|permission|remote|action|mode)\b', ($c.Dim + '$1' + $c.Reset)
    $out = $out -replace "([$($Glyphs.Cursor)])", ($c.BrightYellow + '$1' + $c.Reset)
    $out = $out -replace "([$($Glyphs.Sparkle)])", ($c.Accent + '$1' + $c.Reset)
    $out = $out -replace '(\d+ (?:min|h|d) ago|just now)', ($c.Dim + '$1' + $c.Reset)
    return $out
}

function New-HintFooter {
    # The footer as DATA rather than a hand-written string. One list drives three things that must
    # never disagree: what is printed, where it can be clicked, and which key the click stands in
    # for. Written as three separate copies, a footer eventually advertises a key that does nothing.
    #
    # Returns the PLAIN text plus spans. Layout and truncation run on plain text and colour is
    # applied afterwards (Theme.ps1's rule), so the spans stay valid: an escape sequence adds
    # invisible characters and would move every column if it were baked in here.
    #
    # Each hint: Token (the key as printed), Label (what it does), Key (a ConsoleKey name) or Char.
    # Clickable hints get a span covering the WHOLE token+label group - clicking the word "start" is
    # the same intent as clicking "enter", and demanding the exact key word would be a puzzle.
    #
    # -Width wraps: hints are packed greedily onto as many lines as they need, never split, each
    # line indented like the first. Before this (2026-09-02) the footer was one line cut by
    # Limit-Line, so at 60 columns the launch screen lost 'u maintenance' and 'esc quit' - the two
    # hints that are not arrows - and their click spans with them. Returns Lines (Text + Spans per
    # line, every span carrying its Line index) and, for the callers that never wrap, Text/Spans of
    # the FIRST line. Without -Width everything lands on one line, as before.
    param([Parameter(Mandatory)][array]$Hints, [Parameter(Mandatory)][hashtable]$Glyphs, [int]$Width = 0)
    $sep = "  $($Glyphs.HintSep)  "
    $lines = @()
    $text = '  '
    $spans = @()
    foreach ($h in $Hints) {
        $piece = $h.Token
        if ($h.Label) { $piece += ' ' + $h.Label }
        $hasContent = $text.Length -gt 2
        if ($Width -gt 0 -and $hasContent -and ($text.Length + $sep.Length + $piece.Length) -gt $Width) {
            $lines += [pscustomobject]@{ Text = $text; Spans = @($spans) }
            $text = '  '
            $spans = @()
            $hasContent = $false
        }
        if ($hasContent) { $text += $sep }
        $start = $text.Length
        $tokenEnd = $start + $h.Token.Length - 1
        $text += $piece
        if ($h.Clickable) {
            $spans += [pscustomobject]@{
                Start = $start; End = $text.Length - 1
                KeyStart = $start; KeyEnd = $tokenEnd
                Key = $h.Key; Char = $h.Char; Line = $lines.Count
            }
        } else {
            # Not clickable, but still worth painting: the arrow hints are how the keyboard is
            # discovered, and dimming them uniformly is what makes the actions stand out.
            $spans += [pscustomobject]@{
                Start = -1; End = -1
                KeyStart = $start; KeyEnd = $tokenEnd
                Key = ''; Char = ''; Line = $lines.Count
            }
        }
    }
    $lines += [pscustomobject]@{ Text = $text; Spans = @($spans) }
    return [pscustomobject]@{ Text = $lines[0].Text; Spans = @($lines[0].Spans); Lines = @($lines) }
}

function Add-HintColor {
    # Paints the key tokens by COLUMN INDEX, using spans measured on the plain text. Painting by
    # index rather than by pattern is what keeps this reversible - stripping the colour returns the
    # original line exactly, which the suite asserts - and it cannot mis-fire on a label that
    # happens to contain the same word as a key.
    param([string]$Line, [array]$Spans, [switch]$Enabled)
    if (-not $Enabled -or -not $Line -or -not $Spans) { return $Line }
    $c = $script:C
    $out = ''
    $cursor = 0
    foreach ($s in ($Spans | Sort-Object KeyStart)) {
        if ($s.KeyStart -lt $cursor -or $s.KeyEnd -ge $Line.Length) { continue }
        $out += $c.Dim + $Line.Substring($cursor, $s.KeyStart - $cursor) + $c.Reset
        $tint = if ($s.Start -ge 0) { $c.BrightCyan } else { $c.Dim }
        $out += $tint + $Line.Substring($s.KeyStart, $s.KeyEnd - $s.KeyStart + 1) + $c.Reset
        $cursor = $s.KeyEnd + 1
    }
    if ($cursor -lt $Line.Length) { $out += $c.Dim + $Line.Substring($cursor) + $c.Reset }
    return $out
}

function Complete-PickerFrame {
    # Appends the footer, records its clickable spans, and paints - the last three steps of every
    # screen. Shared because the picker returns from three different branches and three copies of
    # this would drift; the empty-list branch in particular is the one that gets forgotten when a
    # footer changes. The launch screen goes through here too (-Body launch) since the footer
    # learned to wrap: the span bookkeeping for several footer lines is one thing, not two.
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][array]$Lines,
        [Parameter(Mandatory)]$Footer,
        [int]$Width, [hashtable]$Glyphs, [switch]$Color, $RowMap,
        [ValidateSet('picker', 'launch')][string]$Body = 'picker'
    )
    $footerLines = @($Footer.Lines)
    if ($footerLines.Count -eq 0) { $footerLines = @([pscustomobject]@{ Text = $Footer.Text; Spans = @($Footer.Spans) }) }
    $all = @($Lines)
    $footerIndex = $all.Count
    $visible = @()
    for ($f = 0; $f -lt $footerLines.Count; $f++) {
        $footerText = Limit-Line -Text $footerLines[$f].Text -Max $Width
        $all += $footerText
        # Spans past the truncation are dropped rather than clamped: half a hint is not a hint, and
        # a click landing on a word nobody can read would look like the menu acting at random.
        $visible += @($footerLines[$f].Spans | Where-Object { $_.Start -ge 0 -and $_.End -lt $footerText.Length })
    }
    if ($RowMap -and $RowMap.Value) {
        # FooterY is the FIRST footer line; each span's Line says how far below it sits.
        $RowMap.Value | Add-Member -NotePropertyName FooterY -NotePropertyValue $footerIndex -Force
        $RowMap.Value | Add-Member -NotePropertyName Footer -NotePropertyValue $visible -Force
        $RowMap.Value | Add-Member -NotePropertyName FooterLines -NotePropertyValue $footerLines.Count -Force
    }
    $painted = @()
    for ($i = 0; $i -lt $all.Count; $i++) {
        $l = Limit-Line -Text $all[$i] -Max $Width
        # The footer is painted by column span, not by pattern: its words ('row', 'value', 'start')
        # are ordinary English and a pattern-based rule would tint them wherever else they appear.
        if ($i -ge $footerIndex) { $painted += Add-HintColor -Line $l -Spans $footerLines[$i - $footerIndex].Spans -Enabled:$Color }
        elseif ($Body -eq 'launch') { $painted += Add-LaunchColor -Line $l -Enabled:$Color -Glyphs $Glyphs }
        else { $painted += Add-PickerColor -Line $l -Enabled:$Color -Glyphs $Glyphs }
    }
    return @($painted)
}

function Get-LaunchFrame {
    param(
        [Parameter(Mandatory)]$State,
        [int]$Width = 78,
        [int]$Height = 24,
        [hashtable]$Limits = @{},
        [hashtable]$Version = @{},
        [string[]]$Restored = @(),
        [string]$RestoredAge = '',
        # What "default" resolves to right now, already formatted (e.g. 'default
        # (Fable 5.1[1M])'). Computed by the caller (Get-DefaultModelLabel in Env.ps1) because
        # this function must stay pure - no file reads - to keep it directly assertable.
        [string]$DefaultModelLabel = 'default',
        # Same contract for the advisor row's 'default' - resolved by Get-DefaultAdvisorLabel in
        # Env.ps1, passed in already formatted, for the same purity reason.
        [string]$DefaultAdvisorLabel = 'default',
        [switch]$Color,
        [switch]$Ascii,
        # Where each row and each option cell landed, for hit-testing a click. Filled by the code
        # that draws them: the rows are NOT one per line (the limit bars insert their own), so any
        # independent guess at the mapping would drift the moment a bar appears or disappears.
        [ref]$RowMap
    )
    # The restore marks live on the STATE since they became per tab: switching tabs recomputes both
    # (Switch-LaunchAccount), so a caller that captured them before the screen opened would freeze
    # the first tab's marks on every tab. The parameters stay for the callers that pass their own -
    # explicit wins - but the default has to come from the state, not from an empty array.
    # $PSBoundParameters rather than a "is it empty" test: "the caller said nothing" and "the caller
    # said nothing was restored" are different answers, and only this can tell them apart.
    if (-not $PSBoundParameters.ContainsKey('Restored') -and $State.Restored) { $Restored = @($State.Restored) }
    if (-not $PSBoundParameters.ContainsKey('RestoredAge') -and $State.RestoredAge) { $RestoredAge = "$($State.RestoredAge)" }
    if ($RowMap) { $RowMap.Value = [pscustomobject]@{ Rows = @() } }
    if ($Width -lt $script:MinWidth -or $Height -lt $script:MinHeight) {
        return (Get-TooSmallFrame -Width $Width -Height $Height)
    }
    $g = Get-Glyphs -Ascii:$Ascii
    $boxWidth = [Math]::Min($Width, 100)
    $inner = $boxWidth - 4
    $wide = $Width -ge $script:TwoPaneWidth

    # Header: brand on the left, versions on the right, with the update marker only when they differ.
    $left = " $($g.Sparkle) claude-auto"
    $right = ''
    if ($Version.Installed) {
        $right = "v$($Version.Installed)"
        if ($Version.Newest -and $Version.Newest -ne $Version.Installed) {
            $right += "  $($g.Up) $($Version.Newest)"
        }
    }
    $pad = [Math]::Max(1, $inner - $left.Length - $right.Length)
    $header = @($left + (' ' * $pad) + $right)

    $body = @()
    $rowHits = @()
    $limit = $Limits[$State.Account]
    for ($i = 0; $i -lt $script:Rows.Count; $i++) {
        $row = $script:Rows[$i]
        $current = $State.($row.Name)
        $prefix = if ($State.Row -eq $i) { " $($g.Cursor) " } else { '   ' }
        # A restored value the owner cannot see is one they cannot be surprised by only until it
        # costs them something. Mark it, and say how old it is.
        $mark = if ($row.Name -in $Restored) { '*' } else { ' ' }
        $labelPart = ($row.Label + $mark).PadRight(12)

        if ($row.Name -eq 'Account') {
            # A TAB STRIP since 2026-09-04, not an option row: '[work 40%] : personal 17% : low 43%'.
            # Switching accounts is the habit the owner exercises most, and the percentage that
            # decides WHICH account to switch to has to be on the row before the switch, not after
            # it. The separator is HintSep rather than V for the reason Theme.ps1 records - a line of
            # V glyphs reads as a pane divider - and it degrades to ':' on an ASCII console.
            $joiner = " $($g.HintSep) "
            # Percentages first; dropped as a group if the line with them does not fit. Never per
            # tab: three tabs where only some carry a number reads as "these two have no limit".
            foreach ($withPercent in @($true, $false)) {
                $cells = @(foreach ($v in $row.Values) {
                    $l = $Limits[$v]
                    $text = if ($withPercent -and $l -and $null -ne $l.FiveHour) { "$v $($l.FiveHour)%" } else { "$v" }
                    if ($v -eq $current) { "[$text]" } else { $text }
                })
                if (($prefix + $labelPart + ($cells -join $joiner)).Length -le $inner) { break }
            }
        } else {
            $joiner = ' '
            $cells = @(foreach ($v in $row.Values) {
                $text = Get-RowOptionText -Row $row -Key $v -DefaultModelLabel $DefaultModelLabel -DefaultAdvisorLabel $DefaultAdvisorLabel
                if ($v -eq $current) { "$($g.On) [$text]" } else { "$($g.Off) $text" }
            })
        }

        # Column spans for each option, measured off the same strings that are about to be joined.
        # A click inside one of these means "this value", which is what makes the screen a menu
        # rather than a picture of one.
        $cellHits = @()
        $cursorX = $prefix.Length + $labelPart.Length
        for ($c = 0; $c -lt @($cells).Count; $c++) {
            $len = @($cells)[$c].Length
            $cellHits += [pscustomobject]@{ Start = $cursorX; End = $cursorX + $len - 1; Value = $row.Values[$c] }
            $cursorX += $len + $joiner.Length
        }
        $line = $prefix + $labelPart + ($cells -join $joiner)
        # The model row's human-readable labels ('Sonnet 5[1M]', a resolved 'default (...)') can
        # outgrow the box before Limit-Line ever truncates it - and a mid-word cut there is exactly
        # what this exists to avoid. Collapse to the selected value alone, marked with ‹ › to say
        # more options exist off-screen (the footer already explains left/right cycles them).
        if ($line.Length -gt $inner) {
            $selText = Get-RowOptionText -Row $row -Key $current -DefaultModelLabel $DefaultModelLabel -DefaultAdvisorLabel $DefaultAdvisorLabel
            $line = $prefix + $labelPart + "$($g.LAngle) $selText $($g.RAngle)"
            # Collapsed: the other options are not on screen, so there is nothing to click. The ROW
            # is still clickable; offering cell hits here would select values the owner cannot see.
            $cellHits = @()
        }
        $rowHits += [pscustomobject]@{ Index = $i; Name = $row.Name; BodyY = $body.Count; Cells = $cellHits }

        $body += $line
        # The bars belong to the ACTIVE tab and now sit under the strip, never on it: the row itself
        # carries every account's five-hour number, and a bar for one account beside the names of
        # three read as if it described all of them. The age travels with the last bar - these are
        # last-known numbers from whichever writer saw them last (Env.ps1), and a three-day-old
        # percentage that reads as current is the failure this line exists to prevent.
        if ($row.Name -eq 'Account' -and $limit) {
            # Label and bar kept apart: the one-line form joins them with a single space, the
            # stacked form pads every label to the widest so the three bar runs share a column.
            $bars = @()
            if ($null -ne $limit.FiveHour) { $bars += @{ Label = '5h'; Bar = "$(New-Bar -Percent $limit.FiveHour -Width 8 -Ascii:$Ascii) $('{0,3}' -f $limit.FiveHour)%" } }
            if ($null -ne $limit.SevenDay) { $bars += @{ Label = '7d'; Bar = "$(New-Bar -Percent $limit.SevenDay -Width 8 -Ascii:$Ascii) $('{0,3}' -f $limit.SevenDay)%" } }
            # The third bar exists only when the record carries a model bucket - statusline.js writes
            # none, so a record of its origin has nothing to show here. Drawing a 0% bar instead
            # would claim a measurement nobody took.
            if ($null -ne $limit.Model) {
                $modelLabel = if ($limit.ModelLabel) { "$($limit.ModelLabel)".ToLower() } else { 'model' }
                $bars += @{ Label = $modelLabel; Bar = "$(New-Bar -Percent $limit.Model -Width 8 -Ascii:$Ascii) $('{0,3}' -f $limit.Model)%" }
            }
            if ($bars.Count -gt 0) {
                $oneLine = '     ' + (@($bars | ForEach-Object { "$($_.Label) $($_.Bar)" }) -join '  ') + '   ' + $limit.AgeText
                # One line when it fits, one bar per line when it does not - measured, not assumed
                # from the width breakpoint alone, because the model label's length is the record's
                # and not ours.
                if ($wide -and $oneLine.Length -le $inner) {
                    $body += $oneLine
                } else {
                    $labelWidth = (@($bars | ForEach-Object { $_.Label.Length }) | Measure-Object -Maximum).Maximum
                    for ($b = 0; $b -lt $bars.Count; $b++) {
                        $tail = if ($b -eq $bars.Count - 1) { '   ' + $limit.AgeText } else { '' }
                        $body += '     ' + $bars[$b].Label.PadRight($labelWidth) + ' ' + $bars[$b].Bar + $tail
                    }
                }
            }
        }
    }

    $lines = @()
    $boxLines = @(New-Box -Lines $header -Width $boxWidth -Ascii:$Ascii)
    $lines += $boxLines
    $lines += ''
    # Frame coordinates at last: the header box plus the blank line sit above the body, and the row
    # positions were recorded relative to the body while it was being built.
    if ($RowMap) {
        $offset = $boxLines.Count + 1
        $RowMap.Value = [pscustomobject]@{
            Rows = @($rowHits | ForEach-Object {
                [pscustomobject]@{ Index = $_.Index; Name = $_.Name; Y = $offset + $_.BodyY; Cells = $_.Cells }
            })
        }
    }
    $lines += $body
    $lines += ''
    # [string] before the multiply: a [char] times an int is not string repetition in PowerShell.
    $lines += '  ' + ([string]$g.H * ($boxWidth - 4))
    if ($Restored.Count -gt 0) {
        # 'just now' already reads as a time; appending 'ago' to it produced "just now ago". Short
        # enough for 50 columns with the longest age ('12 min ago'), and it names ctrl+r: the line
        # said 'r resets everything' for ten days after a bare r stopped doing anything (2026-08-23).
        $when = if ($RestoredAge -eq 'just now') { 'just now' } else { "$RestoredAge ago" }
        $lines += "  * restored ($when), ctrl+r resets"
    }
    # Arrows are deliberately NOT clickable: "up/down" names two directions, and a click on it
    # cannot mean one of them. Everything that IS a single action is.
    $footer = New-HintFooter -Glyphs $g -Width $Width -Hints @(
        @{ Token = 'up/down';    Label = 'row';         Clickable = $false }
        @{ Token = 'left/right'; Label = 'value';       Clickable = $false }
        @{ Token = 'enter';      Label = 'start';       Clickable = $true; Key = 'Enter';  Char = '' }
        @{ Token = 'u';          Label = 'maintenance'; Clickable = $true; Key = '';       Char = 'u' }
        @{ Token = 'esc';        Label = 'quit';        Clickable = $true; Key = 'Escape'; Char = '' }
    )
    return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $Width -Glyphs $g -Color:$Color -RowMap $RowMap -Body launch)
}

function Get-SessionExchange {
    # Falls back to LastUser/LastAssistant (in that order) for a session object that carries no
    # RecentMessages - the shape every hand-built fixture in the test suite still uses, and the
    # shape a stale on-disk cache entry (written before this field existed) still has. Order and
    # placeholder text match what the detail pane showed before this change, so existing callers
    # see identical content, just through the shared multi-message renderer now.
    param($Session)
    if ($Session.RecentMessages -and @($Session.RecentMessages).Count -gt 0) { return @($Session.RecentMessages) }
    $out = @()
    if ($Session.LastUser) { $out += [pscustomobject]@{ Speaker = 'user'; Text = $Session.LastUser } }
    elseif ($Session.Title) { $out += [pscustomobject]@{ Speaker = 'user'; Text = $Session.Title } }
    else { $out += [pscustomobject]@{ Speaker = 'user'; Text = '(nothing was asked)' } }
    if ($Session.LastAssistant) { $out += [pscustomobject]@{ Speaker = 'assistant'; Text = $Session.LastAssistant } }
    else { $out += [pscustomobject]@{ Speaker = 'assistant'; Text = '(no reply)' } }
    return $out
}

function Get-ExchangeLines {
    # Renders as many of the most recent messages as fit in $MaxLines, oldest at the top, newest
    # at the bottom - the shared body for both picker layouts. Works backwards from the newest
    # message so the pane always shows what just happened, not whatever happened to fit from the
    # start of the (already-capped) recent-messages list.
    #
    # Attribution is a literal word plus a glyph ('you ›', 'claude ›'), never colour or a glyph
    # alone: colour can be off (NO_COLOR, a dumb terminal) and the glyph set can be ASCII, and
    # either one on its own must still say unambiguously who spoke.
    #
    # Per-message cap (2026-08-11 owner fix): without one, a single long message ate the whole
    # remaining budget and the pane read as a wall of text instead of a back-and-forth. The cap is
    # recomputed before EVERY message from what is left of the budget and how many messages are
    # still left to consider - not fixed once for the whole pane - so with few messages left it
    # grows to use the space rather than leaving it empty, and with many left it shrinks toward
    # $MinLinesPerMessage so at least a handful of exchanges stay visible instead of the newest one
    # truncating every older one to nothing.
    param([array]$Messages, [int]$Width, [int]$MaxLines, [hashtable]$Glyphs, [int]$MinLinesPerMessage = 5)
    if ($MaxLines -le 0 -or $Width -le 0 -or -not $Messages -or @($Messages).Count -eq 0) { return @() }
    $Messages = @($Messages)
    $blocks = @()
    $used = 0
    for ($i = $Messages.Count - 1; $i -ge 0; $i--) {
        if ($used -ge $MaxLines) { break }
        $remainingBudget = $MaxLines - $used
        # i+1 is how many messages (this one included) are still waiting to be considered; capping
        # the divisor at 3 is what guarantees room survives for at least three exchanges on a pane
        # tall enough to hold them, per the owner's "3 messages in a 30-row pane" target.
        $remainingTarget = [Math]::Min($i + 1, 3)
        $cap = [Math]::Max($MinLinesPerMessage, [int][Math]::Floor($remainingBudget / $remainingTarget))
        $budget = [Math]::Min($cap, $remainingBudget)

        $m = $Messages[$i]
        $prefix = if ($m.Speaker -eq 'user') { "you $($Glyphs.RAngle) " } else { "claude $($Glyphs.RAngle) " }
        $indent = ' ' * $prefix.Length
        $wrapWidth = [Math]::Max(1, $Width - $prefix.Length)
        $wrapped = @(Split-TextLines -Text $m.Text -Width $wrapWidth -MaxLines $budget)
        if ($wrapped.Count -eq 0) { continue }
        $lines = for ($j = 0; $j -lt $wrapped.Count; $j++) {
            if ($j -eq 0) { $prefix + $wrapped[$j] } else { $indent + $wrapped[$j] }
        }
        $blocks = , @{ Lines = $lines } + $blocks
        $used += $lines.Count
    }
    $out = @()
    foreach ($b in $blocks) { $out += $b.Lines }
    return $out
}

function Select-ResumableSessions {
    # A session with zero human messages (the owner launched it and typed nothing) has nothing to
    # resume into - filtered from the picker here, not in Get-ClaudeSessions, so the data function
    # stays an honest reader of what is actually on disk and any other caller still sees everything.
    # Shared by Invoke-SessionPicker (navigation) and Get-PickerFrame (rendering) so the two can never
    # disagree about which index points at which session.
    param([Parameter(Mandatory)][AllowEmptyCollection()][array]$Sessions)
    return @($Sessions | Where-Object { $_.PromptCount -gt 0 })
}

function Select-SessionMatch {
    param([Parameter(Mandatory)][AllowEmptyCollection()][array]$Sessions, [string]$Filter)
    if ([string]::IsNullOrWhiteSpace($Filter)) { return $Sessions }
    $f = $Filter.Trim()
    return @($Sessions | Where-Object {
        "$($_.Project) $($_.Worktree) $($_.Title) $($_.LastUser) $($_.LastAssistant)" -like "*$f*"
    })
}

function Add-PickerColor {
    param([string]$Line, [switch]$Enabled, [hashtable]$Glyphs)
    if (-not $Enabled -or -not $Line) { return $Line }
    $c = $script:C
    if ($Line -match 'nothing matches') { return $c.Dim + $Line + $c.Reset }
    $out = $Line

    # Speaker attribution gets a colour so a wall of preview text reads as a back-and-forth at a
    # glance. The literal words stay exactly where they were: colour is an ADDITION to 'you >' /
    # 'claude >', never a replacement, because NO_COLOR and dumb terminals are real and the
    # attribution has to survive both (the rule Get-ExchangeLines is built on).
    #
    # Parked behind a sentinel rather than coloured in place: in ASCII mode Cursor and RAngle are
    # BOTH '>', so the cursor rule below would repaint the '>' belonging to 'you >' - and would do
    # it in the middle of the escape sequence this rule had just inserted. Substituting first and
    # restoring last keeps the two rules from ever seeing each other's output.
    $rA = [regex]::Escape($Glyphs.RAngle)
    $markUser = [string][char]1 + 'u' + [string][char]1
    $markClaude = [string][char]1 + 'a' + [string][char]1
    $out = $out -replace "(^|\s)you $rA ", ('$1' + $markUser)
    $out = $out -replace "(^|\s)claude $rA ", ('$1' + $markClaude)

    $out = $out -replace "([$($Glyphs.Cursor)])", ($c.BrightYellow + '$1' + $c.Reset)
    $out = $out -replace "([$($Glyphs.Bullet)])", ($c.Magenta + '$1' + $c.Reset)
    $out = $out -replace "([$($Glyphs.Worktree)])", ($c.Yellow + '$1' + $c.Reset)
    $out = $out -replace '(\d+ (?:min|h|d)|now)$', ($c.Dim + '$1' + $c.Reset)
    $out = $out -replace '(\d+ msgs)', ($c.Dim + '$1' + $c.Reset)

    # Restore the parked attributions. BrightCyan for the owner, the warm Accent for Claude - the
    # same accent the launcher uses elsewhere for Claude's own colour, so the pane reads as one
    # palette rather than two arbitrary hues.
    $out = $out.Replace($markUser, $c.BrightCyan + 'you ' + $Glyphs.RAngle + ' ' + $c.Reset)
    $out = $out.Replace($markClaude, $c.Accent + 'claude ' + $Glyphs.RAngle + ' ' + $c.Reset)
    return $out
}

function Get-PickerFrame {
    # Master-detail above 100 columns, single column below it. The list identifies sessions; the
    # detail pane is where the last exchange is actually readable, wrapped rather than cut.
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][array]$Sessions,
        [int]$Index = 0,
        [string]$Filter = '',
        [int]$Width = 78,
        [int]$Height = 24,
        [datetime]$Now = (Get-Date),
        [switch]$Color,
        [switch]$Ascii,
        # Where the session rows landed, for hit-testing a mouse click. Filled by the SAME code that
        # renders them - the alternative is a second copy of the viewport arithmetic, and the two
        # would eventually disagree about which index sits on which line, which is precisely the
        # bug this file's other comments already warn about.
        [ref]$RowMap
    )
    if ($RowMap) { $RowMap.Value = [pscustomobject]@{ FirstRowY = 0; RowCount = 0; Start = 0 } }
    if ($Width -lt $script:MinWidth -or $Height -lt $script:MinHeight) {
        return (Get-TooSmallFrame -Width $Width -Height $Height)
    }
    $g = Get-Glyphs -Ascii:$Ascii
    # A session nobody typed anything into is not offered - see Select-ResumableSessions. The count
    # is stated rather than the list just quietly getting shorter: dropping rows silently reads as
    # "these are all your sessions" when 16 of 40 were never a real conversation.
    $resumable = @(Select-ResumableSessions -Sessions $Sessions)
    $hiddenCount = @($Sessions).Count - $resumable.Count
    $items = @(Select-SessionMatch -Sessions $resumable -Filter $Filter)

    $title = "resume $($g.H) $($items.Count) sessions"
    if ($hiddenCount -gt 0) { $title += " $($g.H) $hiddenCount empty hidden" }
    if ($Filter) { $title += " $($g.H) filter: $Filter" }
    # Same rule as the launch screen: the arrow hint names two directions and cannot be clicked
    # into one of them; every single action can.
    $footer = New-HintFooter -Glyphs $g -Width $Width -Hints @(
        @{ Token = 'up/down'; Label = 'move';   Clickable = $false }
        @{ Token = '/';       Label = 'filter'; Clickable = $true; Key = ''; Char = '/' }
        @{ Token = 'enter';   Label = 'open';   Clickable = $true; Key = 'Enter'; Char = '' }
        @{ Token = 'f';       Label = 'fork';   Clickable = $true; Key = ''; Char = 'f' }
        @{ Token = 'esc';     Label = 'back';   Clickable = $true; Key = 'Escape'; Char = '' }
    )

    if ($items.Count -eq 0) {
        $emptyMsg =
            if ($Filter) { '  nothing matches that filter' }
            elseif ($hiddenCount -gt 0) { "  all $hiddenCount sessions here are empty - nothing to resume" }
            else { '  no sessions found' }
        $lines = New-Box -Lines @('', $emptyMsg, '') -Width $Width -Title $title -Ascii:$Ascii
        return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $Width -Glyphs $g -Color:$Color -RowMap $RowMap)
    }

    # Box top + box bottom + one headroom row + the footer's lines (one on a wide terminal, more
    # when it wraps). Everything below budgets against this, so no branch can produce a frame
    # taller than the terminal - which scrolls the top away and desynchronises every later redraw.
    # The headroom row is the one Write-Frame's trailing newline needs (see Get-MaintenanceFrame);
    # until 2026-09-02 this budget lacked it and a full list filled the terminal exactly.
    $bodyRows = [Math]::Max(3, $Height - 3 - @($footer.Lines).Count)
    $wide = $Width -ge $script:TwoPaneWidth

    $leftWidth = if ($wide) { [Math]::Max(28, [int](($Width - 2) * 0.4)) } else { $Width - 2 }
    $rightWidth = ($Width - 2) - $leftWidth - 1

    if (-not $wide) {
        # 6 rows reserved for the preview, BEFORE the viewport is sized: one header (project,
        # worktree, message count, date - the columns the narrow list has no room for), one
        # separator, two for the question, two for the answer.
        $previewRows = 6
        $listRows = [Math]::Max(1, $bodyRows - $previewRows)
        $vp = Get-Viewport -Count $items.Count -Index $Index -Visible $listRows

        $list = @()
        for ($i = $vp.Start; $i -lt ($vp.Start + $vp.Visible); $i++) {
            $s = $items[$i]
            $mark = if ($i -eq $Index) { " $($g.Cursor) " } else { '   ' }
            $where = $s.Project
            if ($s.Worktree) { $where = "$($g.Worktree) $($s.Worktree)" }
            $age = Format-RelativeAge -From $s.Modified -Now $Now
            $what = if ($s.LastUser) { $s.LastUser } elseif ($s.Title) { $s.Title } else { '' }
            # The project alone does not identify a session: on this machine twelve consecutive rows
            # are all the same repository. The snippet is what makes the list scannable.
            $room = $leftWidth - $mark.Length - $age.Length - 2
            $label = $where
            if ($what -and $room -gt ($where.Length + 4)) {
                $label = $where + '  ' + (Limit-Line -Text $what -Max ($room - $where.Length - 2))
            }
            $pad = [Math]::Max(1, $leftWidth - $mark.Length - $label.Length - $age.Length - 1)
            $list += $mark + (Limit-Line -Text $label -Max ($leftWidth - $mark.Length - $age.Length - 2)) +
                     (' ' * $pad) + $age + ' '
        }

        # Single column: the selected session's preview goes underneath the list.
        $s = $items[$Index]
        $head = "$($s.Project)"
        if ($s.Worktree) { $head += "  $($g.Worktree) $($s.Worktree)" }
        $head += "  $($g.H)  $($s.PromptCount) msgs  $($g.H)  $($s.Modified.ToString('dd MMM HH:mm'))"
        $body = @($list)
        $body += '  ' + (Limit-Line -Text $head -Max ($Width - 4))
        $body += '  ' + ([string]$g.H * ($Width - 6))
        # $previewRows (6, fixed above) minus the two header lines just added is what is left for
        # the exchange itself - same total budget as before this change, now spent on as many
        # attributed messages as fit rather than a hardcoded one question, one answer.
        $exchangeBudget = [Math]::Max(1, $previewRows - 2)
        foreach ($l in (Get-ExchangeLines -Messages (Get-SessionExchange -Session $s) -Width ($Width - 4) -MaxLines $exchangeBudget -Glyphs $g)) {
            $body += '  ' + $l
        }
        $lines = New-Box -Lines $body -Width $Width -Title $title -Ascii:$Ascii
        # The list is the first $vp.Visible entries of $body, and New-Box wraps $body between a top
        # and a bottom border. Deriving the offset from the counts rather than hardcoding 1 means a
        # future change to the box cannot silently move every row by one.
        if ($RowMap) {
            $RowMap.Value = [pscustomobject]@{
                FirstRowY = $lines.Count - $body.Count - 1
                RowCount  = $vp.Visible
                Start     = $vp.Start
            }
        }
        return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $Width -Glyphs $g -Color:$Color -RowMap $RowMap)
    }

    $vp = Get-Viewport -Count $items.Count -Index $Index -Visible $bodyRows
    $list = @()
    for ($i = $vp.Start; $i -lt ($vp.Start + $vp.Visible); $i++) {
        $s = $items[$i]
        $mark = if ($i -eq $Index) { " $($g.Cursor) " } else { '   ' }
        $where = $s.Project
        if ($s.Worktree) { $where = "$($g.Worktree) $($s.Worktree)" }
        $age = Format-RelativeAge -From $s.Modified -Now $Now
        $what = if ($s.LastUser) { $s.LastUser } elseif ($s.Title) { $s.Title } else { '' }
        # The project alone does not identify a session: on this machine twelve consecutive rows
        # are all the same repository. The snippet is what makes the list scannable.
        $room = $leftWidth - $mark.Length - $age.Length - 2
        $label = $where
        if ($what -and $room -gt ($where.Length + 4)) {
            $label = $where + '  ' + (Limit-Line -Text $what -Max ($room - $where.Length - 2))
        }
        $pad = [Math]::Max(1, $leftWidth - $mark.Length - $label.Length - $age.Length - 1)
        $list += $mark + (Limit-Line -Text $label -Max ($leftWidth - $mark.Length - $age.Length - 2)) +
                 (' ' * $pad) + $age + ' '
    }

    $s = $items[$Index]
    $where = $s.Project
    if ($s.Worktree) { $where += "  $($g.Worktree) $($s.Worktree)" }

    $detail = @()
    $detail += ' ' + (Limit-Line -Text $where -Max ($rightWidth - 2))
    $detail += ' ' + $s.Modified.ToString('dd MMM HH:mm') + "  $($g.H)  $($s.PromptCount) msgs  $($g.H)  $('{0:N0}' -f ($s.SizeBytes / 1KB)) KB"
    $detail += ' ' + ([string]$g.H * ($rightWidth - 2))
    # Budget against the body height, not the number of list rows: with three sessions on a 40-row
    # terminal the old arithmetic gave the preview two lines while 30 sat empty. As many recent,
    # attributed messages as this budget allows, oldest at the top - not just the last question and
    # the last answer.
    $room = [Math]::Max(2, $bodyRows - $detail.Count)
    foreach ($l in (Get-ExchangeLines -Messages (Get-SessionExchange -Session $s) -Width ($rightWidth - 2) -MaxLines $room -Glyphs $g)) {
        $detail += ' ' + $l
    }

    $body = Join-Panes -Left $list -Right $detail -LeftWidth $leftWidth -RightWidth $rightWidth -Ascii:$Ascii
    $lines = New-Box -Lines $body -Width $Width -Title $title -Ascii:$Ascii
    # Same derivation as the narrow branch. Join-Panes can make the body TALLER than the list when
    # the detail pane is longer, so the row count is the viewport's, never the body's.
    if ($RowMap) {
        $RowMap.Value = [pscustomobject]@{
            FirstRowY = $lines.Count - $body.Count - 1
            RowCount  = $vp.Visible
            Start     = $vp.Start
        }
    }
    return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $Width -Glyphs $g -Color:$Color -RowMap $RowMap)
}

function Get-MaintenanceFrame {
    param($Info, [int]$Width = 78, [int]$Height = 24, [string]$Status = '', [switch]$Color, [switch]$Ascii, [ref]$RowMap, [object[]]$Actions = @())
    if ($RowMap) { $RowMap.Value = [pscustomobject]@{ Rows = @() } }
    if ($Width -lt $script:MinWidth -or $Height -lt $script:MinHeight) {
        return (Get-TooSmallFrame -Width $Width -Height $Height)
    }
    $g = Get-Glyphs -Ascii:$Ascii
    $boxWidth = [Math]::Min($Width, 100)
    # Both verdicts are kept short enough to survive the MINIMUM width: the label column is 14 and
    # the inner box at 50 columns is 48, so anything past 34 characters is cut - which is how the
    # old wording lost its own advice ('...close every sess...') exactly when it was needed.
    $verdict =
        if ($Info.Matches) { 'yes - newest downloaded build' }
        else { 'NO - older; press u to install it' }
    # Built before the body: how many lines the footer takes decides how much status fits.
    # Every hint here is a single action, so every one of them is clickable - unlike the other two
    # screens, which carry an arrow hint that names two directions at once.
    # The configured actions sit between the built-in ones and esc: one hint per action, keyed by
    # the letter the config assigned it.
    $hints = @(
        @{ Token = 'u';   Label = 'update';      Clickable = $true; Key = '';       Char = 'u' }
        @{ Token = 'r';   Label = 'rename swap'; Clickable = $true; Key = '';       Char = 'r' }
        @{ Token = 'd';   Label = 'doctor';      Clickable = $true; Key = '';       Char = 'd' }
        @{ Token = 'm';   Label = 'mcp list';    Clickable = $true; Key = '';       Char = 'm' }
        @{ Token = 'p';   Label = 'prune';       Clickable = $true; Key = '';       Char = 'p' }
    )
    foreach ($a in $Actions) {
        $hints += @{ Token = $a.Key; Label = $a.Label; Clickable = $true; Key = ''; Char = $a.Key }
    }
    $hints += @{ Token = 'esc'; Label = 'back'; Clickable = $true; Key = 'Escape'; Char = '' }
    $footer = New-HintFooter -Glyphs $g -Width $Width -Hints $hints

    $body = @(
        "  installed   $($Info.BinPath)",
        "  hash        $(if ($Info.InstalledHash) { $Info.InstalledHash.Substring(0, [Math]::Min(16, $Info.InstalledHash.Length)) } else { 'missing' })",
        "  newest      $($Info.NewestVersion)",
        "  hash        $(if ($Info.NewestHash) { $Info.NewestHash.Substring(0, [Math]::Min(16, $Info.NewestHash.Length)) } else { 'none downloaded' })",
        "  match       $verdict",
        "  versions    $($Info.VersionCount) builds, $('{0:N1}' -f ($Info.VersionsBytes / 1GB)) GB"
    )
    if ($Status) {
        # A multi-line status used to go in as ONE row. Its embedded newlines then split that row in
        # the middle of the box - the bottom border and the key hints were pushed down a line and the
        # frame was corrupt - while Limit-Line measured the whole blob as a single 78-column line and
        # threw everything past the first 78 characters away. Rows now come out one per line,
        # wrapped, and capped so the frame can never grow past the terminal.
        #
        # 9 is the fixed chrome around the status: the box top and bottom, the six info rows and the
        # blank separator; the footer's own line count comes on top (one line wide, up to three at
        # 50 columns). The extra row is headroom, and it is load-bearing: Write-Frame appends a
        # newline after EVERY line including the last, so a frame that fills the terminal exactly
        # scrolls the alternate buffer by one - the top border leaves the screen and the next
        # repaint's cursor-home lands a row off. Measured 2026-08-13: at Height 24 the picker
        # returns 22 lines and the launch screen 14; this was the only frame that returned 24.
        $room = [Math]::Max(1, $Height - 10 - @($footer.Lines).Count)
        $body += ''
        $body += @(Split-OutputLines -Text $Status -Width ($boxWidth - 4) -MaxLines $room |
                   ForEach-Object { "  $_" })
    }

    $lines = New-Box -Lines $body -Width $boxWidth -Title 'maintenance' -Ascii:$Ascii
    return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $Width -Glyphs $g -Color:$Color -RowMap $RowMap)
}
