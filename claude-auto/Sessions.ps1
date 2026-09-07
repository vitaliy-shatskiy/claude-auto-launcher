# Pure transcript reading for the claude-auto launcher. No console output and no side effects
# beyond the on-disk cache, so every function here is directly assertable.

# Wrapper text Claude Code injects around real prompts. Without this filter almost every
# session's title comes out as "/clear": measured on this machine, the first four user records
# of a fresh session were a caveat block, a slash command, a skill invocation and a tool result.
$script:NoisePrefixes = @(
    '<local-command-caveat>', '<command-name>', '<command-message>',
    '<command-args>', '<local-command-stdout>', '<system-reminder>',
    '<task-notification>'
)

# Injected wrappers keep being invented; the suffix rule catches the next one without waiting for it
# to leak into a preview. Deliberately narrow: a prompt that legitimately opens with '<' survives
# unless its tag name ends in one of these.
$script:NoiseTagPattern = '^<[a-z][a-z0-9-]*(-notification|-hook|-reminder|-caveat|-stdout)>'

# Resolved at load time so the per-file loop does not stat this module 40 times. A failure falls
# back to a constant rather than throwing: a cache that never invalidates is bad, a launcher that
# will not start is worse.
$script:SummaryVersion = try { (Get-Item -LiteralPath $PSCommandPath).LastWriteTimeUtc.Ticks } catch { 'v2' }

function Get-ClaudeRecordText {
    # Content is either a plain string or an array of blocks; only 'text' blocks carry anything
    # a human wrote or read. 'thinking' and 'tool_use' are deliberately dropped.
    param($Content)
    if ($null -eq $Content) { return $null }
    if ($Content -is [string]) { return $Content }
    $text = @($Content | Where-Object { $_.type -eq 'text' } | ForEach-Object { $_.text })
    if ($text.Count -eq 0) { return $null }
    return ($text -join ' ')
}

function Get-ClaudeUserPrompt {
    # The text a human actually typed, or $null when the record is machinery.
    param($Record)
    if ($Record.type -ne 'user') { return $null }
    if ($Record.isSidechain) { return $null }        # subagent traffic, not this conversation

    $content = $Record.message.content
    if ($content -isnot [string]) {
        # An array carrying a tool_result is a tool answer wearing the user role.
        if (@($content | Where-Object { $_.type -eq 'tool_result' }).Count -gt 0) { return $null }
    }
    $text = Get-ClaudeRecordText -Content $content
    if ([string]::IsNullOrWhiteSpace($text)) { return $null }
    $trimmed = $text.TrimStart()

    # A slash command arrives wrapped in tags, and the words the user typed sit inside
    # <command-args>. Discarding the whole record loses the only description such a session has: a
    # transcript can open with /doctor and carry its entire human content in that argument.
    # Bare commands like /clear carry empty args and stay noise.
    if ($trimmed.StartsWith('<command-message>') -or $trimmed.StartsWith('<command-name>')) {
        $name = [regex]::Match($trimmed, '<command-name>([^<]*)</command-name>').Groups[1].Value.Trim()
        $argText = [regex]::Match($trimmed, '<command-args>([^<]*)</command-args>').Groups[1].Value.Trim()
        if ($argText) { return ("$name $argText").Trim() }
        return $null
    }

    if ($Record.isMeta) { return $null }
    foreach ($p in $script:NoisePrefixes) {
        if ($trimmed.StartsWith($p)) { return $null }
    }
    if ($trimmed -match $script:NoiseTagPattern) { return $null }
    return $text
}

function Test-ClaudeUsableUserRecord {
    param($Record)
    return $null -ne (Get-ClaudeUserPrompt -Record $Record)
}

function Get-CodeFenceCollapsedText {
    # Triple-backtick blocks are usually pasted terminal output, diffs or scripts - wrapped into a
    # narrow detail pane they are unreadable noise that crowds out the actual conversation. Must run
    # here, before the RecentMessages $clean step collapses every newline to a single space: by the
    # time text reaches the picker no real line breaks survive, and a marker built from that could
    # not report a genuine line count. Inline single backticks are left untouched - they read fine at
    # this width and are not "code blocks" in the sense the owner means.
    param([string]$Text)
    if (-not $Text -or $Text -notmatch '```') { return $Text }
    return [regex]::Replace($Text, '(?s)```[^\n]*\r?\n(.*?)```', {
        param($m)
        $body = $m.Groups[1].Value.TrimEnd("`r`n".ToCharArray())
        $n = if ($body -ne '') { @($body -split "`r?`n").Count } else { 0 }
        "[code: $n lines]"
    })
}

function ConvertFrom-ClaudeProjectSlug {
    # 'C--Users-someone-Desktop-Projects-Foo'                       -> Foo
    # 'C--Users-someone-Desktop-Projects-Foo--claude-worktrees-bar' -> Foo, worktree bar
    param([Parameter(Mandatory)][string]$Slug)
    $worktree = $null
    $main = $Slug
    $marker = '--claude-worktrees-'
    $i = $Slug.IndexOf($marker)
    if ($i -ge 0) {
        $main = $Slug.Substring(0, $i)
        $worktree = $Slug.Substring($i + $marker.Length)
    }
    # Folder names contain dashes of their own and the slug flattens them, so recover the tail
    # after the last known path segment instead of guessing where the name starts.
    $project = $main
    $known = 'Projects-'
    $j = $main.IndexOf($known)
    if ($j -ge 0) { $project = $main.Substring($j + $known.Length) }
    # @() is load-bearing: a single-element pipeline is a STRING here, and [-1] on a string indexes
    # its last CHARACTER. That is how the slug 'subagents' rendered as the project 's'.
    else { $project = @($main -split '-' | Where-Object { $_ })[-1] }
    return [pscustomobject]@{ Project = $project; Worktree = $worktree }
}

function ConvertFrom-JsonlLine {
    param([string]$Line)
    if ([string]::IsNullOrWhiteSpace($Line)) { return $null }
    try { return $Line | ConvertFrom-Json -ErrorAction Stop } catch { return $null }
}

function Get-FileTailLines {
    # Replaces `Get-Content -Tail`, which is pathological on these transcripts. Measured on this
    # machine 2026-08-15 over the 40 sessions the picker summarises: -Tail cost 6.1 s of the
    # 7.05 s total, ~152 ms per file average. The cost is NOT proportional to file size - one
    # 6.24 MB session whose longest line is 257 KB cost ~3.0 s on its own (reproduced 3x), while
    # three neighbouring files of 6-6.6 MB cost 83-94 ms each. Reading that same 6.24 MB file
    # end to end with a StreamReader took 24 ms, i.e. -Tail was ~125x slower than reading
    # everything by hand.
    #
    # Walking backwards in byte chunks keeps the cost proportional to the TAIL rather than to the
    # file, which matters because the largest transcript on this machine is 112 MB. Counting 0x0A
    # on RAW BYTES is safe regardless of chunk boundaries: UTF-8 continuation bytes are all >= 0x80,
    # so a newline can never be half of a multi-byte character.
    param(
        [Parameter(Mandatory)][string]$Path,
        [int]$Count = 120,
        [int]$ChunkSize = 65536
    )
    $stream = $null
    try {
        # ReadWrite sharing: a live session is appending to its own transcript while the picker
        # reads it, and an exclusive open would fail on exactly the session the owner wants back.
        $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
        if ($stream.Length -eq 0) { return @() }

        $chunks = [System.Collections.Generic.List[byte[]]]::new()
        $newlines = 0
        $pos = $stream.Length
        while ($pos -gt 0 -and $newlines -le $Count) {
            $take = [Math]::Min($ChunkSize, $pos)
            $pos -= $take
            $null = $stream.Seek($pos, [System.IO.SeekOrigin]::Begin)
            $buf = [byte[]]::new($take)
            $got = $stream.Read($buf, 0, $take)
            if ($got -lt $take) { $buf = $buf[0..($got - 1)] }
            $chunks.Insert(0, $buf)
            # IndexOf is a native scan; a per-byte PowerShell loop here would cost more than the
            # -Tail call this function exists to replace.
            $at = 0
            while ($at -ge 0 -and $at -lt $buf.Length) {
                $at = [Array]::IndexOf($buf, [byte]10, $at)
                if ($at -lt 0) { break }
                $newlines++
                $at++
            }
        }

        $total = 0
        foreach ($c in $chunks) { $total += $c.Length }
        $all = [byte[]]::new($total)
        $off = 0
        foreach ($c in $chunks) { [Array]::Copy($c, 0, $all, $off, $c.Length); $off += $c.Length }

        $lines = @([System.Text.Encoding]::UTF8.GetString($all) -split "`n")
        # Unless the walk reached byte 0, the first element is whatever the chunk boundary cut in
        # half - a truncated JSON line that would parse as nothing anyway.
        if ($pos -gt 0 -and $lines.Count -gt 1) { $lines = @($lines[1..($lines.Count - 1)]) }
        $lines = @($lines | ForEach-Object { $_.TrimEnd([char]13) })
        while ($lines.Count -gt 0 -and [string]::IsNullOrWhiteSpace($lines[-1])) {
            $lines = @($lines[0..($lines.Count - 2)])
        }
        if ($lines.Count -gt $Count) { $lines = @($lines[($lines.Count - $Count)..($lines.Count - 1)]) }
        return @($lines)
    } catch {
        return @()   # an unreadable transcript degrades to "no preview", never to a broken picker
    } finally {
        if ($stream) { $stream.Dispose() }
    }
}

function Get-ClaudeSessionSummary {
    param(
        [Parameter(Mandatory)][string]$Path,
        # 60 was too small: a session that opens with a skill injection buries its first real
        # prompt hundreds of records deep, and five of the eight newest transcripts on this
        # machine fell back to the session id. Get-Content stops at the limit, so a high value
        # costs nothing on short files.
        [int]$HeadLines = 400,
        [int]$TailLines = 120
    )
    $file = Get-Item -LiteralPath $Path
    $slug = Split-Path (Split-Path $Path -Parent) -Leaf
    $names = ConvertFrom-ClaudeProjectSlug -Slug $slug

    $title = $null
    foreach ($line in (Get-Content -LiteralPath $Path -TotalCount $HeadLines -ErrorAction SilentlyContinue)) {
        $rec = ConvertFrom-JsonlLine -Line $line
        if (-not $rec) { continue }
        $prompt = Get-ClaudeUserPrompt -Record $rec
        if ($prompt) { $title = $prompt; break }
    }
    $lastUser = $null; $lastAssistant = $null
    # Recent messages for the picker's detail pane, collected newest-first while walking the tail
    # backwards (cheap - the same walk that already finds LastUser/LastAssistant), then reversed
    # once at the end. Capped well above what any pane height can show, so the render side decides
    # how many actually fit rather than this reader guessing a screen size it does not know.
    $maxRecent = 12
    $recent = @()
    $tail = @(Get-FileTailLines -Path $Path -Count $TailLines)
    for ($i = $tail.Count - 1; $i -ge 0; $i--) {
        $rec = ConvertFrom-JsonlLine -Line $tail[$i]
        if (-not $rec) { continue }
        if (-not $lastAssistant -and $rec.type -eq 'assistant' -and -not $rec.isSidechain) {
            $lastAssistant = Get-ClaudeRecordText -Content $rec.message.content
        }
        if (-not $lastUser) { $lastUser = Get-ClaudeUserPrompt -Record $rec }
        if ($recent.Count -lt $maxRecent) {
            if ($rec.type -eq 'assistant' -and -not $rec.isSidechain) {
                $t = Get-ClaudeRecordText -Content $rec.message.content
                if ($t) { $recent += [pscustomobject]@{ Speaker = 'assistant'; Text = $t } }
            } elseif ($rec.type -eq 'user') {
                $t = Get-ClaudeUserPrompt -Record $rec
                if ($t) { $recent += [pscustomobject]@{ Speaker = 'user'; Text = $t } }
            }
        }
        if ($lastUser -and $lastAssistant -and $recent.Count -ge $maxRecent) { break }
    }
    [array]::Reverse($recent)

    # Fall back to the last prompt before the session id: "c47c2bc1" identifies nothing, whereas
    # the most recent thing asked usually does.
    if (-not $title) { $title = $lastUser }
    # Sessions that were opened, cleared and abandoned really do contain no prompt. Say so:
    # a bare hex id in the title column reads like the parser failed.
    if (-not $title) { $title = "(no prompt) " + $file.BaseName.Substring(0, [Math]::Min(8, $file.BaseName.Length)) }

    $clean = { param($s) if ($s) { ($s -replace '\s+', ' ').Trim() } else { '' } }
    return [pscustomobject]@{
        SessionId       = $file.BaseName
        Path            = $file.FullName
        Project         = $names.Project
        Worktree        = $names.Worktree
        Modified        = $file.LastWriteTime
        SizeBytes       = $file.Length
        PromptCount     = (Measure-ClaudePrompts -Path $Path)
        Title           = (& $clean $title)
        LastUser        = (& $clean $lastUser)
        LastAssistant   = (& $clean $lastAssistant)
        RecentMessages  = @($recent | ForEach-Object { [pscustomobject]@{ Speaker = $_.Speaker; Text = (& $clean (Get-CodeFenceCollapsedText -Text $_.Text)) } })
    }
}

function Get-SessionsRootForAccount {
    # deferred review finding: the session picker used to call Get-ClaudeSessions with no root at
    # all, which defaults to the CANONICAL account's ~/.claude/projects regardless of which account
    # is actually selected. With sharing off (the shipped default) that lists the wrong account's
    # sessions, and a chosen one then resumes into a CLAUDE_CONFIG_DIR that never held it.
    # Extracted so the resolution itself is assertable without a console.
    param([Parameter(Mandatory)][string]$Account, [Parameter(Mandatory)][System.Collections.IDictionary]$ProfileRoots)
    if (-not $ProfileRoots.Contains($Account)) { throw "unknown account '$Account' (roster: $($ProfileRoots.Keys -join ', '))" }
    return (Join-Path $ProfileRoots[$Account] 'projects')
}

function Get-ClaudeSessions {
    # Newest $Limit transcripts, summarised. Over 2000 files and more than a gigabyte live under
    # the projects root, so the sort touches filesystem metadata only and just the survivors are
    # ever opened.
    #
    # CachePath defaults from ProjectsRoot (not a single hardcoded ~/.claude path): the cache key
    # already carries each session's own full path, so correctness never depended on this, but the
    # whole cache FILE is overwritten on every call (see the Set-Content at the end) - a launcher
    # with sharing off and several accounts thrashed one shared cache file on every account switch
    # instead of keeping one per root.
    param(
        [string]$ProjectsRoot = (Join-Path $HOME '.claude\projects'),
        [int]$Limit = 40,
        [string]$CachePath = (Join-Path (Split-Path $ProjectsRoot -Parent) 'claude-auto-sessions.json')
    )
    if (-not (Test-Path $ProjectsRoot)) { return @() }

    $cache = @{}
    if (Test-Path $CachePath) {
        try {
            (Get-Content $CachePath -Raw | ConvertFrom-Json).PSObject.Properties |
                ForEach-Object { $cache[$_.Name] = $_.Value }
        } catch { $cache = @{} }   # a corrupt cache is rebuilt, never fatal
    }

    # Enumerate ONE level down instead of recursing the whole tree and filtering afterwards. The
    # rule is unchanged - a transcript sits directly inside its project slug folder, everything
    # deeper is machinery - but enforcing it structurally is what makes it cheap. Measured
    # 2026-08-15: the recursive walk found 2 392 files of which only 389 were sessions, and
    # Test-ClaudeSessionFile's two Resolve-Path calls per file cost 1.19 s of the 1.62 s spent
    # just getting to the shortlist. Test-ClaudeSessionFile stays: it is the written form of the
    # same rule and the suites pin it.
    $files = Get-ChildItem -LiteralPath $ProjectsRoot -Directory -Force -ErrorAction SilentlyContinue |
        ForEach-Object { Get-ChildItem -LiteralPath $_.FullName -Filter *.jsonl -File -Force -ErrorAction SilentlyContinue } |
        Sort-Object LastWriteTime -Descending | Select-Object -First $Limit

    $out = @(); $fresh = @{}
    foreach ($f in $files) {
        # The cache key carries the mtime of THIS module, not a hand-written version literal.
        # A literal guards the schema but not the logic: on 2026-08-10 a corrected prompt counter
        # kept serving pre-fix numbers under an unchanged "v2|" key, and the stale value was
        # briefly reported as a verification result.
        $key = "$script:SummaryVersion|$($f.FullName)|$($f.LastWriteTimeUtc.Ticks)|$($f.Length)"
        if ($cache.ContainsKey($key)) { $summary = $cache[$key] }
        else {
            try { $summary = Get-ClaudeSessionSummary -Path $f.FullName } catch { continue }
        }
        $fresh[$key] = $summary
        $out += $summary
    }

    try { $fresh | ConvertTo-Json -Depth 6 | Set-Content $CachePath -Encoding utf8 } catch { }
    return $out
}

function Test-ClaudeSessionFile {
    # A session transcript sits DIRECTLY inside its project slug folder. Anything one level deeper
    # is machinery: <slug>/<session>/subagents/*.jsonl and <slug>/<session>/tool-results/*.
    # Filtering by parent depth covers both, and every future sibling directory, in one rule.
    #
    # Measured on this machine (PowerShell 7.6.4): Split-Path's -LiteralPath parameter set does not
    # accept -Parent here (a parameter-binding error, not this function's business) - LiteralPath
    # alone already returns the parent, so the switch is dropped rather than passed and failing.
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$ProjectsRoot)
    $parent = Split-Path -LiteralPath $Path
    $grand = Split-Path -LiteralPath $parent
    return ((Resolve-Path -LiteralPath $grand -ErrorAction SilentlyContinue).Path -eq
            (Resolve-Path -LiteralPath $ProjectsRoot -ErrorAction SilentlyContinue).Path)
}

function Measure-ClaudePrompts {
    # How many times a human actually typed something. Streamed with ReadLines and matched by
    # substring for the common case: parsing 111 MB of JSON would cost seconds, this costs ~300 ms
    # cold for 40 files and is free afterwards through the caller's cache. (Measured on this machine
    # 2026-08-10.)
    #
    # The naive count - every '"type":"user"' line - reports 428 for a session with 25 prompts,
    # because a tool result wears the user role. The three exclusions below are load-bearing.
    #
    # A line carrying no leading tag is unambiguous and is counted by substring alone. A line whose
    # content STARTS with '<' is ambiguous, because every rejection rule in Get-ClaudeUserPrompt -
    # $script:NoisePrefixes and $script:NoiseTagPattern alike - is anchored to the start of the
    # content (StartsWith / a ^-anchored regex), never a mid-string match. '"content":"<' is the JSON
    # encoding of exactly that condition, so it is the one trigger that cannot drift out of sync with
    # the authority as new wrapper tags get invented.
    #
    # A hand-maintained list of specific tag names WAS tried here and measured to drift already:
    # '<bash-stdout>' (a wrapper this list did not know about) slipped through as unambiguous and got
    # over-counted in 6 of 7058 real records on this machine on 2026-08-10, while Get-ClaudeUserPrompt
    # correctly rejected it via $script:NoiseTagPattern's '-stdout' suffix rule. Anchoring on
    # '"content":"<' instead reached exact agreement (0 over-count, 0 dropped) over the same data.
    param([Parameter(Mandatory)][string]$Path)
    $n = 0
    try {
        foreach ($line in [System.IO.File]::ReadLines($Path)) {
            if (-not $line.Contains('"type":"user"')) { continue }
            if ($line.Contains('"isSidechain":true')) { continue }
            if ($line.Contains('"tool_result"')) { continue }
            if ($line.Contains('"isMeta":true')) { continue }
            if ($line.Contains('"content":"<')) {
                $rec = ConvertFrom-JsonlLine -Line $line
                if ($rec -and (Get-ClaudeUserPrompt -Record $rec)) { $n++ }
                continue
            }
            $n++
        }
    } catch { return 0 }
    return $n
}

function Format-RelativeAge {
    # -Now is a parameter so the assertion does not depend on when the test runs.
    param([Parameter(Mandatory)][datetime]$From, [datetime]$Now = (Get-Date))
    $age = $Now - $From
    if ($age.TotalSeconds -lt 60) { return 'now' }
    if ($age.TotalMinutes -lt 60) { return '{0:N0} min' -f $age.TotalMinutes }
    if ($age.TotalHours -lt 24) { return '{0:N0} h' -f $age.TotalHours }
    return '{0:N0} d' -f $age.TotalDays
}
