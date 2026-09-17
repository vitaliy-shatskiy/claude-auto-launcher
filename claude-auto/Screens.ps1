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
    # The box is capped at 100 columns (Get-LaunchFrame's $boxWidth) and this is the width-critical
    # row, which is why Config.ps1 limits an account key to 8 characters.
    @{ Name = 'Account';    Label = 'account';    Values = @('work') }
    # Action is no longer a launch-screen row (2026-09-15): the project screen between this one and
    # the session picker decides new/continue/resume/worktree, since it is the one place that
    # already knows WHICH project the action applies to. $State.Action still exists (New-LaunchState
    # keeps it at 'new') - Get-LaunchArgs still reads it, and the project screen's own result
    # (Invoke-ProjectScreen's -Action) sets it in claude-auto.ps1 - only the row that let this
    # screen edit it directly is gone.
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
    if ($Remote) {
        # PowerShell's range operator counts BACKWARDS when Count is 1 (1..0 is the range
        # 1,0, not empty), so $rows[1..($rows.Count-1)] silently re-yielded $rows[0] instead of an
        # empty tail. Guarded explicitly rather than relying on the range never going negative.
        $tail = if ($rows.Count -gt 1) { @($rows[1..($rows.Count - 1)]) } else { @() }
        $rows = @($rows[0]) + @($script:RemoteRow) + $tail
    }
    $script:Rows = $rows
    $script:AccountTints = @{}
    foreach ($a in $Accounts) { $script:AccountTints[$a.Key] = "$($a.Tint)" }
}

# 50 columns since 2026-09-02: the owner launches over RDP from a phone, where 50x50 is what the
# screen gives. Every frame must survive it - footers wrap (New-HintFooter -Width), option rows
# collapse to the selected value, the maintenance verdicts fit 34 characters.
# Height 20, RE-MEASURED 2026-09-15 when the Action row left the launch screen (Task 9: the project
# screen decides new/continue/resume/worktree now) and the footer's 'enter' hint became 'next': at
# 50 columns the worst launch frame is 19 lines - 3 box + 1 blank + 7 rows + 3 bars (five hour,
# seven day, model bucket) + 1 blank + 1 separator + 1 restored + 2 wrapped footer lines - plus the
# headroom row Write-Frame needs. It was 20+1 with the Action row still on this screen.
# Never guess this number: Test-Ui renders that exact LAUNCH frame at an unrefusable height, counts
# it and asserts this constant is the count plus one, so it re-measures itself on every run.
# The "7 rows" above assumes the Remote row is present (remote: true in the config): with
# remote: false the frame is one row shorter and this minimum has headroom to spare.
# The picker and project screens are NOT part of this measurement and must never be (fix round 1,
# Task 10 review, IMPORTANT 1): both scroll, so unlike the launch screen above they have no fixed
# worst-case line count to measure at all - rendered at an unbounded height, their line count grows
# LINEARLY with however many rows the fixture happens to have (measured: project 12/13/40 rows ->
# 19/20/47 lines; picker 6/15/30 rows -> 16/25/40 lines). Test-Ui instead proves both CLAMP their
# viewport to whatever height they are given (a 40-row fixture still fits at MinHeight, and renders
# MORE lines unbound - the only way to tell a real clamp from a merely-short fixture), which is why
# neither can ever push this constant higher than the launch screen's own worst case. Re-measure the
# LAUNCH screen, never the other two, before ever moving this constant again.
$script:MinWidth = 50
$script:MinHeight = 20
$script:TwoPaneWidth = 100

function Get-FrameWidth {
    # The last console column is never written: a line that fills it exactly wraps on some
    # terminals (spec D4). Drawing uses this everywhere $Width was used; the MinWidth check
    # above stays on $Width itself, since that gate is about the terminal, not the drawing budget.
    param([int]$Width)
    return $Width - 1
}

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
        # What the mouse is over: the footer button (-1 for none), the row (-1 for none) and the
        # option value ('' for none). On the STATE because the screen's -Draw takes the state and
        # nothing else, so that is the only channel a hover has to the frame.
        Hover = -1
        HoverRow = -1
        HoverValue = ''
        # Per-tab state (Prefs.ps1). Profiles is this session's stash of the five habit rows per
        # account; Restored/RestoredAge are recomputed on every tab switch, which is why the frame
        # reads them from here rather than from a copy the launcher captured before the screen.
        Profiles = @{}; Restored = @(); RestoredAge = ''
        # The last project directory launched from, per account (Prefs.ps1). Not in $ProfileFields:
        # it is a filesystem path, not a pick from a row's option list, so it is saved and merged by
        # its own explicit lines, validated by existence rather than membership.
        Project = ''
        # The registry slug for Project, when it is a known project - '' for an unrecognised
        # directory (a free path, or a cwd nothing has seen before). Derived, never persisted: it
        # rides along so the session picker can scope by slug (exact) rather than by name (which two
        # repositories can share) without looking the path up in the registry a second time.
        ProjectSlug = ''
        # EVERY slug of that directory, not just the primary one. One real directory can own several
        # slug folders (a cwd recorded with different separators, a folder renamed and renamed back);
        # Get-ProjectRegistry merges those into one row and keeps them all here, and the session
        # picker scopes on the whole set - otherwise half a project's sessions are unreachable from
        # the screen that just named it (adversarial review 2026-09-16, A12).
        ProjectSlugs = @()
        # No ProjectAction here, deliberately (review W1, 2026-09-16): the project screen's action
        # field is NOT remembered. Prefs.ps1's own header states the rule - an action describes one
        # launch, not a habit - and a remembered 'worktree' would turn the next launch's reflexive
        # Enter into a git worktree. The field starts at 'new' every time; $Action still carries the
        # launch itself, set from the screen's result in claude-auto.ps1.
    }
}

# The project screen's action field. Exactly the values claude-auto.ps1 already dispatches on
# (Get-LaunchArgs' switch, and its own `if ($state.Action -ne 'resume')`), so the field can never
# produce one the rest of the launcher has not heard of.
$script:ProjectActions = @('new', 'continue', 'resume', 'worktree')

function Get-ProjectActions { return $script:ProjectActions }

function Step-Option {
    # The one wrap-stepper. Both the launch rows and the project action field used to carry a copy
    # of this arithmetic; a fix to one (the case-canonicalisation, review W4) had to be made twice.
    # Canonicalised once: -in/-eq are case-insensitive, [Array]::IndexOf is not, so 'RESUME' used to
    # pass validation and then step from index 0.
    param([Parameter(Mandatory)][string[]]$Values, [string]$Current, [int]$Delta)
    $canon = @($Values | Where-Object { $_ -eq $Current })
    $i = if ($canon.Count -gt 0) { [Array]::IndexOf($Values, $canon[0]) } else { 0 }
    if ($i -lt 0) { $i = 0 }
    $i = ($i + $Delta) % $Values.Count
    if ($i -lt 0) { $i += $Values.Count }
    return $Values[$i]
}

function Step-ProjectAction {
    # Thin wrapper: the values are the four the launcher dispatches on (Get-LaunchArgs).
    param([string]$Action, [int]$Delta)
    return (Step-Option -Values $script:ProjectActions -Current $Action -Delta $Delta)
}

function Step-LaunchValue {
    # Thin wrapper: steps the row under the cursor and writes the result back onto the state.
    param($State, [int]$Delta)
    $row = $script:Rows[$State.Row]
    $State.($row.Name) = Step-Option -Values @($row.Values) -Current $State.($row.Name) -Delta $Delta
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
    $frameWidth = Get-FrameWidth -Width $Width
    $lines = @(
        "  need $($script:MinWidth)x$($script:MinHeight), have ${Width}x${Height}"
        '  terminal too small'
        ''
        '  resize the window, or press esc'
    )
    $lines = @($lines | ForEach-Object { Limit-Line -Text $_ -Max $frameWidth })
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
    $out = $out -replace '\b(account|model|effort|advisor|permission|remote|mode)\b', ($c.Dim + '$1' + $c.Reset)
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
    param([Parameter(Mandatory)][array]$Hints, [Parameter(Mandatory)][hashtable]$Glyphs, [int]$Width = 0, [switch]$Plain)
    $sep = "  $($Glyphs.HintSep)  "
    $lines = @()
    $text = '  '
    $spans = @()
    foreach ($h in $Hints) {
        # A clickable hint is a BUTTON: the key gets a cell of its own so reverse video paints an
        # even block around it. With colour off that block is invisible, so -Plain brackets the
        # token instead - the structure has to survive NO_COLOR and a dumb terminal. Both forms are
        # the same width (1 + token + 1), which is what keeps wrapping identical either way.
        $token =
            if (-not $h.Clickable) { $h.Token }
            elseif ($Plain) { "[$($h.Token)]" }
            else { " $($h.Token) " }
        $piece = $token
        if ($h.Label) { $piece += ' ' + $h.Label }
        $hasContent = $text.Length -gt 2
        # The wrap is a CELL budget, never .Length - Theme.ps1's rule, and this line was the last
        # place on the screen still measuring the other way. A configured maintenance action with a
        # CJK label counts one code unit per two drawn cells, so the line was laid out as fitting,
        # overflowed -Width, and Complete-PickerFrame then cut it - dropping the spans past the cut
        # (see its own comment) and leaving a half-drawn hint nobody could click.
        if ($Width -gt 0 -and $hasContent -and ((Get-DisplayWidth -Text $text) + (Get-DisplayWidth -Text $sep) + (Get-DisplayWidth -Text $piece)) -gt $Width) {
            $lines += [pscustomobject]@{ Text = $text; Spans = @($spans) }
            $text = '  '
            $spans = @()
            $hasContent = $false
        }
        if ($hasContent) { $text += $sep }
        $start = $text.Length
        $tokenEnd = $start + $token.Length - 1
        $text += $piece
        # One shape for both cases: a non-clickable hint is still worth painting - the arrow hints
        # are how the keyboard is discovered, and dimming them uniformly is what makes the actions
        # stand out - so it gets the same span with Start/End sentinelled to -1 rather than a whole
        # separate object literal.
        $spans += [pscustomobject]@{
            Start = if ($h.Clickable) { $start } else { -1 }
            End = if ($h.Clickable) { $text.Length - 1 } else { -1 }
            KeyStart = $start; KeyEnd = $tokenEnd
            Key = if ($h.Clickable) { $h.Key } else { '' }
            Char = if ($h.Clickable) { $h.Char } else { '' }
            Line = $lines.Count
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
    #
    # -HasHover singles out ONE clickable span for the accent-filled cap; -Selected (Task 6, spec
    # D1) does the same for every span naming the action the field currently reads, so the button
    # that Enter will run is always lit even with the mouse elsewhere. Both match by (Key, Char)
    # rather than position: those two fields are what New-HintFooter gives every hint to keep it
    # unique, and they survive a footer that wraps onto a second line where a plain column offset
    # would not. Idle buttons get the DIM inverse block (ButtonBg/ButtonFg), never bare reverse
    # video - a footer full of white blocks read as one undifferentiated wall of buttons.
    param([string]$Line, [array]$Spans, [switch]$Enabled, [switch]$HasHover, [string]$HoverKey = '', [string]$HoverChar = '', [array]$Selected = @())
    if (-not $Enabled -or -not $Line -or -not $Spans) { return $Line }
    $c = $script:C
    $out = ''
    $cursor = 0
    foreach ($s in ($Spans | Sort-Object KeyStart)) {
        if ($s.KeyStart -lt $cursor -or $s.KeyEnd -ge $Line.Length) { continue }
        $out += $c.Dim + $Line.Substring($cursor, $s.KeyStart - $cursor) + $c.Reset
        $isHovered  = $HasHover -and $s.Start -ge 0 -and $s.Key -eq $HoverKey -and $s.Char -eq $HoverChar
        $isSelected = $s.Start -ge 0 -and @($Selected | Where-Object { $_.Key -eq $s.Key -and $_.Char -eq $s.Char }).Count -gt 0
        $capTint   = if ($isHovered -or $isSelected) { $c.AccentBg + $c.AccentFg } elseif ($s.Start -ge 0) { $c.ButtonBg + $c.ButtonFg } else { $c.Dim }
        $labelTint = if ($isHovered -or $isSelected) { $c.Bold } else { $c.Dim }
        $out += $capTint + $Line.Substring($s.KeyStart, $s.KeyEnd - $s.KeyStart + 1) + $c.Reset
        $cursor = $s.KeyEnd + 1
        # Clamped to the (possibly Limit-Line-truncated) line: Complete-PickerFrame paints against
        # the UNFILTERED span list (only its own row-map bookkeeping filters End -lt Length), so a
        # span measured on the full text can outrun a footer cut short at low width.
        if ($s.End -ge $cursor -and $cursor -lt $Line.Length) {
            $labelEnd = [Math]::Min($s.End, $Line.Length - 1)
            $out += $labelTint + $Line.Substring($cursor, $labelEnd - $cursor + 1) + $c.Reset
            $cursor = $labelEnd + 1
        }
    }
    if ($cursor -lt $Line.Length) { $out += $c.Dim + $Line.Substring($cursor) + $c.Reset }
    return $out
}

function Get-FooterHover {
    # Which (Key, Char) pair a hovered footer-button INDEX names, or "nothing hovered". The index is
    # into the SAME clickable-span order Complete-PickerFrame later flattens into RowMap.Footer, so
    # index N here is index N there - which is what lets a loop hand straight back the index
    # Get-HitAt gave it. Add-HintColor matches on (Key, Char) rather than on a column, so this
    # survives a footer that wraps onto a second line.
    # ONE copy for all four screens (spec D1/D3): written per screen, the copies drift the first time
    # the span bookkeeping changes and a button lights on the wrong hint.
    param([Parameter(Mandatory)]$Footer, [int]$Hover = -1)
    $none = [pscustomobject]@{ HasHover = $false; Key = ''; Char = '' }
    if ($Hover -lt 0) { return $none }
    $clickable = @($Footer.Lines | ForEach-Object { $_.Spans } | Where-Object { $_.Start -ge 0 })
    if ($Hover -ge $clickable.Count) { return $none }
    return [pscustomobject]@{ HasHover = $true; Key = $clickable[$Hover].Key; Char = $clickable[$Hover].Char }
}

# ONE memo per list screen (spec D11), keyed on every input EXCEPT the three hover ones. A hover is
# the only input whose whole effect is a band on one row, one radio piece and one footer button - so
# it is the only input that can be served by repainting those lines out of the last frame instead of
# rebuilding the picture behind them, which is what a mouse asks for dozens of times a second. One
# entry per builder: a screen only ever redraws its own last frame.
$script:FrameMemo = @{}

function Get-ListSignature {
    # "The same list, unchanged" as one string - the part of a memo key that the rows come from.
    # EVERY field a row or the preview is rendered from belongs in it: a field left out is a memo
    # that keeps showing a row whose text has moved on. A reference-typed field (a RecentMessages
    # array) is signed by object identity, which can only produce a false MISS - one rebuild too
    # many - never a false hit.
    param([array]$Items, [string[]]$Fields)
    $sb = [Text.StringBuilder]::new()
    $null = $sb.Append(@($Items).Count)
    foreach ($it in $Items) {
        foreach ($f in $Fields) {
            $null = $sb.Append([char]31)
            $v = $it.$f
            if ($v -is [datetime]) { $null = $sb.Append($v.Ticks) }
            elseif ($null -eq $v -or $v -is [string] -or $v -is [ValueType]) { $null = $sb.Append([string]$v) }
            else { $null = $sb.Append([Runtime.CompilerServices.RuntimeHelpers]::GetHashCode($v)) }
        }
    }
    return $sb.ToString()
}

function Get-SessionListSignature {
    # The same answer as Get-ListSignature for the picker's twelve session fields, at a quarter of
    # the property reads - and this key is rebuilt on every hover, hit or miss. A session object is
    # constructed once and never written to after Get-ClaudeSessions hands it over, so its IDENTITY
    # already stands for Title, LastUser, RecentMessages and the rest (controller ruling P19).
    # Path and Project are the two exceptions and stay read by value: Get-ClaudeSessions refreshes
    # both IN PLACE on a cached summary, and Read-SessionsCache's busy fallback can serve the SAME
    # object to a second call - identity alone would leave a renamed project on the row.
    # Modified.Ticks costs one read and closes the remaining gap: a transcript that grew arrives as
    # a new object anyway, but a fixture built twice from one template would not.
    param([array]$Items)
    $parts = [Collections.Generic.List[string]]::new()
    $parts.Add([string]@($Items).Count)
    foreach ($it in $Items) {
        $parts.Add([string][Runtime.CompilerServices.RuntimeHelpers]::GetHashCode($it))
        $parts.Add([string]$it.Modified.Ticks)
        $parts.Add([string]$it.Path)
        $parts.Add([string]$it.Project)
    }
    return ($parts -join [string][char]31)
}

function Get-RowBandLength {
    # How many CHARACTERS of a finished frame line the hover band wraps for one list row: the row
    # itself plus the pad that follows it, because a hovered row carries that pad INSIDE its band
    # (New-ListRow's right gutter, the free-path row's fill) while an unhovered row gets the
    # identical spaces from New-Box or Join-Panes afterwards - the two lines are the same bytes.
    # Measured on the row WITHOUT its markers: they cost no cells and no columns (Theme.ps1).
    param([string]$Row, [int]$PaneWidth)
    $plain = $Row
    if ($plain.IndexOf($script:HoverOpen) -ge 0) { $plain = $plain.Replace([string]$script:HoverOpen, '').Replace([string]$script:HoverClose, '') }
    return ($plain.Length + [Math]::Max(0, $PaneWidth - (Get-DisplayWidth -Text $plain)))
}

function Add-HoverSpanAt {
    # Add-HoverSpan for a piece of an already-finished line: the same two markers, around the columns
    # the builder itself would have wrapped. $null when the range does not fit - a line that New-Box
    # or Join-Panes had already cut is one where the builder's markers would have landed elsewhere,
    # and the caller must rebuild rather than guess.
    param([string]$Line, [int]$Start, [int]$Length)
    if ($Start -lt 0 -or $Length -le 0 -or ($Start + $Length) -gt $Line.Length) { return $null }
    return $Line.Substring(0, $Start) + [string]$script:HoverOpen + $Line.Substring($Start, $Length) + [string]$script:HoverClose + $Line.Substring($Start + $Length)
}

function Format-FrameLine {
    # The per-line tail of every frame - truncate, then paint - as ONE function, because the memo's
    # repaint has to run the identical chain. The ORDER is the contract (dim spans first, then the
    # body painter, then the hover band LAST - see Complete-PickerFrame's own comments); a second
    # copy of it would drift, and a drifted repaint shows as one row painted unlike its neighbours.
    param([string]$Line, [int]$Width, [hashtable]$Glyphs, [switch]$Color,
          [ValidateSet('picker', 'launch')][string]$Body = 'picker',
          [switch]$Footer, [array]$Spans = @(),
          [switch]$HasHover, [string]$HoverKey = '', [string]$HoverChar = '', [array]$Selected = @())
    $l = Limit-Line -Text $Line -Max $Width
    # The footer is painted by column span, not by pattern: its words ('row', 'value', 'start') are
    # ordinary English and a pattern-based rule would tint them wherever else they appear.
    if ($Footer) { return (Add-HintColor -Line $l -Spans $Spans -Enabled:$Color -HasHover:$HasHover -HoverKey $HoverKey -HoverChar $HoverChar -Selected $Selected) }
    # The dim spans a builder marked are resolved FIRST - before the pattern painters, which search
    # for glyphs and words and would otherwise have to find them again inside an escape sequence -
    # and on EVERY body line, coloured or not: with colour off this is what strips the markers, so
    # nothing internal reaches a terminal or a check reference.
    $l = Add-DimSpanColor -Line $l -Enabled:$Color
    $l = if ($Body -eq 'launch') { Add-LaunchColor -Line $l -Enabled:$Color -Glyphs $Glyphs }
         else { Add-PickerColor -Line $l -Enabled:$Color -Glyphs $Glyphs }
    # The hover band is resolved LAST (spec D10), after both painters above: it paints a BACKGROUND
    # across text they have already tinted, and every Reset they left inside the band would end it -
    # so the band has to be the pass that sees them and paints over each one. Unmarked lines leave
    # it untouched, which is every line of every frame that has nothing hovered.
    return (Add-HoverSpanColor -Line $l -Enabled:$Color)
}

function Get-MemoFrame {
    # The hit path: the same frame with a different hover. Repaints only the lines whose hover state
    # actually changed - the row that lost the band, the row that gained it, the action row when the
    # value under the pointer moved, the footer lines when the lit button did - and hands back the
    # rest of the cached picture untouched. $null means "nothing memoised for this key", and the
    # caller builds; it is also the answer when a band would not fit the cached line, so a frame that
    # was truncated somewhere unexpected is rebuilt rather than approximated.
    param([string]$Builder, [string]$Key, [int]$HoverRow = -1, [string]$HoverValue = '', [int]$Hover = -1, $RowMap)
    $e = $script:FrameMemo[$Builder]
    if (-not $e -or $e.Key -ne $Key) { return $null }
    # A frame built without a row map has none to hand back, and a caller asking for one needs the
    # real thing - the hit test is derived from the same build that drew the rows, and a $null map
    # would leave every click inert. Rebuild instead.
    if ($RowMap -and $null -eq $e.Map) { return $null }
    $painted = $e.Painted.Clone()
    $was = $e.Hover

    if ($was.Row -ne $HoverRow) {
        # BOTH rows: the one losing the band needs repainting exactly as much as the one gaining it.
        foreach ($idx in @($was.Row, $HoverRow)) {
            $span = $e.Rows[$idx]
            if (-not $span) { continue }
            $line = $e.Unpainted[$span.Y]
            if ($idx -eq $HoverRow) {
                $line = Add-HoverSpanAt -Line $line -Start $span.Start -Length $span.Length
                if ($null -eq $line) { return $null }
            }
            $painted[$span.Y] = Format-FrameLine -Line $line -Width $e.Width -Glyphs $e.Glyphs -Color:$e.Color -Body $e.Body
        }
    }
    if ($e.Action -and $was.Value -ne $HoverValue) {
        $line = $e.Unpainted[$e.Action.Y]
        $span = $e.Action.Values[$HoverValue]
        if ($span) {
            $line = Add-HoverSpanAt -Line $line -Start $span.Start -Length $span.Length
            if ($null -eq $line) { return $null }
        }
        $painted[$e.Action.Y] = Format-FrameLine -Line $line -Width $e.Width -Glyphs $e.Glyphs -Color:$e.Color -Body $e.Body
    }
    if ($was.Footer -ne $Hover) {
        # Every footer line, not just one: the lit button is matched by (Key, Char) and can sit on
        # any line the footer wrapped onto.
        $hov = Get-FooterHover -Footer $e.Footer -Hover $Hover
        for ($f = 0; $f -lt $e.FooterSpans.Count; $f++) {
            $y = $e.FooterIndex + $f
            $painted[$y] = Format-FrameLine -Line $e.Unpainted[$y] -Width $e.Width -Glyphs $e.Glyphs -Color:$e.Color -Body $e.Body `
                                            -Footer -Spans $e.FooterSpans[$f] -HasHover:$hov.HasHover -HoverKey $hov.Key -HoverChar $hov.Char -Selected $e.Selected
        }
    }
    $e.Painted = $painted
    $e.Hover = @{ Row = $HoverRow; Value = $HoverValue; Footer = $Hover }
    if ($RowMap) { $RowMap.Value = $e.Map }
    return @($painted)
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
        [ValidateSet('picker', 'launch')][string]$Body = 'picker',
        # Forwarded to Add-HintColor untouched. Every existing caller omits these, so every existing
        # frame paints exactly as before - only a caller that names a hovered (Key, Char) pair, or a
        # -Selected list, changes what comes out.
        [switch]$HasHover, [string]$HoverKey = '', [string]$HoverChar = '', [array]$Selected = @(),
        # The bookkeeping a list screen hands in so its frame can be memoised (spec D11): the key,
        # the hover state these lines were painted for, and where a band goes on every row and radio
        # value the NEXT frame could hover. Omitted by the launch and maintenance screens, which
        # keep no memo - one mouse move there is one frame, and always was.
        [hashtable]$Memo = $null
    )
    $footerLines = @($Footer.Lines)
    if ($footerLines.Count -eq 0) { $footerLines = @([pscustomobject]@{ Text = $Footer.Text; Spans = @($Footer.Spans) }) }
    $all = @($Lines)
    $footerIndex = $all.Count
    $visible = @()
    for ($f = 0; $f -lt $footerLines.Count; $f++) {
        # Stripped BEFORE Limit-Line and before the spans are measured against the cut: the footer is
        # painted by COLUMN INDEX, so a marker surviving into it would shift every span by one, and a
        # marker cut in half by the truncation would reach the terminal.
        # No -Enabled here: footers cannot carry dim spans - stripped by design, always, whether or
        # not colour is on.
        $footerText = Limit-Line -Text (Add-DimSpanColor -Line $footerLines[$f].Text) -Max $Width
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
        # Both branches through Format-FrameLine, which is also what the memo's repaint calls: one
        # painter chain, never two. The footer path carries no hover markers - a footer is lit by
        # (Key, Char) through Add-HintColor, never by a band.
        if ($i -ge $footerIndex) {
            $painted += Format-FrameLine -Line $all[$i] -Width $Width -Glyphs $Glyphs -Color:$Color -Body $Body `
                                         -Footer -Spans $footerLines[$i - $footerIndex].Spans -HasHover:$HasHover -HoverKey $HoverKey -HoverChar $HoverChar -Selected $Selected
        } else {
            $painted += Format-FrameLine -Line $all[$i] -Width $Width -Glyphs $Glyphs -Color:$Color -Body $Body
        }
    }
    if ($Memo) {
        # Stored from the SAME $all these lines were painted from, with the hover markers taken back
        # out: those markers are the only difference between a hovered row and an unhovered one -
        # every pad a band adds inside itself is a pad the box or the pane would have added outside
        # it - so one stripped copy answers every hover the next frame can ask for.
        $unpainted = @(foreach ($l in $all) {
            if ($l -and ($l.IndexOf($script:HoverOpen) -ge 0 -or $l.IndexOf($script:HoverClose) -ge 0)) {
                $l.Replace([string]$script:HoverOpen, '').Replace([string]$script:HoverClose, '')
            } else { [string]$l }
        })
        $script:FrameMemo[$Memo.Builder] = @{
            Key = $Memo.Key; Unpainted = $unpainted; Painted = $painted
            Hover = @{ Row = [int]$Memo.HoverRow; Value = [string]$Memo.HoverValue; Footer = [int]$Memo.Hover }
            Rows = $Memo.Rows; Action = $Memo.Action
            FooterIndex = $footerIndex; FooterSpans = @($footerLines | ForEach-Object { , @($_.Spans) })
            Footer = $Footer; Selected = $Selected
            Width = $Width; Glyphs = $Glyphs; Color = [bool]$Color; Body = $Body
            Map = $(if ($RowMap) { $RowMap.Value } else { $null })
        }
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
    $frameWidth = Get-FrameWidth -Width $Width
    $boxWidth = [Math]::Min($frameWidth, 100)
    $inner = $boxWidth - 4
    # Layout breakpoint about the TERMINAL width, not the drawing budget - stays on $Width by design.
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
    $pad = [Math]::Max(1, $inner - (Get-DisplayWidth -Text $left) - (Get-DisplayWidth -Text $right))
    $header = @($left + (' ' * $pad) + $right)

    $body = @()
    $rowHits = @()
    $limit = $Limits[$State.Account]
    # What the mouse is over (spec D8/D10), read off the state exactly as $State.Hover already is -
    # and defaulted the same way, because a hand-built fixture carries neither field and hovers
    # nothing rather than row 0.
    $hoverRow = if ($null -ne $State.HoverRow) { [int]$State.HoverRow } else { -1 }
    $hoverValue = "$($State.HoverValue)"
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
                if ((Get-DisplayWidth -Text ($prefix + $labelPart + ($cells -join $joiner))) -le $inner) { break }
            }
            # Column spans for each tab, measured off the same strings that were just joined. A click
            # inside one of these means "this account", which is what makes the strip a menu rather
            # than a picture of one.
            # Drawn from a marked copy, measured from the plain one: the click cells are the reason
            # this strip is a menu, and they must be the identical columns whether or not the mouse
            # happens to be over one of them (the markers are zero cells, so the two agree anyway -
            # this keeps them agreeing by construction rather than by arithmetic).
            $drawCells = @(for ($ci = 0; $ci -lt $cells.Count; $ci++) {
                if ($hoverRow -eq $i -and "$($row.Values[$ci])" -eq $hoverValue) { Add-HoverSpan -Text $cells[$ci] } else { $cells[$ci] }
            })
            $line = $prefix + $labelPart + ($drawCells -join $joiner)
            $cellHits = @(Measure-CellSpans -Pieces $cells -Joiner $joiner -Values @($row.Values) -StartX (Get-DisplayWidth -Text ($prefix + $labelPart)))
        } else {
            # Every other row is a radio row, drawn by the builder the project screen's action field
            # uses too: one layout, one set of click cells, no second copy to drift.
            $optionLabels = @{}
            foreach ($v in $row.Values) {
                $optionLabels[$v] = Get-RowOptionText -Row $row -Key $v -DefaultModelLabel $DefaultModelLabel -DefaultAdvisorLabel $DefaultAdvisorLabel
            }
            # The band goes on the hovered VALUE, and only while the pointer is on this row: two rows
            # can offer the same value ('default' is on three of them), and matching on the value
            # alone would light every one of them at once.
            $radio = New-RadioRow -Prefix $prefix -Label ($row.Label + $mark) -Values @($row.Values) -Current $current -Glyphs $g -Labels $optionLabels -MaxWidth $inner `
                                  -Hover $(if ($hoverRow -eq $i) { $hoverValue } else { '' })
            $line = $radio.Text
            $cellHits = @($radio.Cells)
        }

        # The model row's human-readable labels ('Sonnet 5[1M]', a resolved 'default (...)') can
        # outgrow the box even in New-RadioRow's compact form - and a mid-word cut there is exactly
        # what this exists to avoid. Collapse to the selected value alone, marked with ‹ › to say
        # more options exist off-screen (the footer already explains left/right cycles them).
        if ((Get-DisplayWidth -Text $line) -gt $inner) {
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
    # Arrows are deliberately NOT clickable: "w/s" (or "a/d") names two directions, and a click on
    # it cannot mean one of them. Everything that IS a single action is. w/s and a/d, not
    # up/down and left/right (2026-09-09): WASD navigates every screen with a cursor now, and
    # naming it here is what tells the owner the shorter keys exist at all.
    $footer = New-HintFooter -Glyphs $g -Width $frameWidth -Plain:(-not $Color) -Hints @(
        @{ Token = 'w/s'; Label = 'row';         Clickable = $false }
        @{ Token = 'a/d'; Label = 'value';       Clickable = $false }
        @{ Token = 'enter';      Label = 'next';        Clickable = $true; Key = 'Enter';  Char = '' }
        @{ Token = 'u';          Label = 'maintenance'; Clickable = $true; Key = '';       Char = 'u' }
        @{ Token = 'esc';        Label = 'quit';        Clickable = $true; Key = 'Escape'; Char = '' }
    )
    # A state that carries no Hover at all (a hand-built fixture) hovers nothing.
    $hov = Get-FooterHover -Footer $footer -Hover $(if ($null -ne $State.Hover) { [int]$State.Hover } else { -1 })
    return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $frameWidth -Glyphs $g -Color:$Color -RowMap $RowMap -Body launch -HasHover:$hov.HasHover -HoverKey $hov.Key -HoverChar $hov.Char)
}

function New-ListRow {
    # mark + label + pad + tail + ' ' + age, in exactly -Width cells. The label is clamped FIRST so a
    # very long name cannot push the age off the row; the tail (a path, a snippet) takes whatever is
    # left and is dropped below 9 cells - a nine-character path fragment identifies nothing. Both
    # the project screen and the session picker draw their rows through here; the two copies they
    # carried before differed only in which column they cut first.
    # -TrailingSpace: the session picker's rows put the separator AFTER the age (age + ' ') rather
    # than before it (' ' + age) - a difference from the project rows that predates this function
    # and is kept verbatim rather than reflowed, since the byte-identity rule covers text, not taste.
    # -DimTail / -DimAge (spec D6): wrap that column in the zero-width dim-span markers, so the
    # painter can tint the PATH and the AGE without a pattern rule having to find them again in a
    # finished row - which it cannot do reliably: a path is not a word, and inside a box the age is
    # not at the end of the line either. The markers cost no cells, so everything below still
    # measures the same (Theme.ps1, Add-DimSpanColor).
    # -PathTail (spec D4): the tail is a PATH, so cut it in the MIDDLE and keep its leaf. Only the
    # caller knows which it is - the project rows pass paths, the picker's rows pass a conversation
    # snippet, and middle-truncating a snippet cuts away the one half of it anybody reads.
    # -Hover (spec D10): the row is the one under the pointer, so the WHOLE row - mark column
    # included - carries the band. Wrapped after the arithmetic like the dim markers below, and
    # before the mark rather than after it, which is what keeps the free-path row's bullet anchor
    # in Add-PickerColor able to find the mark column at all.
    param([string]$Mark = '   ', [string]$Label = '', [string]$Tail = '', [string]$Age = '', [int]$Width, [switch]$Ascii, [switch]$TrailingSpace,
          [switch]$DimTail, [switch]$DimAge, [switch]$PathTail, [switch]$Hover)
    $ageW = Get-DisplayWidth -Text $Age
    $ageCol = if ($Age) { if ($TrailingSpace) { $Age + ' ' } else { ' ' + $Age } } else { '' }
    # Review fix round 1 (C1): the reserve here is mark + a 1-cell minimum pad + the age's OWN
    # width, same as the old inline code (`$inner - $mark.Length - $age.Length - 2`) - using
    # $ageCol's width instead double-counts the separator $ageCol already carries and clamps the
    # label one cell too early (an "…" that used to fit no longer does).
    $label = Limit-Line -Text $Label -Max ([Math]::Max(1, $Width - (Get-DisplayWidth -Text $Mark) - $ageW - 2)) -Ascii:$Ascii
    $room = $Width - (Get-DisplayWidth -Text $Mark) - (Get-DisplayWidth -Text $label) - (Get-DisplayWidth -Text $ageCol) - 3
    $tail =
        if (-not $Tail -or $room -le 8) { '' }
        elseif ($PathTail) { Limit-Path -Text $Tail -Max $room -Ascii:$Ascii }
        else { Limit-Line -Text $Tail -Max $room -Ascii:$Ascii }
    # Review fix round 1 (Important): with no -Age (the cwd row) the old inline code filled only
    # $inner - 1 cells, leaving one cell of right gutter before the box border. An age column
    # spends that cell on the separator already folded into $ageCol; without one, nothing does -
    # so only the no-age case reserves it here.
    $gutter = if ($Age) { 0 } else { 1 }
    $pad = [Math]::Max(1, $Width - (Get-DisplayWidth -Text $Mark) - (Get-DisplayWidth -Text $label) - (Get-DisplayWidth -Text $tail) - (Get-DisplayWidth -Text $ageCol) - $gutter)
    # Marked AFTER the arithmetic above, never before it: every width here is measured on the plain
    # column, so a marked row and an unmarked one are laid out by the identical numbers.
    if ($DimTail -and $tail) { $tail = [string]$script:DimOpen + $tail + [string]$script:DimClose }
    if ($DimAge -and $ageCol) { $ageCol = [string]$script:DimOpen + $ageCol + [string]$script:DimClose }
    $row = $Mark + $label + (' ' * $pad) + $tail + $ageCol
    # The right gutter goes INSIDE the band (P10), and only when the row is hovered: every hovered
    # list row then spans the identical inner width, while an unhovered row keeps the exact bytes it
    # always had. Without it an age-less row (the cwd row) banded one cell short of the rows above
    # and below it, which reads as a rendering fault rather than as a highlight.
    if ($Hover) { $row = Add-HoverSpan -Text ($row + (' ' * $gutter)) }
    return $row
}

function Measure-CellSpans {
    # Start/End (inclusive) frame columns for a list of pieces about to be joined, measured in
    # DISPLAY CELLS - Get-DisplayWidth, never .Length, for the reason Theme.ps1 records. The account
    # tab strip and every radio row need exactly this arithmetic; written twice, the two copies drift
    # the first time a joiner changes and the drift shows up as clicks landing on the wrong value.
    # -Values: the VALUE each piece stands for, carried on the span itself. Both callers zipped the
    # spans back onto their own value list in a second for-loop afterwards - two copies of the same
    # index arithmetic, which is exactly what this function exists to prevent (C2). A span with no
    # -Values carries Value = $null; nothing reads it there.
    param([string[]]$Pieces = @(), [string]$Joiner = ' ', [int]$StartX = 0, [string[]]$Values = @())
    $spans = @()
    $x = $StartX
    $joinerWidth = Get-DisplayWidth -Text $Joiner
    for ($i = 0; $i -lt $Pieces.Count; $i++) {
        $w = Get-DisplayWidth -Text $Pieces[$i]
        $spans += [pscustomobject]@{
            Start = $x; End = $x + $w - 1
            Value = $(if ($i -lt $Values.Count) { $Values[$i] } else { $null })
        }
        $x += $w + $joinerWidth
    }
    return @($spans)
}

function New-RadioRow {
    # One option row: 'label  ○ a  ● [b]  ○ c' with a click cell per value. The launch screen drew
    # this inline; the project screen's action field now uses the same row, so a value never has to
    # be discovered behind ‹ › caps and nothing on the row moves when the value changes.
    # -MaxWidth: when the full form does not fit, the COMPACT form drops the On/Off glyphs and keeps
    # only the brackets ('label  a  [b]  c'); the cells still cover each value. Measured in cells
    # (Get-DisplayWidth), never .Length - Theme's rule.
    # -Hover (spec D10): the VALUE under the pointer, '' for none. Only that value's piece gets the
    # band - a row is a menu of several options and banding all of it would say the whole row is
    # under the mouse. The spans below are measured on the marked pieces, which is safe because the
    # markers are zero cells (Theme.ps1): the click cells land exactly where they land unhovered.
    param([string]$Prefix = '   ', [string]$Label, [Parameter(Mandatory)][string[]]$Values, [string]$Current,
          [Parameter(Mandatory)][hashtable]$Glyphs, [hashtable]$Labels = @{}, [int]$LabelWidth = 12, [int]$MaxWidth = 0,
          [string]$Hover = '')
    $labelPart = $Label.PadRight($LabelWidth)
    # ONE resolver for the text a value is drawn as (C3): the compact-form gate below asked the same
    # question in its own copy, and the two answering differently is how a row would be laid out from
    # one string and measured from another.
    $labelOf = { param([string]$v) if ($Labels.ContainsKey($v)) { "$($Labels[$v])" } else { "$v" } }
    $build = {
        param([bool]$Compact)
        $plain = @(foreach ($v in $Values) {
            $text = & $labelOf $v
            if ($Compact) { if ($v -eq $Current) { "[$text]" } else { "$text" } }
            elseif ($v -eq $Current) { "$($Glyphs.On) [$text]" }
            else { "$($Glyphs.Off) $text" }
        })
        # Where each value's piece sits in CHARACTERS of the unmarked row. The frame memo wraps a
        # band around exactly these columns when the pointer moves onto a value, and the markers it
        # inserts cost none of them (Theme.ps1) - so an offset measured here holds there. Characters,
        # not cells, because a band is a string operation; $cells below stays the CLICK arithmetic.
        $chars = @()
        $at = ($Prefix + $labelPart).Length
        for ($i = 0; $i -lt $plain.Count; $i++) {
            $chars += [pscustomobject]@{ Value = $Values[$i]; Start = $at; Length = $plain[$i].Length }
            $at += $plain[$i].Length + 1
        }
        # The band goes over the piece as drawn, so the CURRENT value keeps its brackets and its
        # On glyph under it (spec D10) - hovering the selected value must not redraw it as an
        # unselected one, or the mouse would look like it had changed the setting.
        $pieces = @(for ($i = 0; $i -lt $plain.Count; $i++) {
            if ($Hover -and $Values[$i] -eq $Hover) { Add-HoverSpan -Text $plain[$i] } else { $plain[$i] }
        })
        # Spans off the pieces themselves, not off the joined line: the value a click means is the
        # piece it lands in, and re-finding it in the finished string would match the wrong one the
        # first time two values share a prefix.
        $cells = @(Measure-CellSpans -Pieces $pieces -Joiner ' ' -Values $Values -StartX (Get-DisplayWidth -Text ($Prefix + $labelPart)))
        [pscustomobject]@{ Text = ($Prefix + $labelPart + ($pieces -join ' ')); Cells = $cells; Chars = @($chars) }
    }
    $full = & $build $false
    if ($MaxWidth -le 0 -or (Get-DisplayWidth -Text $full.Text) -le $MaxWidth) { return $full }
    # The compact form separates values with ONE space and nothing else, so a value whose own label
    # carries a space stops being one value: the remote row would read '   remote      on off on+QR
    # [stop server]' - four values that parse as six words - and the model row's labels ('Fable 5.1',
    # 'Sonnet 5[1M]') do the same. Such a row keeps the FULL form even over -MaxWidth and lets the
    # caller's own overflow branch (Get-LaunchFrame's ‹ › collapse) handle it, exactly as before this
    # row existed. Controller ruling R4 - deliberately not a two-space or middle-dot separator, which
    # would change every row that already reads correctly.
    # Keys on a literal space in the RENDERED label (labelOf's output - a Labels[] override when one
    # exists, the raw value otherwise), never in $v itself: the value 'sonnet1m' has none, but the
    # label it draws as, 'Sonnet 5[1M]', does.
    foreach ($v in $Values) {
        if ((& $labelOf $v) -match ' ') { return $full }
    }
    return (& $build $true)
}

function Get-ProjectFrame {
    # Where the session will run, and what it will do there. Pure like every builder in this file.
    # The pinned rows (current directory, enter a path) sit after the registry so the common case -
    # the project you were just in - is the first row and one Enter away.
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][array]$Projects,
        [int]$Index = 0,
        [string]$Filter = '',
        [switch]$Typing,
        [string]$Cwd = '',
        [int]$Width = 78,
        [int]$Height = 24,
        [datetime]$Now = (Get-Date),
        [switch]$Color,
        [switch]$Ascii,
        # -Hover names a clickable footer-button INDEX (matching the order Get-ClaudeFooterHit and
        # the input loop use: [Array]::IndexOf into the RowMap's flattened Footer list), -1 for none.
        [int]$Hover = -1,
        # A rejected pick (a vanished directory, a free path that does not exist) - shown once in the
        # title and cleared by the loop on the next key, so the reason a press did nothing is never
        # silent.
        [string]$Notice = '',
        # The action field under the list (2026-09-16). An INDICATOR, never a cursor stop, so it
        # takes no focus parameter. A value outside Get-ProjectActions renders as the first one
        # rather than being printed raw.
        [string]$Action = 'new',
        # What the mouse is over (spec D10), from the loop's own state: -HoverRow is a LIST index
        # (the same index -Index is, not a screen line), -1 for none; -HoverValue is the action
        # value under the pointer, '' for none. Hover PAINTS only - the cursor, the action and the
        # preview are wherever a click or a key put them (spec D8), so neither of these is ever read
        # as a selection here.
        [int]$HoverRow = -1,
        [string]$HoverValue = '',
        [ref]$RowMap
    )
    if ($RowMap) { $RowMap.Value = [pscustomobject]@{ FirstRowY = 0; RowCount = 0; Start = 0 } }
    if ($Width -lt $script:MinWidth -or $Height -lt $script:MinHeight) {
        return (Get-TooSmallFrame -Width $Width -Height $Height)
    }
    $g = Get-Glyphs -Ascii:$Ascii
    $frameWidth = Get-FrameWidth -Width $Width
    # The memo (spec D11): every input EXCEPT the three hover ones. -Now to the MINUTE, because the
    # only thing it feeds is the age column and that column changes once a minute - a key to the
    # tick would miss on every single frame and the memo would never answer anything.
    $memoKey = "$Width|$Height|$Color|$Ascii|$Index|$Filter|$Typing|$Notice|$Action|$Cwd|" + $Now.ToString('yyyyMMddHHmm') + '|' +
               (Get-ListSignature -Items $Projects -Fields @('Name', 'Path', 'Slug', 'Worktree', 'LastActivity'))
    $memoHit = Get-MemoFrame -Builder 'project' -Key $memoKey -HoverRow $HoverRow -HoverValue $HoverValue -Hover $Hover -RowMap $RowMap
    if ($null -ne $memoHit) { return $memoHit }
    $items = @(Select-ProjectMatch -Projects $Projects -Filter $Filter)
    # The pinned rows are rows: they are selected, hit-tested and entered exactly like a project, so
    # the loop below never needs to know which kind it is looking at. The CURRENT DIRECTORY leads
    # (spec D6): arriving here from a terminal already standing in the right folder is the common
    # case, and it must be one Enter away rather than a walk past the whole registry. Invoke-
    # ProjectScreen's own $rowsFor builds the identical order - the two are pinned against each other.
    $rows = @([pscustomobject]@{ Kind = 'cwd'; Item = [pscustomobject]@{ Name = 'current directory'; Path = $Cwd; LastActivity = $null } })
    $rows += @($items | ForEach-Object { [pscustomobject]@{ Kind = 'project'; Item = $_ } })
    $rows += [pscustomobject]@{ Kind = 'path'; Item = [pscustomobject]@{ Name = 'enter a path...';   Path = '';   LastActivity = $null } }

    $title = "project $($g.H) $($items.Count) known"
    # -Typing shows the filter box the moment '/' is pressed, before any character narrows it, and
    # the trailing '_' is the only cursor this plain-text title has room for.
    # Through the same sanitiser every other transcript-sourced field on this screen goes through.
    # Today the loop's own keystroke whitelist is what keeps an escape out of -Filter, and -Notice is
    # only ever set to a literal - so this is defence in depth, not a live hole; but every other
    # field here is protected structurally and these two were the exception (adversarial review
    # 2026-09-16, A12). A future caller passing text should not have to know.
    if ($Filter -or $Typing) { $title += " $($g.H) filter: $(Get-CleanTranscriptText -Text $Filter)"; if ($Typing) { $title += '_' } }
    if ($Notice) { $title += " $($g.H) $(Get-CleanTranscriptText -Text $Notice)" }

    $footer = New-HintFooter -Glyphs $g -Width $frameWidth -Plain:(-not $Color) -Hints @(
        @{ Token = 'w/s';   Label = 'move';     Clickable = $false }
        # Not clickable, for the reason the launch screen's own arrow hints are not: the token names
        # two directions and a click on it cannot mean one of them. MEASURED before it was added -
        # at 50, 80 and 100 columns the project footer wraps onto exactly the same number of lines
        # with it as without, so it costs the list no row anywhere. 'a/d' rather than the caps: the
        # caps are no longer drawn on the field, and the launch screen names the same pair of keys.
        @{ Token = 'a/d'; Label = 'action'; Clickable = $false }
        # 'run', not 'new', since the action field landed: Enter runs whatever the field says, and a
        # footer reading 'new' beside a field reading 'resume' advertises a key that does something
        # else - the one failure New-HintFooter's whole data-driven shape exists to prevent.
        @{ Token = 'enter'; Label = 'run';      Clickable = $true; Key = 'Enter';  Char = '' }
        @{ Token = 'c';     Label = 'continue'; Clickable = $true; Key = '';       Char = 'c' }
        @{ Token = 'r';     Label = 'resume';   Clickable = $true; Key = '';       Char = 'r' }
        @{ Token = 't';     Label = 'worktree'; Clickable = $true; Key = '';       Char = 't' }
        @{ Token = '/';     Label = 'filter';   Clickable = $true; Key = '';       Char = '/' }
        @{ Token = 'esc';   Label = 'back';     Clickable = $true; Key = 'Escape'; Char = '' }
    )
    $hov = Get-FooterHover -Footer $footer -Hover $Hover

    # Box top + box bottom + headroom + the footer's own lines, exactly like Get-PickerFrame, MINUS
    # the action field's own row. The field is drawn inside the box under the list, so the row it
    # costs comes out of the LIST's viewport - never out of $script:MinHeight, which is measured off
    # the launch frame alone (see the constant's own comment) and which this screen must keep
    # fitting under with a registry of any size.
    # MINUS the two separator lines as well (spec D6): the blank line under the cwd row and the one
    # above the free-path row are body lines the box has to hold, so they come out of the LIST's own
    # budget - never out of the frame's height, which must still fit $script:MinHeight with a
    # registry of any size.
    $bodyRows = [Math]::Max(3, $Height - 4 - 2 - @($footer.Lines).Count)
    if ($Index -ge $rows.Count) { $Index = [Math]::Max(0, $rows.Count - 1) }
    $vp = Get-Viewport -Count $rows.Count -Index $Index -Visible $bodyRows
    $inner = $frameWidth - 2

    # $rowYs: where each VISIBLE cursor row landed in $body. The separators below are blank body
    # LINES, not rows - nothing selects one, nothing is hit-tested onto one - so the row index can no
    # longer be derived from a y by arithmetic, and the map carries the actual list instead.
    $body = @()
    $rowYs = @()
    # Where a band goes on every visible row, for the memo: body index and how many characters of
    # the finished line it wraps. Recorded here because this loop is the only place that knows which
    # row is which - re-deriving it from the painted frame is the second copy this file keeps warning about.
    $rowBands = @{}
    for ($i = $vp.Start; $i -lt ($vp.Start + $vp.Visible); $i++) {
        $r = $rows[$i]
        # The blank line ABOVE the free-path row - skipped when that row opens the viewport, since a
        # box whose first body line is empty reads as a rendering fault rather than as a separator.
        if ($r.Kind -eq 'path' -and $body.Count -gt 0) { $body += '' }
        $mark = if ($i -eq $Index) { " $($g.Cursor) " } else { '   ' }
        $rowYs += $body.Count
        $rowBandY = $body.Count
        if ($r.Kind -eq 'project') {
            $age = Format-RelativeAge -From $r.Item.LastActivity -Now $Now
            $name = $r.Item.Name
            if ($r.Item.Worktree) { $name = "$($g.Worktree) $name" }
            $rowText = New-ListRow -Mark $mark -Label $name -Tail $r.Item.Path -Age $age -Width $inner -Ascii:$Ascii -DimTail -DimAge -PathTail -Hover:($i -eq $HoverRow)
            $rowBands[$i] = @{ Y = $rowBandY; Length = (Get-RowBandLength -Row $rowText -PaneWidth $inner) }
            $body += $rowText
        } elseif ($r.Kind -eq 'cwd') {
            # The reader must see which directory the row means - rendered like a project row's
            # name+path columns, minus the age no pinned row has a real LastActivity for.
            $rowText = New-ListRow -Mark $mark -Label $r.Item.Name -Tail $r.Item.Path -Width $inner -Ascii:$Ascii -DimTail -PathTail -Hover:($i -eq $HoverRow)
            $rowBands[$i] = @{ Y = $rowBandY; Length = (Get-RowBandLength -Row $rowText -PaneWidth $inner) }
            $body += $rowText
            # And the blank line UNDER it: the current directory is a group of its own, so the eye
            # stops there instead of reading it as the first entry of the registry (spec D6). Not
            # when the next visible row is the free-path row - it brings its own separator, and both
            # rules firing puts TWO blank lines in the box (reachable with any filter that matches
            # nothing, and with an empty registry).
            if ($i -lt ($vp.Start + $vp.Visible - 1) -and $rows[$i + 1].Kind -ne 'path') { $body += '' }
        } else {
            # The one row not built by New-ListRow, so it marks itself. Same rule as there: the open
            # marker goes in FRONT of the mark column, which is where Add-PickerColor's bullet
            # anchor expects to be able to step over it.
            $pathRow = $mark + $($g.Bullet) + ' ' + (Limit-Line -Text $r.Item.Name -Max ($inner - $mark.Length - 3))
            # Padded to the full inner width INSIDE the hover branch (P10), for the reason
            # New-ListRow's own gutter carries: every hovered row on this screen bands the same
            # width, and an unhovered frame keeps the exact bytes it had. This row is far shorter
            # than the list rows above it, so its band was the ragged one.
            if ($i -eq $HoverRow) { $pathRow = Add-HoverSpan -Text ($pathRow + (' ' * [Math]::Max(0, $inner - (Get-DisplayWidth -Text $pathRow)))) }
            $rowBands[$i] = @{ Y = $rowBandY; Length = (Get-RowBandLength -Row $pathRow -PaneWidth $inner) }
            $body += $pathRow
        }
    }

    # The two separator slots are RESERVED, not merely spent (R15): which of them is drawn depends on
    # where the viewport sits - both when the whole list fits, one at either end of a long list, none
    # mid-scroll - so without this the box bottom and the whole footer would jump a line the moment
    # the list scrolls past a separator. Padding to a height that does not depend on $Index is what
    # keeps the frame still while the list moves inside it.
    while ($body.Count -lt ($vp.Visible + 2)) { $body += '' }

    # The action field: one launch-screen-style RADIO row under the list, drawn by the same
    # New-RadioRow every launch row goes through, so the whole screen is driveable with the arrows
    # and Enter and no hotkey has to be memorised. Every value is on the row, marked and clickable -
    # nothing has to be discovered by pressing, and nothing on the row moves when the value changes
    # (spec D2). It carries no cursor: the arrows step it from every row.
    # -Current through the same canonicaliser the stepper uses, so what is DRAWN and what Right steps
    # from can never be two different strings (review W4: 'RESUME' rendered, then stepped from 'new').
    # -Hover is the raw hovered value: the action row is the only cell-bearing row on this screen
    # (the list rows carry no cells), so a HoverValue can have come from nowhere else and needs no
    # row check of its own - unlike the launch screen, where several rows offer the same value.
    $radio = New-RadioRow -Prefix '   ' -Label 'action' -Values (Get-ProjectActions) -Current (Step-ProjectAction -Action $Action -Delta 0) -Glyphs $g -LabelWidth 8 -MaxWidth $inner -Hover $HoverValue
    # Where the field actually lands in the body, taken before it is appended: with the separators in
    # the box the field is no longer $vp.Visible lines below the first row, and a Y computed that way
    # points at a blank line - a click on the field would do nothing and a click on a gap would step
    # it (controller ruling C7).
    $actionIndex = $body.Count
    $body += $radio.Text

    $lines = New-Box -Lines $body -Width $frameWidth -Title $title -Ascii:$Ascii
    # Derived from the counts, never hardcoded, for the reason the map below records - and taken out
    # here because the memo needs the same offset whether or not the caller asked for a map.
    $firstRowY = $lines.Count - $body.Count - 1
    if ($RowMap) {
        $RowMap.Value = [pscustomobject]@{
            # RowCount stays the LIST's own, so the field is never hit-tested as a project row
            # (Get-ClaudeMouseRow returns $null for it and the caller falls through to the field).
            FirstRowY = $firstRowY
            RowCount  = $vp.Visible
            Start     = $vp.Start
            # The frame-relative y of every visible cursor row, in row order. Get-HitAt prefers this
            # over the FirstRowY/RowCount arithmetic, which cannot see the separators and would read
            # a click under one as a row further down the list.
            RowYs     = [int[]]@($rowYs | ForEach-Object { $firstRowY + $_ })
            Action    = [pscustomobject]@{
                Y = $firstRowY + $actionIndex
                # +1 for the box's own left border, which New-Box puts in front of every body line:
                # these are FRAME columns, the coordinates a click arrives in.
                Cells = @($radio.Cells | ForEach-Object {
                    [pscustomobject]@{ Start = $_.Start + 1; End = $_.End + 1; Value = $_.Value }
                })
            }
        }
    }
    # The footer button naming the CURRENT action is lit even with the mouse elsewhere (spec D1):
    # Enter runs whatever the field says, so that button should never look idle. 'new' lights
    # nothing - there is no 'n' hotkey on this footer, only the enter/run button, and lighting
    # 'enter' here would light it for every action, not just new (W3).
    $selected = switch (Step-ProjectAction -Action $Action -Delta 0) {
        'continue' { @(@{ Key = ''; Char = 'c' }) }
        'resume'   { @(@{ Key = ''; Char = 'r' }) }
        'worktree' { @(@{ Key = ''; Char = 't' }) }
        default    { @() }
    }
    # +1 on every Start for the box's own left border, which New-Box puts in front of every body
    # line: these are FRAME columns, the same coordinates the row map's click cells are in.
    $memoRows = @{}
    foreach ($k in $rowBands.Keys) { $memoRows[$k] = @{ Y = $firstRowY + $rowBands[$k].Y; Start = 1; Length = $rowBands[$k].Length } }
    $memoAction = @{ Y = $firstRowY + $actionIndex; Values = @{} }
    foreach ($ch in @($radio.Chars)) { $memoAction.Values[$ch.Value] = @{ Start = $ch.Start + 1; Length = $ch.Length } }
    return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $frameWidth -Glyphs $g -Color:$Color -RowMap $RowMap -HasHover:$hov.HasHover -HoverKey $hov.Key -HoverChar $hov.Char -Selected $selected `
                                 -Memo @{ Builder = 'project'; Key = $memoKey; HoverRow = $HoverRow; HoverValue = $HoverValue; Hover = $Hover; Rows = $memoRows; Action = $memoAction })
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
    #
    # Read as an INT. PromptCount used to be the string "N+" past the counter's byte budget, and
    # PowerShell coerces the other operand to the left one's type - so '0+' -gt 0 was TRUE and a
    # >4 MB transcript whose first 4 MB holds nothing a human typed was offered as resumable
    # (adversarial review 2026-09-16, E4b). The trailing '+' is still stripped so a cache written by
    # an older build stays readable.
    param([Parameter(Mandatory)][AllowEmptyCollection()][array]$Sessions)
    return @($Sessions | Where-Object { (Get-PromptCountValue -Session $_) -gt 0 })
}

function Get-PromptCountValue {
    # The prompt count as an int, whatever shape the row carries it in.
    param($Session)
    $n = 0
    [void][int]::TryParse(("$($Session.PromptCount)" -replace '\+$', ''), [ref]$n)
    return $n
}

function Format-PromptCount {
    # What the picker shows: the number, plus the '+' that says the transcript ran past the
    # counter's byte budget. The marker is rendered from the FLAG, never stored in the number.
    param($Session)
    $capped = if ($null -ne $Session.PSObject.Properties['PromptCountCapped']) { [bool]$Session.PromptCountCapped }
              else { "$($Session.PromptCount)".EndsWith('+') }
    $n = Get-PromptCountValue -Session $Session
    if ($capped) { return "$n+" }
    return "$n"
}

function Select-SessionMatch {
    # -like reads '[' as the start of a character class, and an unmatched one is a TERMINATING
    # WildcardPatternException that escapes Where-Object into the render loop - the same defect
    # Select-ProjectMatch (Projects.ps1) already carries the fix for. Escaping the filter text is
    # the fix: someone who typed '[' is looking for a literal '[', and Escape gives them that.
    param([Parameter(Mandatory)][AllowEmptyCollection()][array]$Sessions, [string]$Filter)
    # @() for the same reason as Select-ProjectMatch: an unfiltered one-element answer must not
    # unroll to a bare object where the filtered answer is an array.
    if ([string]::IsNullOrWhiteSpace($Filter)) { return @($Sessions) }
    $f = [Management.Automation.WildcardPattern]::Escape($Filter.Trim())
    return @($Sessions | Where-Object {
        "$($_.Project) $($_.Worktree) $($_.Title) $($_.LastUser) $($_.LastAssistant)" -like "*$f*"
    })
}

$script:PickerRx = @{}

function Get-PickerPatterns {
    # Add-PickerColor's patterns, built ONCE per glyph set. Every one of them is a constant for a
    # given set of glyphs, and building them per call put a pattern parse on every body line of
    # every frame - the painter is called once per line, and a frame is fifty of them.
    # IgnoreCase + CultureInvariant are the options PowerShell's own -replace applies: compiled
    # without them, text differing only in case would stop matching and the frame would change.
    param([hashtable]$Glyphs)
    $key = "$($Glyphs.Cursor)$($Glyphs.RAngle)$($Glyphs.Bullet)$($Glyphs.Worktree)"
    if ($script:PickerRx[$key]) { return $script:PickerRx[$key] }
    $opt = [Text.RegularExpressions.RegexOptions]::IgnoreCase -bor [Text.RegularExpressions.RegexOptions]::CultureInvariant
    $rA = [regex]::Escape($Glyphs.RAngle)
    $markPattern = "$([string]$script:HoverOpen)?(?:   | $([regex]::Escape([string]$Glyphs.Cursor)) )"
    $p = [pscustomobject]@{
        Nothing  = [regex]::new('nothing matches', $opt)
        You      = [regex]::new("(^|\s)you $rA ", $opt)
        Claude   = [regex]::new("(^|\s)claude $rA ", $opt)
        Bullet   = [regex]::new("(?<=^.$markPattern)($([regex]::Escape([string]$Glyphs.Bullet)))(?= enter a path)", $opt)
        Cursor   = [regex]::new("([$($Glyphs.Cursor)])", $opt)
        Worktree = [regex]::new("([$($Glyphs.Worktree)])", $opt)
        Age      = [regex]::new('(\d+ (?:min|h|d)|now)$', $opt)
        Msgs     = [regex]::new('(\d+ msgs)', $opt)
    }
    $script:PickerRx[$key] = $p
    return $p
}

function Add-PickerColor {
    param([string]$Line, [switch]$Enabled, [hashtable]$Glyphs)
    if (-not $Enabled -or -not $Line) { return $Line }
    $c = $script:C
    $rx = Get-PickerPatterns -Glyphs $Glyphs
    if ($rx.Nothing.IsMatch($Line)) { return $c.Dim + $Line + $c.Reset }
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
    # U+0003, not U+0001: U+0001 is $script:DimOpen, and a mark that collides with a dim-span marker
    # only works while Add-DimSpanColor happens to run first.
    $markUser = [string][char]3 + 'u' + [string][char]3
    $markClaude = [string][char]3 + 'a' + [string][char]3
    $out = $rx.You.Replace($out, ('$1' + $markUser))
    $out = $rx.Claude.Replace($out, ('$1' + $markClaude))

    # The bullet, ANCHORED to the row it belongs to and painted BEFORE the cursor rule (R13). In
    # ASCII mode Bullet is '+' - the same character as all four box corners - so a bare `([+])` rule
    # painted every corner of every frame magenta, and a project named 'c++' with it. The free-path
    # row is the only place a bullet is drawn: one box border, the 3-cell mark ('   ' or
    # ' <cursor> '), the bullet, then its own label. The label is in the pattern because the shape
    # alone is not unique - a project NAMED '+ something' sits in the identical columns - so this
    # rule is deliberately coupled to the text Get-ProjectFrame gives that row. Before the cursor
    # rule, because that rule paints the mark itself and its escapes would break this lookbehind.
    # The optional hover-open marker between the border and the mark column is what lets a HOVERED
    # free-path row keep its bullet: the band wraps the whole row, so its open marker sits in front
    # of the 3-cell mark (spec D10 puts it there for exactly this lookbehind) and an anchor that
    # could not step over it would leave the bullet unpainted on the one row the mouse is on.
    $out = $rx.Bullet.Replace($out, ($c.Magenta + '$1' + $c.Reset))
    $out = $rx.Cursor.Replace($out, ($c.BrightYellow + '$1' + $c.Reset))
    $out = $rx.Worktree.Replace($out, ($c.Yellow + '$1' + $c.Reset))
    $out = $rx.Age.Replace($out, ($c.Dim + '$1' + $c.Reset))
    $out = $rx.Msgs.Replace($out, ($c.Dim + '$1' + $c.Reset))

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
        # 'none' | 'project' | 'all'. 'none' is the picker's original, pre-Task-8 shape exactly -
        # no tab hint, no title suffix, project column shown - for the caller that has no project
        # context at all (fix round 1, IMPORTANT 1: a hint that always does nothing, because the
        # launcher never had a slug or name to scope by in the first place, is dead weight that
        # costs a wrapped footer line at 80 columns for every user, not a feature). 'project': the
        # caller has already narrowed -Sessions to the scoped pool - every row here is then the same
        # project, so its own name is dropped from the list column (pure noise) - and the tab hint
        # offers to widen. 'all': every session is shown and the tab hint offers to narrow back.
        [ValidateSet('none', 'project', 'all')]
        [string]$Scope = 'none',
        # Display label for the title under -Scope project ONLY (fix round 1, IMPORTANT 3) - falls
        # back to the generic "this project" when the caller knows a slug but not a display name.
        [string]$ProjectName = '',
        # -Hover names a clickable footer-button INDEX, -1 for none - the same contract
        # Get-ProjectFrame and Get-LaunchFrame carry (spec D1/D3). Without it this screen kept the
        # hover state and repainted a byte-identical frame for every footer-button crossing.
        [int]$Hover = -1,
        # -HoverRow is the session row under the pointer (spec D10), a LIST index like -Index and -1
        # for none. It paints and nothing else: the cursor and the preview follow -Index, which only
        # a click or a key moves (spec D8): a row highlighted under the mouse, and a preview that
        # stays locked on whatever was last chosen.
        [int]$HoverRow = -1,
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
    $frameWidth = Get-FrameWidth -Width $Width
    # The memo (spec D11): every input EXCEPT the two hover ones. Same shape and the same
    # minute-granularity -Now as Get-ProjectFrame's - see the comment there.
    $memoKey = "$Width|$Height|$Color|$Ascii|$Index|$Filter|$Scope|$ProjectName|" + $Now.ToString('yyyyMMddHHmm') + '|' +
               (Get-SessionListSignature -Items $Sessions)
    $memoHit = Get-MemoFrame -Builder 'picker' -Key $memoKey -HoverRow $HoverRow -Hover $Hover -RowMap $RowMap
    if ($null -ne $memoHit) { return $memoHit }
    # A session nobody typed anything into is not offered - see Select-ResumableSessions. The count
    # is stated rather than the list just quietly getting shorter: dropping rows silently reads as
    # "these are all your sessions" when 16 of 40 were never a real conversation.
    $resumable = @(Select-ResumableSessions -Sessions $Sessions)
    $hiddenCount = @($Sessions).Count - $resumable.Count
    $items = @(Select-SessionMatch -Sessions $resumable -Filter $Filter)
    # Fix round 2, item 4: a filter can shrink $items below whatever -Index the caller passed - the
    # loop in Invoke-SessionPicker always re-clamps its own $index before drawing, so production
    # never hits this, but a caller that renders a frame directly (a test, a future screen) does not
    # get that shield for free. Same clamp Get-ProjectFrame already carries for its own rows.
    if ($Index -ge $items.Count) { $Index = [Math]::Max(0, $items.Count - 1) }

    $title = "resume $($g.H) $($items.Count) sessions"
    # IMPORTANT 3: the comment on Invoke-SessionPicker's -ProjectName always said this was the
    # display label - it never actually reached the title until now. Falls back to the generic
    # phrase only when the caller knows a slug but never learned a display name for it.
    if ($Scope -eq 'project') { $title += " $($g.H) " + $(if ($ProjectName) { $ProjectName } else { 'this project' }) }
    if ($hiddenCount -gt 0) { $title += " $($g.H) $hiddenCount empty hidden" }
    if ($Filter) { $title += " $($g.H) filter: $Filter" }
    # Same rule as the launch screen: the arrow hint names two directions and cannot be clicked
    # into one of them; every single action can. w/s, not up/down (2026-09-09) - see Get-LaunchFrame.
    # The tab hint's label names the scope a press LANDS ON, not the one showing now - the same
    # convention Tab uses everywhere else in this codebase (a toggle names its destination).
    # -Scope none gets NO tab hint at all (IMPORTANT 1): it would be permanently dead (Tab does
    # nothing without a project to scope by - Invoke-SessionPicker never even reaches the Tab
    # branch) and costs a wrapped footer line at 80 columns for a click that can never do anything.
    $hints = @(
        @{ Token = 'w/s'; Label = 'move';   Clickable = $false }
        @{ Token = '/';       Label = 'filter'; Clickable = $true; Key = ''; Char = '/' }
        @{ Token = 'enter';   Label = 'open';   Clickable = $true; Key = 'Enter'; Char = '' }
        @{ Token = 'f';       Label = 'fork';   Clickable = $true; Key = ''; Char = 'f' }
    )
    if ($Scope -ne 'none') {
        $hints += @{ Token = 'tab'; Label = $(if ($Scope -eq 'project') { 'all projects' } else { 'this project' }); Clickable = $true; Key = 'Tab'; Char = '' }
    }
    $hints += @{ Token = 'esc'; Label = 'back'; Clickable = $true; Key = 'Escape'; Char = '' }
    $footer = New-HintFooter -Glyphs $g -Width $frameWidth -Plain:(-not $Color) -Hints $hints
    # Resolved once, before the three returns below: the empty-list branch is the one that gets
    # forgotten when the footer changes, which is why Complete-PickerFrame exists at all.
    $hov = Get-FooterHover -Footer $footer -Hover $Hover

    if ($items.Count -eq 0) {
        $emptyMsg =
            if ($Filter) { '  nothing matches that filter' }
            elseif ($hiddenCount -gt 0) { "  all $hiddenCount sessions here are empty - nothing to resume" }
            else { '  no sessions found' }
        $lines = New-Box -Lines @('', $emptyMsg, '') -Width $frameWidth -Title $title -Ascii:$Ascii
        # Memoised like the other two branches, with no rows to band: a footer button is still
        # hoverable over an empty list, and that is exactly one footer repaint.
        return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $frameWidth -Glyphs $g -Color:$Color -RowMap $RowMap -HasHover:$hov.HasHover -HoverKey $hov.Key -HoverChar $hov.Char `
                                     -Memo @{ Builder = 'picker'; Key = $memoKey; HoverRow = $HoverRow; HoverValue = ''; Hover = $Hover; Rows = @{}; Action = $null })
    }

    # Box top + box bottom + one headroom row + the footer's lines (one on a wide terminal, more
    # when it wraps). Everything below budgets against this, so no branch can produce a frame
    # taller than the terminal - which scrolls the top away and desynchronises every later redraw.
    # The headroom row is the one Write-Frame's trailing newline needs (see Get-MaintenanceFrame);
    # until 2026-09-02 this budget lacked it and a full list filled the terminal exactly.
    $bodyRows = [Math]::Max(3, $Height - 3 - @($footer.Lines).Count)
    # Layout breakpoint about the TERMINAL width, not the drawing budget - stays on $Width by design.
    $wide = $Width -ge $script:TwoPaneWidth

    $leftWidth = if ($wide) { [Math]::Max(28, [int](($frameWidth - 2) * 0.4)) } else { $frameWidth - 2 }
    $rightWidth = ($frameWidth - 2) - $leftWidth - 1

    # Shared by both list loops below (narrow and wide) so the label/where/what computation is not
    # duplicated a third time. A nested function, not a scriptblock, so it needs no .GetNewClosure()
    # to see $Scope/$Now/$g/$Ascii/$leftWidth/$Index from Get-PickerFrame's own scope.
    function New-SessionRow {
        param($Item, $RowIndex)
        $mark = if ($RowIndex -eq $Index) { " $($g.Cursor) " } else { '   ' }
        # Under -Scope project every row IS the same project - naming it on each one is pure
        # noise, so it is dropped here and the room it frees goes to the snippet below.
        $where = if ($Scope -eq 'project') { '' } else { $Item.Project }
        if ($Item.Worktree) { $where = "$($g.Worktree) $($Item.Worktree)" }
        $age = Format-RelativeAge -From $Item.Modified -Now $Now
        $what = if ($Item.LastUser) { $Item.LastUser } elseif ($Item.Title) { $Item.Title } else { '' }
        # The project alone does not identify a session: on this machine twelve consecutive rows
        # are all the same repository. The snippet is what makes the list scannable.
        $room = $leftWidth - $mark.Length - $age.Length - 2
        $label = $where
        # Minor (fix round 1): with $where dropped to '' under -Scope project, the old fixed
        # '  ' separator left the label starting with two dead spaces nobody could read anything
        # into. Built from only the non-empty parts, and the freed width goes to the snippet -
        # reclaiming, not just hiding, the columns -Scope project frees.
        if ($what -and $room -gt ($where.Length + 4)) {
            $sep = if ($where) { '  ' } else { '' }
            $label = $where + $sep + (Limit-Line -Text $what -Max ($room - $where.Length - $sep.Length))
        }
        return (New-ListRow -Mark $mark -Label $label -Age $age -Width $leftWidth -Ascii:$Ascii -TrailingSpace -Hover:($RowIndex -eq $HoverRow))
    }

    if (-not $wide) {
        # 6 rows reserved for the preview, BEFORE the viewport is sized: one header (project,
        # worktree, message count, date - the columns the narrow list has no room for), one
        # separator, two for the question, two for the answer.
        $previewRows = 6
        $listRows = [Math]::Max(1, $bodyRows - $previewRows)
        $vp = Get-Viewport -Count $items.Count -Index $Index -Visible $listRows

        $list = @()
        # Where a band goes on each visible row, for the memo - see Get-ProjectFrame's own $rowBands.
        $rowBands = @{}
        for ($i = $vp.Start; $i -lt ($vp.Start + $vp.Visible); $i++) {
            $rowText = New-SessionRow -Item $items[$i] -RowIndex $i
            $rowBands[$i] = @{ Y = ($i - $vp.Start); Length = (Get-RowBandLength -Row $rowText -PaneWidth $leftWidth) }
            $list += $rowText
        }

        # Single column: the selected session's preview goes underneath the list.
        $s = $items[$Index]
        $head = "$($s.Project)"
        if ($s.Worktree) { $head += "  $($g.Worktree) $($s.Worktree)" }
        $head += "  $($g.H)  $(Format-PromptCount -Session $s) msgs  $($g.H)  $($s.Modified.ToString('dd MMM HH:mm'))"
        $body = @($list)
        $body += '  ' + (Limit-Line -Text $head -Max ($frameWidth - 4))
        $body += '  ' + ([string]$g.H * ($frameWidth - 6))
        # $previewRows (6, fixed above) minus the two header lines just added is what is left for
        # the exchange itself - same total budget as before this change, now spent on as many
        # attributed messages as fit rather than a hardcoded one question, one answer.
        $exchangeBudget = [Math]::Max(1, $previewRows - 2)
        foreach ($l in (Get-ExchangeLines -Messages (Get-SessionExchange -Session $s) -Width ($frameWidth - 4) -MaxLines $exchangeBudget -Glyphs $g)) {
            $body += '  ' + $l
        }
        $lines = New-Box -Lines $body -Width $frameWidth -Title $title -Ascii:$Ascii
        # The list is the first $vp.Visible entries of $body, and New-Box wraps $body between a top
        # and a bottom border. Deriving the offset from the counts rather than hardcoding 1 means a
        # future change to the box cannot silently move every row by one.
        $firstRowY = $lines.Count - $body.Count - 1
        if ($RowMap) {
            $RowMap.Value = [pscustomobject]@{
                FirstRowY = $firstRowY
                RowCount  = $vp.Visible
                Start     = $vp.Start
            }
        }
        # +1 on Start for the box's own left border - the band covers the row, which in this layout
        # is the whole inner width.
        $memoRows = @{}
        foreach ($k in $rowBands.Keys) { $memoRows[$k] = @{ Y = $firstRowY + $rowBands[$k].Y; Start = 1; Length = $rowBands[$k].Length } }
        return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $frameWidth -Glyphs $g -Color:$Color -RowMap $RowMap -HasHover:$hov.HasHover -HoverKey $hov.Key -HoverChar $hov.Char `
                                     -Memo @{ Builder = 'picker'; Key = $memoKey; HoverRow = $HoverRow; HoverValue = ''; Hover = $Hover; Rows = $memoRows; Action = $null })
    }

    $vp = Get-Viewport -Count $items.Count -Index $Index -Visible $bodyRows
    $list = @()
    $rowBands = @{}
    for ($i = $vp.Start; $i -lt ($vp.Start + $vp.Visible); $i++) {
        $rowText = New-SessionRow -Item $items[$i] -RowIndex $i
        # The band covers the LEFT PANE only here, not the whole line: the right pane is the preview
        # and nothing hovers it.
        $rowBands[$i] = @{ Y = ($i - $vp.Start); Length = (Get-RowBandLength -Row $rowText -PaneWidth $leftWidth) }
        $list += $rowText
    }

    $s = $items[$Index]
    $where = $s.Project
    if ($s.Worktree) { $where += "  $($g.Worktree) $($s.Worktree)" }

    $detail = @()
    $detail += ' ' + (Limit-Line -Text $where -Max ($rightWidth - 2))
    $detail += ' ' + $s.Modified.ToString('dd MMM HH:mm') + "  $($g.H)  $(Format-PromptCount -Session $s) msgs  $($g.H)  $('{0:N0}' -f ($s.SizeBytes / 1KB)) KB"
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
    $lines = New-Box -Lines $body -Width $frameWidth -Title $title -Ascii:$Ascii
    # Same derivation as the narrow branch. Join-Panes can make the body TALLER than the list when
    # the detail pane is longer, so the row count is the viewport's, never the body's.
    $firstRowY = $lines.Count - $body.Count - 1
    if ($RowMap) {
        $RowMap.Value = [pscustomobject]@{
            FirstRowY = $firstRowY
            RowCount  = $vp.Visible
            Start     = $vp.Start
        }
    }
    $memoRows = @{}
    foreach ($k in $rowBands.Keys) { $memoRows[$k] = @{ Y = $firstRowY + $rowBands[$k].Y; Start = 1; Length = $rowBands[$k].Length } }
    return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $frameWidth -Glyphs $g -Color:$Color -RowMap $RowMap -HasHover:$hov.HasHover -HoverKey $hov.Key -HoverChar $hov.Char `
                                 -Memo @{ Builder = 'picker'; Key = $memoKey; HoverRow = $HoverRow; HoverValue = ''; Hover = $Hover; Rows = $memoRows; Action = $null })
}

function Get-MaintenanceFrame {
    # -Hover names a clickable footer-button INDEX, -1 for none - the same contract the other three
    # frames carry (spec D1/D3). Every hint on this screen is a button, so it is the screen where an
    # unlit hover is most obviously missing.
    param($Info, [int]$Width = 78, [int]$Height = 24, [string]$Status = '', [switch]$Color, [switch]$Ascii, [ref]$RowMap, [object[]]$Actions = @(),
          [int]$Hover = -1)
    if ($RowMap) { $RowMap.Value = [pscustomobject]@{ Rows = @() } }
    if ($Width -lt $script:MinWidth -or $Height -lt $script:MinHeight) {
        return (Get-TooSmallFrame -Width $Width -Height $Height)
    }
    $g = Get-Glyphs -Ascii:$Ascii
    $frameWidth = Get-FrameWidth -Width $Width
    $boxWidth = [Math]::Min($frameWidth, 100)
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
    $footer = New-HintFooter -Glyphs $g -Width $frameWidth -Plain:(-not $Color) -Hints $hints
    $hov = Get-FooterHover -Footer $footer -Hover $Hover

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
    return (Complete-PickerFrame -Lines $lines -Footer $footer -Width $frameWidth -Glyphs $g -Color:$Color -RowMap $RowMap -HasHover:$hov.HasHover -HoverKey $hov.Key -HoverChar $hov.Char)
}
