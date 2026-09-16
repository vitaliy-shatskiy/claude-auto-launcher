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

# The summary walks' pre-filter, as a PATTERN rather than a byte-literal. Two shapes a literal
# '"type":"user"' misses while ConvertFrom-Json accepts them, and both made a real prompt invisible
# to the title, the last-message and the recent-messages walks alike (adversarial review 2026-09-16,
# A1/A2): one space after the colon - JSON-legal, and what any non-compact serialiser emits - and a
# \u-escaped key ('"\u0074ype"'), which the parser decodes and no substring can see. The escaped
# case is covered by the second half of the rule: a line carrying no structural "type" key AT ALL is
# parsed rather than skipped, which costs nothing because every record Claude Code writes has one.
$script:UserTypeLine = '"type"\s*:\s*"user"'
$script:ChatTypeLine = '"type"\s*:\s*"(user|assistant)"'

# The prompt counter's rules, as STRUCTURAL matches. A '"' that is not preceded by a backslash is a
# JSON structural quote; one that is belongs to string CONTENT. Without the lookbehind a prompt that
# quotes "tool_result" or "isSidechain":true in its own text is silently dropped from the count - the
# same class of drift the byte-literal filters above were carrying.
$script:StructUserType   = '(?<!\\)"type"\s*:\s*"user"'
$script:StructToolResult = '(?<!\\)"type"\s*:\s*"tool_result"'
$script:StructSidechain  = '(?<!\\)"isSidechain"\s*:\s*true'
$script:StructMeta       = '(?<!\\)"isMeta"\s*:\s*true'
# The MESSAGE's content, as a plain string opening with an ordinary, non-blank character: no
# rejection rule in Get-ClaudeUserPrompt can apply to it (every one of them is anchored to the start
# of the content, and IsNullOrWhiteSpace rejects blanks), so it is a human prompt and needs no parse.
# Anything else - a content ARRAY, a wrapper tag, an escape, whitespace - goes to the authority.
#
# Anchored INSIDE "message" and blank-rejecting for two measured disagreements (re-review
# 2026-09-16, W2): whitespace-only content counted 1 against the authority's 0 - a session with
# nothing to resume into offered as resumable, the exact symptom B3 is about - and a sibling
# "toolUseResult":{"content":"..."} let a record whose real message is a noise text BLOCK take the
# shortcut. [^{}]{0,400} keeps the between-part on one JSON object and bounds the scan.
$script:PlainPromptContent = '(?<!\\)"message"\s*:\s*\{[^{}]{0,400}?(?<!\\)"content"\s*:\s*"[^<"\\\s]'

# Resolved at load time so the per-file loop does not hash this module 40 times. A failure falls
# back to a constant rather than throwing: a cache that never invalidates is bad, a launcher that
# will not start is worse.
#
# The module's CONTENT, not its mtime. A hand-written literal guards the schema but not the logic
# (on 2026-08-10 a corrected prompt counter kept serving pre-fix numbers under an unchanged "v2|"
# key), which is why this is derived from the module at all - but a TIMESTAMP moves without a byte
# changing: a clone, a git checkout, a profile relink and a copy between the four account roots all
# rewrite it, and every cached summary on the machine is discarded for nothing. Since the four roots
# now share one cache file (Get-SessionsCachePath), an mtime that differs per copy would also have
# them invalidating each other's entries on every switch. A hash invalidates on an edit and on
# nothing else.
$script:SummaryVersion = try {
    [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData([IO.File]::ReadAllBytes($PSCommandPath))).Substring(0, 16)
} catch { 'v2' }

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
    #
    # The walk is bounded by BYTES as well as by newlines. Counting newlines alone is not a bound at
    # all: a transcript with few newlines is read end to end, and one 200 MB newline-free file cost
    # 14 127 ms and 2 515 MB of managed heap to return a single line (adversarial review, 2026-09-08 -
    # a transcript can hold a fetched page or a minified bundle on one line). Past the budget the
    # function returns what it found, which is what a preview pane needs; it is not a parser.
    param(
        [Parameter(Mandatory)][string]$Path,
        [int]$Count = 120,
        [int]$ChunkSize = 65536,
        [int]$MaxBytes = 4MB
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
        $walked = 0
        $pos = $stream.Length
        while ($pos -gt 0 -and $newlines -le $Count -and $walked -lt $MaxBytes) {
            $take = [Math]::Min($ChunkSize, $pos)
            $take = [Math]::Min($take, $MaxBytes - $walked)   # never step over the byte budget
            $pos -= $take
            $walked += $take
            $null = $stream.Seek($pos, [System.IO.SeekOrigin]::Begin)
            $buf = [byte[]]::new($take)
            # Read returns "up to" count bytes and is free to return fewer. $pos has already moved,
            # so accepting a short read would splice bytes from two different offsets into one
            # buffer and hand back a line that never existed in the file. Loop to $take or EOF.
            $read = 0
            while ($read -lt $take) {
                $got = $stream.Read($buf, $read, $take - $read)
                if ($got -le 0) { break }
                $read += $got
            }
            if ($read -le 0) { break }
            if ($read -lt $take) { $buf = $buf[0..($read - 1)] }
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

function Get-CleanTranscriptText {
    # Transcript text is UNTRUSTED: these files routinely hold fetched web pages, other people's
    # repository text and command output. Every field that reaches the terminal through Write-Frame
    # -> [Console]::Write must go through this, so a transcript carrying ESC ] 0 ; ... BEL, ESC [ 2 J
    # or an OSC 52 clipboard write cannot drive the reader's terminal instead of being displayed.
    # '\s' in .NET does NOT match ESC (0x1B) or BEL (0x07), so whitespace normalisation alone lets
    # all of it through. Control and format characters become spaces FIRST, so the collapse that
    # follows also closes the gaps they leave. Extracted to a function (was a local scriptblock in
    # Get-ClaudeSessionSummary) so Get-ClaudeSessions can apply the same rule when it refreshes
    # Project on a cache hit, without duplicating the regex.
    param([string]$Text)
    if ($Text) { return (($Text -replace '[\p{Cc}\p{Cf}]', ' ' -replace '\s+', ' ').Trim()) }
    return ''
}

function Get-ClaudeSessionSummary {
    param(
        [Parameter(Mandatory)][string]$Path,
        # 60 was too small: a session that opens with a skill injection buries its first real
        # prompt hundreds of records deep, and five of the eight newest transcripts on this
        # machine fell back to the session id. Get-Content stops at the limit, so a high value
        # costs nothing on short files.
        [int]$HeadLines = 400,
        [int]$TailLines = 120,
        # Resolved once per project DIRECTORY by the caller (Get-ClaudeSessions) so N sessions in
        # the same folder do not each repeat the same directory listing and transcript read. Left
        # optional so this function stays callable standalone - every existing test call keeps
        # working, and resolves it itself when not supplied.
        [string]$ProjectPath = $null,
        # Turns the substring pre-filter below OFF, so the unfiltered walk that parsed every line
        # stays runnable. It exists for the suite: the filter must change how MANY lines are
        # parsed and nothing else, and that is only assertable against the old path side by side.
        [switch]$NoPreFilter
    )
    $file = Get-Item -LiteralPath $Path
    $slug = Split-Path (Split-Path $Path -Parent) -Leaf
    $names = ConvertFrom-ClaudeProjectSlug -Slug $slug

    # The slug is not reversible (Projects.ps1). The transcript's own cwd is, so the name comes from
    # the real folder and the dash heuristic is only the fallback for a transcript with no cwd.
    if (-not $PSBoundParameters.ContainsKey('ProjectPath')) {
        $ProjectPath = Get-ProjectPathFromTranscript -Directory (Split-Path -LiteralPath $Path)
    }
    $projectName = if ($ProjectPath) { Split-Path -Path $ProjectPath -Leaf } else { $names.Project }

    $title = $null
    foreach ($line in (Get-Content -LiteralPath $Path -TotalCount $HeadLines -ErrorAction SilentlyContinue)) {
        # ConvertFrom-Json on a line that cannot become a title is pure cost, and it was 69% of a
        # cold listing. Get-ClaudeUserPrompt returns $null for any record whose type is not 'user',
        # so a line not carrying that field verbatim is rejected either way - the filter changes how
        # many lines are parsed, never which are accepted (pinned by Test-Sessions 18). Anchored on
        # the same rule Measure-ClaudePrompts uses, so the two cannot drift apart.
        if (-not $NoPreFilter -and $line.Contains('"type"') -and $line -notmatch $script:UserTypeLine) { continue }
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
        # Same rule as the head loop: this walk reads only 'user' records (Get-ClaudeUserPrompt) and
        # 'assistant' ones (Get-ClaudeRecordText), so a line carrying neither type contributes to
        # nothing and does not need parsing.
        if (-not $NoPreFilter -and $tail[$i].Contains('"type"') -and $tail[$i] -notmatch $script:ChatTypeLine) { continue }
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
    $promptCount = Measure-ClaudePromptDetail -Path $Path

    # Fall back to the last prompt before the session id: "c47c2bc1" identifies nothing, whereas
    # the most recent thing asked usually does.
    if (-not $title) { $title = $lastUser }
    # Sessions that were opened, cleared and abandoned really do contain no prompt. Say so:
    # a bare hex id in the title column reads like the parser failed.
    if (-not $title) { $title = "(no prompt) " + $file.BaseName.Substring(0, [Math]::Min(8, $file.BaseName.Length)) }

    # Transcript text is UNTRUSTED (see Get-CleanTranscriptText): every field below that can carry
    # transcript content goes through it before it reaches the terminal. Project now comes from the
    # transcript's own cwd (Projects.ps1) and needs it exactly as much as Title/LastUser/
    # LastAssistant do - the protection Project had before this task was structural (an NTFS
    # directory name cannot contain 0x00-0x1F), not deliberate, and a cwd sourced from JSON has no
    # such guarantee. Worktree alone is left unsanitised on purpose: it still comes from the slug
    # (Split on a directory NAME), never from transcript content, so it stays structurally safe.
    #
    # The resolved path itself (-ProjectPath, the parameter) is deliberately NOT in this object.
    # Review round 2: sanitising it corrupts genuine paths (two consecutive spaces, U+00AD, a ZWSP -
    # all legal in an NTFS path component, none of them survive Get-CleanTranscriptText) while a raw
    # cwd carrying \p{Cc} never named a real directory in the first place - so sanitising bought
    # safety only against a value that was already fake, at the cost of breaking real ones. There is
    # no consumer for a path here: the picker filters on Slug (exact) and displays Project; whatever
    # later needs to Set-Location asks Get-ProjectRegistry, which resolves AND validates with
    # Test-Path. A path is a machine value - it must not be corrupted to make it renderable.
    return [pscustomobject]@{
        SessionId       = $file.BaseName
        Path            = $file.FullName
        Slug            = $slug
        Project         = (Get-CleanTranscriptText -Text $projectName)
        Worktree        = $names.Worktree
        Modified        = $file.LastWriteTime
        SizeBytes       = $file.Length
        # An INT plus a flag, never the string "N+": see Measure-ClaudePromptDetail.
        PromptCount     = $promptCount.Count
        PromptCountCapped = $promptCount.Capped
        Title           = (Get-CleanTranscriptText -Text $title)
        LastUser        = (Get-CleanTranscriptText -Text $lastUser)
        LastAssistant   = (Get-CleanTranscriptText -Text $lastAssistant)
        RecentMessages  = @($recent | ForEach-Object { [pscustomobject]@{ Speaker = $_.Speaker; Text = (Get-CleanTranscriptText -Text (Get-CodeFenceCollapsedText -Text $_.Text)) } })
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

function Get-PhysicalDirectoryPath {
    # The directory a path really names, with any junction or symlink in it resolved to its final
    # target, and no trailing separator. Best-effort: anything unresolvable comes back as given.
    # EVERY component, not only the last. ResolveLinkTarget inspects the path's own final component
    # and nothing above it, so a junctioned PROFILE ROOT holding a real projects\ directory - a
    # layout equally consistent with "four roots' projects\ are one junction" - came back unresolved
    # and quietly got its own cache file (adversarial review 2026-09-16, B2). The parent chain is
    # resolved first and the leaf re-attached to whatever it resolved to.
    param([Parameter(Mandatory)][string]$Path, [int]$Depth = 0)
    # Memoised at the public entry only. A launcher run asks about the same two or three roots over
    # and over, and each ask walks the whole component chain; the answer cannot change inside one
    # short-lived process without somebody re-pointing a junction under it.
    $p = $Path
    try {
        # $true = resolve the FINAL target: a chain of links must land on the real directory, not on
        # the next link in it, or two roots reaching the same place through different hops would
        # still be treated as two places.
        $target = [IO.Directory]::ResolveLinkTarget($p, $true)
        if ($target) { $p = $target.FullName }
    } catch { }
    # 64 is a depth no real path reaches and a bound a cycle of links cannot spin past.
    if ($Depth -lt 64) {
        $parent = try { Split-Path -Path $p } catch { $null }
        if ($parent -and $parent -ne $p) {
            $realParent = Get-PhysicalDirectoryPath -Path $parent -Depth ($Depth + 1)
            if ($realParent -and $realParent -ne $parent) {
                $leaf = try { Split-Path -Path $p -Leaf } catch { $null }
                if ($leaf) {
                    $rebuilt = Join-Path $realParent $leaf
                    # The leaf may itself be a link once the chain above it moved.
                    try { $t2 = [IO.Directory]::ResolveLinkTarget($rebuilt, $true); if ($t2) { $rebuilt = $t2.FullName } } catch { }
                    $p = $rebuilt
                }
            }
        }
    }
    # A drive root is 'C:\' and trimming it to 'C:' changes what it means; nothing else needs its
    # trailing separator.
    if ($p.Length -gt 3) { $p = $p.TrimEnd([char]92, [char]47) }
    return $p
}

function Get-SessionsCachePath {
    # Where a projects root's summary cache lives. Keyed on the PHYSICAL directory, not on the path
    # the caller happened to name: on this machine the four account roots reach one projects
    # directory through a junction, so a cache keyed on the named root is built cold once per
    # account over the identical files, and each account's whole-file write is invisible to the
    # other three. Resolving the link first makes all four share one warm file.
    #
    # Best-effort by construction: a root that is not a link, or one that cannot be resolved,
    # falls back to the path as given - which is exactly the behaviour this replaces, so a
    # filesystem that has no reparse points loses nothing.
    param([Parameter(Mandatory)][string]$ProjectsRoot)
    $physical = Get-PhysicalDirectoryPath -Path $ProjectsRoot
    $parent = try { Split-Path -Path $physical } catch { $null }
    if (-not $parent) { $parent = $physical }
    return (Join-Path $parent 'claude-auto-sessions.json')
}

function Write-SessionsCache {
    # Set-Content truncates in place, and this file is now SHARED by every account root that reaches
    # the same projects directory while up to four launcher instances run at once - so a reader can
    # see a half-written file where before it could only see its own account's. Write a sibling temp
    # and move it over: the rename is atomic on NTFS, the last writer wins (all four are computing
    # the same rows from the same files, so there is nothing to merge), and a reader sees either the
    # whole old file or the whole new one. The temp is a SIBLING so the move is a rename rather than
    # a cross-volume copy, which would not be atomic.
    #
    # A failure here is swallowed, as the Set-Content it replaces was: the cache is an optimisation
    # and a launcher that will not start is worse than one that re-summarises.
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$Entries)
    # A launcher killed between the write and the move strands a full cache-sized temp beside the
    # cache, and nothing ever removed it (adversarial review 2026-09-16, B9). Siblings older than a
    # minute only: a younger one may belong to an instance that is mid-write right now.
    try {
        $dir = Split-Path -Path $Path
        $leaf = Split-Path -Path $Path -Leaf
        if ($dir -and $leaf -and (Test-Path -LiteralPath $dir -PathType Container)) {
            $cutoff = (Get-Date).AddMinutes(-1)
            foreach ($stale in @(Get-ChildItem -LiteralPath $dir -Filter "$leaf.*.tmp" -File -Force -ErrorAction SilentlyContinue)) {
                if ($stale.LastWriteTime -lt $cutoff) { Remove-Item -LiteralPath $stale.FullName -Force -ErrorAction SilentlyContinue }
            }
        }
    } catch { }
    $tmp = "$Path.$([guid]::NewGuid().ToString('N')).tmp"
    try {
        [IO.File]::WriteAllText($tmp, ($Entries | ConvertTo-Json -Depth 6), (New-Object System.Text.UTF8Encoding($false)))
        # File.Move-with-overwrite needs DELETE access to the destination and fails while somebody
        # holds it open without delete sharing. The launcher's own readers now share delete
        # (Read-SessionsCache), but a foreign reader still exists - so retry briefly rather than
        # losing the write on the first collision. Give up SILENTLY after that: the cache is an
        # optimisation and a launcher that will not start is worse than one that re-summarises.
        $moved = $false
        for ($i = 0; $i -lt 5 -and -not $moved; $i++) {
            try { [IO.File]::Move($tmp, $Path, $true); $moved = $true }
            catch { if ($i -lt 4) { Start-Sleep -Milliseconds 20 } }
        }
        if (-not $moved) { throw 'the cache file could not be replaced' }
        # This process now knows what the file holds without reading it back. Recorded so a later
        # read that finds the file BUSY has something better to answer than "everything is cold"
        # (Read-SessionsCache).
        try {
            $st = [IO.FileInfo]::new($Path)
            $script:SessionsCacheMemo = @{ Path = $Path; Stamp = "$($st.LastWriteTimeUtc.Ticks)|$($st.Length)"; Entries = $Entries }
        } catch { }
    } catch {
        try { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue } catch { }
    }
}

function Read-SessionsCache {
    # The cache file as a hashtable, or an empty one. Extracted from Get-ClaudeSessions so the read
    # side of the shared file has the same single home as the write side (Write-SessionsCache).
    #
    # A read that fails because another instance has the file OPEN is not a corrupt cache. Up to four
    # launchers run at once over one shared file, and `catch { @{} }` could not tell the two apart:
    # a busy cache became a silent full cold start - the exact cost this cache exists to remove -
    # and Get-Content's error painted over the picker frame (adversarial review 2026-09-16, G4/B14).
    # Three answers, in order: the file, a brief retry, then this process's own last good copy.
    param([Parameter(Mandatory)][string]$Path)
    $stat = try { [IO.FileInfo]::new($Path) } catch { $null }
    if (-not $stat -or -not $stat.Exists) { return @{} }
    $stamp = "$($stat.LastWriteTimeUtc.Ticks)|$($stat.Length)"
    for ($i = 0; $i -lt 5; $i++) {
        try {
            $text = $null
            # FileShare.Delete as well as ReadWrite: without it THIS read blocks another instance's
            # atomic replace of the same file (File.Move over an open destination needs delete
            # sharing), so the launcher's own cache read silently ate other launchers' writes.
            $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read,
                                  [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
            try { $text = [IO.StreamReader]::new($fs).ReadToEnd() } finally { $fs.Dispose() }
            $cache = @{}
            ($text | ConvertFrom-Json -ErrorAction Stop).PSObject.Properties |
                ForEach-Object { $cache[$_.Name] = $_.Value }
            $script:SessionsCacheMemo = @{ Path = $Path; Stamp = $stamp; Entries = $cache }
            return $cache
        } catch [System.IO.IOException] {
            # busy, not broken: another instance is mid-replace. 5 x 20 ms, then fall through.
            Start-Sleep -Milliseconds 20
        } catch {
            return @{}   # genuinely corrupt: rebuilt, never fatal
        }
    }
    # Still busy. This process read the same bytes earlier and nothing has changed them, so serving
    # that copy is strictly better than declaring every session cold.
    if ($script:SessionsCacheMemo -and $script:SessionsCacheMemo.Path -eq $Path -and
        $script:SessionsCacheMemo.Stamp -eq $stamp) {
        return $script:SessionsCacheMemo.Entries
    }
    return @{}
}

function Get-ClaudeSessionFile {
    # The ordered enumeration a paging caller walks: every transcript under the root (or under one
    # slug), newest first, as full paths. Taken ONCE when a picker opens and handed back to
    # Get-ClaudeSessions -Files for every page.
    #
    # Why a snapshot rather than -Skip over a fresh listing: the listing is sorted by mtime, and a
    # live session appends to its own transcript while the picker is open. One append between page 1
    # and page 2 shifts the whole window down by one - the duplicate is caught by the picker's dedup,
    # but the GAP is not, and the session that fell through it is unreachable for the rest of the run
    # (adversarial review 2026-09-16, C4). Metadata only: no transcript is opened here.
    param(
        [string]$ProjectsRoot = (Join-Path $HOME '.claude\projects'),
        # A LIST, because one real directory can own several slug folders and the picker is scoped to
        # all of them. Declared [string] this took the array joined with a SPACE, matched no
        # directory, and handed the launcher an empty snapshot - which then unbound -Files and put
        # Get-ClaudeSessions back on the full unscoped listing (re-review 2026-09-16, C1).
        [string[]]$ProjectSlug = @()
    )
    if (-not (Test-Path -LiteralPath $ProjectsRoot)) { return @() }
    $dirs = Get-ChildItem -LiteralPath $ProjectsRoot -Directory -Force -ErrorAction SilentlyContinue
    $slugs = @($ProjectSlug | Where-Object { $_ })
    if ($slugs.Count -gt 0) { $dirs = @($dirs | Where-Object { $_.Name -in $slugs }) }
    # A plain array: a caller that has to keep an EMPTY snapshot distinguishable from "no snapshot"
    # stores it in a [string[]] variable and passes THAT (claude-auto.ps1), rather than letting an
    # empty result unroll to $null through an argument expression.
    return @($dirs |
        ForEach-Object { Get-ChildItem -LiteralPath $_.FullName -Filter *.jsonl -File -Force -ErrorAction SilentlyContinue } |
        Sort-Object LastWriteTime -Descending |
        ForEach-Object { $_.FullName })
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
        # Paging. Summarising 40 transcripts is what a cold launch pays before the picker can draw
        # anything; the picker takes a first page and asks for the next only when the cursor reaches
        # the last row (Invoke-SessionPicker -FetchMore). -Skip is applied to the SORTED list, so
        # -Limit N with -Skip 0, N, 2N walks exactly the order an unpaged call returns.
        [int]$Skip = 0,
        [string]$CachePath = (Get-SessionsCachePath -ProjectsRoot $ProjectsRoot),
        [string]$ProjectSlug = '',
        # A SNAPSHOT of transcript paths, newest first, taken once by the caller
        # (Get-ClaudeSessionFile) when a picker opens; -Skip then indexes THIS list instead of a
        # listing re-sorted on every call. See Get-ClaudeSessionFile for why the difference matters.
        # $null (the default) keeps the standalone behaviour: enumerate the root on every call.
        [string[]]$Files = $null
    )
    # -LiteralPath, matching the sibling reader Get-ProjectRegistry: the wildcard PATH set reads a
    # root spelled 'C:\Users\J\Projects\[old]\projects' as a PATTERN and matches nothing, so the
    # project screen lists the project and pressing `r` on it shows an empty picker.
    # A snapshot ALREADY carries its scope, so a slug beside it is either redundant or a
    # contradiction, and silently ignoring it is how a scoped call quietly became an unscoped one.
    if ($PSBoundParameters.ContainsKey('Files') -and @($ProjectSlug | Where-Object { $_ }).Count -gt 0) {
        throw 'Get-ClaudeSessions: -ProjectSlug cannot be combined with -Files - the snapshot already carries the scope.'
    }
    if (-not (Test-Path -LiteralPath $ProjectsRoot)) { return @() }
    # Clamped here rather than left to Select-Object, whose range error is TERMINATING and would
    # escape a picker loop as a raw binding failure.
    if ($Skip -lt 0) { $Skip = 0 }

    $cache = Read-SessionsCache -Path $CachePath

    # Enumerate ONE level down instead of recursing the whole tree and filtering afterwards. The
    # rule is unchanged - a transcript sits directly inside its project slug folder, everything
    # deeper is machinery - but enforcing it structurally is what makes it cheap. Measured
    # 2026-08-15: the recursive walk found 2 392 files of which only 389 were sessions, and
    # Test-ClaudeSessionFile's two Resolve-Path calls per file cost 1.19 s of the 1.62 s spent
    # just getting to the shortlist. Test-ClaudeSessionFile stays: it is the written form of the
    # same rule and the suites pin it.
    # PRESENCE, not emptiness: -Files @() means "this scope holds no transcripts" and must yield an
    # empty page, where falling through to the enumeration would answer with the whole account.
    $all = if ($PSBoundParameters.ContainsKey('Files')) {
        # The caller's snapshot, in the caller's order - deliberately NOT re-sorted. A transcript
        # deleted since the snapshot was taken is dropped here rather than throwing.
        @($Files | ForEach-Object { try { Get-Item -LiteralPath $_ -Force -ErrorAction Stop } catch { } })
    } else {
        $dirs = Get-ChildItem -LiteralPath $ProjectsRoot -Directory -Force -ErrorAction SilentlyContinue
        # Filtering the DIRECTORY list, not the summaries: with the filter on, only that project's
        # transcripts are ever opened, so scoping the picker makes it cheaper rather than slower.
        if ($ProjectSlug) { $dirs = @($dirs | Where-Object { $_.Name -eq $ProjectSlug }) }
        @($dirs |
            ForEach-Object { Get-ChildItem -LiteralPath $_.FullName -Filter *.jsonl -File -Force -ErrorAction SilentlyContinue } |
            Sort-Object LastWriteTime -Descending)
    }
    # NOT named $files: PowerShell variable names are case-insensitive, so a local $files IS the
    # [string[]]$Files parameter, and the coercion turns every FileInfo into its path string.
    $window = @($all | Select-Object -Skip $Skip -First $Limit)

    # Resolved once per project DIRECTORY, not once per session file: on a cold cache, N sessions
    # under the same slug would otherwise repeat the same directory listing and the same read of
    # the newest transcript N times to compute the identical answer.
    $projectPaths = @{}
    # The cache FILE is shared by every root that reaches this physical directory
    # (Get-SessionsCachePath), so the cache KEY has to be shared too. Keyed on $f.FullName the four
    # account roots write four disjoint key sets into one file, and because an unfiltered call
    # replaces that file wholesale, each account switch would delete the other three's rows - a
    # shared file with unshared keys is strictly worse than four files. Rewriting the named root's
    # prefix to the physical one makes the same transcript one key however it was reached.
    $namedRoot = if ($ProjectsRoot.Length -gt 3) { $ProjectsRoot.TrimEnd([char]92, [char]47) } else { $ProjectsRoot }
    $physicalRoot = Get-PhysicalDirectoryPath -Path $ProjectsRoot
    $out = @(); $fresh = @{}
    foreach ($f in $window) {
        # The cache key carries the mtime of THIS module, not a hand-written version literal.
        # A literal guards the schema but not the logic: on 2026-08-10 a corrected prompt counter
        # kept serving pre-fix numbers under an unchanged "v2|" key, and the stale value was
        # briefly reported as a verification result.
        $keyPath = $f.FullName
        if ($namedRoot -ne $physicalRoot -and $keyPath.StartsWith($namedRoot, [StringComparison]::OrdinalIgnoreCase)) {
            $keyPath = $physicalRoot + $keyPath.Substring($namedRoot.Length)
        }
        $key = "$script:SummaryVersion|$keyPath|$($f.LastWriteTimeUtc.Ticks)|$($f.Length)"
        try {
            $dirName = Split-Path -LiteralPath $f.FullName
            if (-not $projectPaths.ContainsKey($dirName)) {
                # Memoise a FAILED resolution too, not only a successful one: both Get-ChildItem and
                # Get-Content inside Get-ProjectPathFromTranscript already run with
                # -ErrorAction SilentlyContinue, so this is latent today, but an unmemoised throw
                # would be retried once per file in the directory - exactly undoing the once-per-
                # directory guarantee above, and under the one condition (a failing resolver) it
                # exists to protect against. $null memoises the same as "no cwd found".
                try { $projectPaths[$dirName] = Get-ProjectPathFromTranscript -Directory $dirName }
                catch { $projectPaths[$dirName] = $null }
            }
            $projectPath = $projectPaths[$dirName]
            if ($cache.ContainsKey($key)) {
                $summary = $cache[$key]
                # The resolved cwd is decided by whichever transcript is newest in the DIRECTORY
                # right now, a different file than the one this cache key names - a session cached
                # before a newer sibling arrived (or before that sibling ever had a cwd) must not
                # keep serving a stale or heuristic name forever. The expensive fields (title, tail
                # walk, prompt count) stay cached; only this cheap, memo-backed field is refreshed,
                # through the same sanitiser Get-ClaudeSessionSummary uses (both are transcript-
                # sourced, cache or not).
                # Path is refreshed for the same reason and at the same cost: the row may have been
                # cached by a DIFFERENT account root reaching this transcript through its own
                # junction, and a consumer must get the path it asked about, not the one whoever
                # filled the cache happened to use.
                # Add-Member -Force, not an assignment: `$o.Path = x` THROWS on a PSCustomObject that
                # has no Path property, and the enclosing `catch { continue }` then dropped the
                # session from the listing with no trace (adversarial review 2026-09-16, B12).
                $summary | Add-Member -NotePropertyName Path -NotePropertyValue $f.FullName -Force
                $slugNames = ConvertFrom-ClaudeProjectSlug -Slug "$($summary.Slug)"
                $projectName = if ($projectPath) { Split-Path -Path $projectPath -Leaf } else { $slugNames.Project }
                $summary.Project = Get-CleanTranscriptText -Text $projectName
            } else {
                $summary = Get-ClaudeSessionSummary -Path $f.FullName -ProjectPath $projectPath
            }
        } catch { continue }   # one unreadable file or directory is skipped, never fatal to the listing
        $fresh[$key] = $summary
        $out += $summary
    }

    # Pruning is a SURVEY's privilege and nothing else's: only a call that actually looked at every
    # transcript under the root knows that what it did not find is gone. A scoped call saw one
    # project; a -Skip'ped or snapshot-paged call saw a window; and a -Limit'ed call saw its first N
    # - which is exactly what the launcher's own first page is. Treating that as a survey replaced
    # the whole shared file with ten rows and made every other page, of every other account, cold
    # again on the next launch, forever (adversarial review 2026-09-16, C3/C3b: 25 cache keys before
    # the launcher's call, 10 after).
    $isSurvey = (-not $ProjectSlug) -and (-not $PSBoundParameters.ContainsKey('Files')) -and ($Skip -le 0) -and ($window.Count -eq $all.Count)
    if ($isSurvey) {
        Write-SessionsCache -Path $CachePath -Entries $fresh
    } else {
        $merged = $cache.Clone()
        foreach ($k in $fresh.Keys) { $merged[$k] = $fresh[$k] }
        Write-SessionsCache -Path $CachePath -Entries $merged
    }
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
    #
    # Bounded by BYTES, the same rule and the same 4 MB budget as Get-FileTailLines, and for the same
    # reason: a walk limited only by what it is looking for is not limited at all. This one streams
    # from the START, so an exact count means reading the whole transcript - measured at 24% of a
    # cold listing, and the largest transcript on this machine is 48 MB. A file INSIDE the budget
    # keeps its exact count, because "25 msgs" is the number the picker shows and a wrong one there
    # is worse than a slow launcher. Past it the answer is the string "N+", which is what the picker
    # then displays: it is the truth, where a silently short number would be a lie. Callers that
    # test it (Select-ResumableSessions' PromptCount -gt 0) still read it as more than zero.
    param(
        [Parameter(Mandatory)][string]$Path,
        [int]$MaxBytes = 4MB
    )
    return (Measure-ClaudePromptDetail -Path $Path -MaxBytes $MaxBytes).Count
}

function Measure-ClaudePromptDetail {
    # The count AND whether the byte budget cut it short: @{ Count = [int]; Capped = [bool] }.
    #
    # Two values, never one string. "N+" carried both and was a STRING, and PowerShell coerces the
    # other operand to the left one's type: '0+' -gt 0 is TRUE, so a >4 MB transcript whose first
    # 4 MB holds nothing a human typed was offered as resumable, and '9+' -gt 10 is TRUE, so any
    # future ordering on this field would silently be lexicographic (adversarial review 2026-09-16,
    # E4a/E4b/E4c). The picker renders the "+" from the flag (Format-PromptCount, Screens.ps1).
    #
    # ACCEPTANCE is the authority's, not a private set of substrings. The old escape hatch fired only
    # on the literal '"content":"<', which a content ARRAY never produces, so every wrapper delivered
    # as a text block - a system-reminder, a local-command-stdout, a bare slash command - was counted
    # as a human prompt while the title column said "(no prompt)" about the same session; and the
    # byte-literal '"type":"user"' dropped a genuine prompt written as '{"type": "user"'
    # (adversarial review, A3). The substring tests below are PRE-TESTS for the structural patterns
    # behind them, and anything ambiguous is parsed and handed to Get-ClaudeUserPrompt itself. A
    # tool-heavy transcript still costs no parses: a tool_result line is rejected structurally.
    #
    # Bounded by BYTES, the same rule and the same 4 MB budget as Get-FileTailLines. Counting
    # CHARACTERS read 1.94x the budget on Cyrillic text - which this owner writes - on a function
    # measured at 24% of a cold listing. The budget is spent AFTER the line is examined, so the
    # record that straddles the boundary is counted rather than lost.
    param(
        [Parameter(Mandatory)][string]$Path,
        [int]$MaxBytes = 4MB
    )
    $n = 0
    $capped = $false
    try {
        # One stat, so a small file - the common case - pays nothing for a budget it cannot reach.
        # A file that cannot be stat'ed is treated as bounded: the pessimistic choice.
        $bounded = $true
        try { $bounded = ($MaxBytes -gt 0 -and [IO.FileInfo]::new($Path).Length -gt $MaxBytes) } catch { $bounded = $true }
        # The budget is taken off the FILE, in one read, rather than added up per line. Per-line
        # [Text.Encoding]::UTF8.GetByteCount is exact but costs a marshalled call per line: measured
        # on this machine 2026-09-16 over the ten newest transcripts (26 MB), 397 ms against 38 ms
        # for the character count it replaced - it alone took a cold -Limit 10 listing from 420 ms to
        # 747 ms. Reading the window as bytes is exact AND free: the file offset IS the byte count.
        $lines = if ($bounded) { Read-BoundedFileLines -Path $Path -MaxBytes $MaxBytes } else { [System.IO.File]::ReadLines($Path) }
        if ($bounded) { $capped = $true }
        foreach ($line in $lines) {
            if ($line.Contains('"user"')) {
                if (-not ($line.Contains('"tool_result"') -and $line -match $script:StructToolResult) -and
                    -not ($line.Contains('"isSidechain"') -and $line -match $script:StructSidechain) -and
                    -not ($line.Contains('"isMeta"') -and $line -match $script:StructMeta) -and
                    -not ($line.Contains('"type"') -and $line -notmatch $script:StructUserType)) {
                    if ($line -match $script:PlainPromptContent) { $n++ }
                    else {
                        $rec = ConvertFrom-JsonlLine -Line $line
                        if ($rec -and (Get-ClaudeUserPrompt -Record $rec)) { $n++ }
                    }
                }
            }
        }
    } catch { return [pscustomobject]@{ Count = 0; Capped = $false } }
    return [pscustomobject]@{ Count = $n; Capped = $capped }
}

function Read-BoundedFileLines {
    # The first $MaxBytes of a file, as lines, plus the rest of whatever record straddles that
    # boundary. Counting the budget off the file OFFSET is what makes it exact in bytes at no cost:
    # the alternative, adding up a per-line byte count, is a marshalled call per line and was
    # measured at 10x the character count it replaced.
    #
    # The straddling record is INCLUDED. The budget is charged for a line before the line is
    # examined either way, and a caller that stops one record short of what began inside its budget
    # is reporting a number it did not have to be wrong about.
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][int]$MaxBytes)
    $fs = $null
    try {
        # ReadWrite sharing: a live session is appending to its own transcript while this reads it.
        $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
        $buf = [byte[]]::new($MaxBytes)
        $got = 0
        while ($got -lt $MaxBytes) {
            $r = $fs.Read($buf, $got, $MaxBytes - $got)
            if ($r -le 0) { break }
            $got += $r
        }
        $text = [Text.Encoding]::UTF8.GetString($buf, 0, $got)
        # Only when the window cut a line in half: a window ending exactly on a newline is complete.
        if ($got -gt 0 -and $buf[$got - 1] -ne 10) {
            $tail = [System.Collections.Generic.List[byte]]::new()
            $chunk = [byte[]]::new(4096)
            $done = $false
            while (-not $done) {
                $r = $fs.Read($chunk, 0, $chunk.Length)
                if ($r -le 0) { break }
                $nl = [Array]::IndexOf($chunk, [byte]10, 0, $r)
                $take = if ($nl -ge 0) { $nl } else { $r }
                # Array.Copy into a sized byte[], not $chunk[0..($take-1)]: the range operator
                # produces an Object[] and AddRange refuses it, and 0..($take-1) with $take = 0 is
                # @(0, -1) in PowerShell rather than an empty range.
                if ($take -gt 0) {
                    $piece = [byte[]]::new($take)
                    [Array]::Copy($chunk, 0, $piece, 0, $take)
                    $tail.AddRange($piece)
                }
                if ($nl -ge 0) { $done = $true }
            }
            if ($tail.Count -gt 0) { $text += [Text.Encoding]::UTF8.GetString($tail.ToArray()) }
        }
        # One regex split, not a per-line pipeline: at 30 000 lines a ForEach-Object costs more than
        # the read this function exists to bound.
        return @($text -split "`r?`n")
    } catch {
        return @()
    } finally {
        if ($fs) { $fs.Dispose() }
    }
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
