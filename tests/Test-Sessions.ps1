# Assertions for Sessions.ps1. Run: pwsh -File Test-Sessions.ps1
try {
    . "$PSScriptRoot\..\claude-auto\Sessions.ps1"
    . "$PSScriptRoot\..\claude-auto\Projects.ps1"   # Get-ProjectPathFromTranscript - the real-cwd authority
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

function Assert-True {
    # Assert-Equal $null $x stringifies both sides, so it passes for '' and @() too. Use this for a
    # real type-aware comparison.
    param([bool]$Actual, [string]$Because)
    $script:Ran++
    if (-not $Actual) { Write-Host "FAIL  $Because"; $script:Failed++ } else { Write-Host "ok    $Because" }
}

$fx = "$PSScriptRoot\fixtures"

# 1. The first four user records are the known noise forms; the title is the fifth.
$s = Get-ClaudeSessionSummary -Path "$fx\noisy-head.jsonl"
Assert-Equal 'rewrite the launcher menu' $s.Title 'title skips caveat/command/meta/tool_result records'

# 2. Subagent traffic never surfaces.
$s = Get-ClaudeSessionSummary -Path "$fx\with-sidechain.jsonl"
Assert-Equal 'real user prompt' $s.Title 'isSidechain records are ignored'
Assert-Equal 'real assistant reply' $s.LastAssistant 'sidechain assistant output is ignored, thinking blocks dropped'

# 3. Worktree slug parsing.
$w = ConvertFrom-ClaudeProjectSlug -Slug 'C--Users-someone-Desktop-Projects-Demo-Service--claude-worktrees-feature-1'
Assert-Equal 'Demo-Service' $w.Project 'repo name recovered from a worktree slug'
Assert-Equal 'feature-1' $w.Worktree 'worktree name recovered'

$p = ConvertFrom-ClaudeProjectSlug -Slug 'C--Users-someone-Desktop-Projects-Demo'
Assert-Equal 'Demo' $p.Project 'plain slug yields the folder name'
Assert-Equal '' "$($p.Worktree)" 'plain slug has no worktree'

# 4. A truncated final line must not throw.
$s = Get-ClaudeSessionSummary -Path "$fx\truncated-tail.jsonl"
Assert-Equal 'survives truncation' $s.Title 'a malformed trailing line is skipped'
Assert-Equal 'fine' $s.LastAssistant 'the last valid assistant record is still found'

# 5. A slash command carries its human text in <command-args>. Bare /clear stays noise, but
#    /doctor with an argument is the only description such a session has.
$s = Get-ClaudeSessionSummary -Path "$fx\slash-command.jsonl"
Assert-Equal '/doctor установка зависла на 40%' $s.Title 'command args become the title, bare /clear is skipped'
Assert-Equal 'Checking the installer.' $s.LastAssistant 'assistant reply still read from the tail'

# 6. The slug parser indexed a STRING with [-1] when the pipeline yielded one element, so
#    'subagents' rendered as the project 's'. Any slug without a 'Projects-' segment reproduces it.
$p = ConvertFrom-ClaudeProjectSlug -Slug 'subagents'
Assert-Equal 'subagents' $p.Project 'a single-segment slug yields the whole segment, not its last character'
$p = ConvertFrom-ClaudeProjectSlug -Slug 'C--Users-someone-Desktop-Scratch'
Assert-Equal 'Scratch' $p.Project 'a slug outside Projects- still yields the folder name'

# 7. Subagent transcripts live in <slug>/<session>/subagents/ and are not sessions.
$root = "$fx\tree"
$top = "$root\C--Users-someone-Projects-Demo\aaaa1111.jsonl"
$sub = "$root\C--Users-someone-Projects-Demo\aaaa1111\subagents\agent-b1.jsonl"
Assert-Equal $true  (Test-ClaudeSessionFile -Path $top -ProjectsRoot $root) 'a transcript directly under a slug is a session'
Assert-Equal $false (Test-ClaudeSessionFile -Path $sub -ProjectsRoot $root) 'a subagent transcript is not a session'

$sessions = @(Get-ClaudeSessions -ProjectsRoot $root -Limit 40 -CachePath "$env:TEMP\claude-auto-test-cache.json")
Assert-Equal 1 $sessions.Count 'only the top-level transcript is listed'
Assert-Equal 'Demo' $sessions[0].Project 'the listed session carries its project name'

# 8. Human prompts are counted; tool results wearing the user role are not.
Assert-Equal 3 (Measure-ClaudePrompts -Path "$fx\counted.jsonl") 'tool results, sidechain and meta records do not count as prompts'
$s = Get-ClaudeSessionSummary -Path "$fx\counted.jsonl"
Assert-Equal 3 $s.PromptCount 'the summary carries the prompt count'

# 8b. RecentMessages feeds the picker's detail pane: real exchanges only (tool results, sidechain
# and meta records excluded, same rule as the prompt count above), oldest first.
Assert-Equal 5 $s.RecentMessages.Count 'RecentMessages carries every real exchange, not just the last of each speaker'
Assert-Equal 'user'      $s.RecentMessages[0].Speaker 'the first recent message is the oldest real exchange'
Assert-Equal 'first prompt' $s.RecentMessages[0].Text 'and its text is the first real prompt, not a filtered-out record'
Assert-Equal 'assistant' $s.RecentMessages[1].Speaker 'speakers alternate as they actually did in the transcript'
Assert-Equal 'ok'        $s.RecentMessages[1].Text     'assistant text is read the same way as LastAssistant'
Assert-Equal 'assistant' $s.RecentMessages[4].Speaker  'the newest recent message is the final assistant reply, chronologically last'
Assert-Equal 'done'      $s.RecentMessages[4].Text     'its text matches the transcript''s final assistant record'

# 9. <task-notification> is noise, like every other injected wrapper.
$s = Get-ClaudeSessionSummary -Path "$fx\task-notification.jsonl"
Assert-Equal 'the real question' $s.Title 'a task-notification record never becomes the title'
Assert-Equal 'the real question' $s.LastUser 'a task-notification record never becomes the last message'

# 10. Relative age, computed against an injected clock so the assertion is stable.
$now = Get-Date '2026-08-10 20:00:00'
Assert-Equal 'now'    (Format-RelativeAge -From (Get-Date '2026-08-10 19:59:40') -Now $now) 'under a minute reads as now'
Assert-Equal '5 min'  (Format-RelativeAge -From (Get-Date '2026-08-10 19:55:00') -Now $now) 'minutes are shown up to an hour'
Assert-Equal '3 h'    (Format-RelativeAge -From (Get-Date '2026-08-10 17:00:00') -Now $now) 'hours are shown up to a day'
Assert-Equal '2 d'    (Format-RelativeAge -From (Get-Date '2026-08-08 20:00:00') -Now $now) 'days beyond that'

# 11. Measure-ClaudePrompts must agree with Get-ClaudeUserPrompt about what counts as a prompt.
#     A bare '/clear' wrapper carries an empty <command-args></command-args> and is not a prompt;
#     '/doctor <real text>' carries non-empty args and must still count - it is the session's only
#     description. A count that disagrees with the title logic shows "1 msg" beside "(no prompt)".
Assert-Equal 0 (Measure-ClaudePrompts -Path "$fx\bare-clear.jsonl") 'a bare /clear wrapper is not a prompt'
Assert-Equal 1 (Measure-ClaudePrompts -Path "$fx\slash-command.jsonl") 'a slash command carrying real argument text still counts as a prompt'
Assert-Equal 1 (Measure-ClaudePrompts -Path "$fx\task-notification.jsonl") 'a task-notification wrapper is not counted as a prompt either'

# A tag appearing MID-STRING, not at the start, must not disqualify the record: the unanchored
# substring check used to reject it outright, while Get-ClaudeUserPrompt (anchored, StartsWith-only)
# would have counted it. Deferring to the authority on ambiguous lines closes that gap.
Assert-Equal 1 (Measure-ClaudePrompts -Path "$fx\embedded-tag.jsonl") 'a tag embedded mid-string does not disqualify a real prompt'

# 12. The cache key carries the module's own mtime (SummaryVersion), not a hand-written literal,
#     so a future correction to the summarising logic invalidates the cache by itself.
$tmpRoot = Join-Path $env:TEMP "claude-auto-cache-test-$(Get-Random)"
$tmpSlug = Join-Path $tmpRoot 'C--Users-someone-Desktop-Projects-CacheTest'
New-Item -ItemType Directory -Path $tmpSlug -Force | Out-Null
$tmpSession = Join-Path $tmpSlug 'bbbb2222.jsonl'
$tmpLines = @(
    '{"type":"user","message":{"content":"first prompt"}}',
    '{"type":"assistant","message":{"content":[{"type":"text","text":"ok"}]}}',
    '{"type":"user","message":{"content":"second prompt"}}'
)
[IO.File]::WriteAllText($tmpSession, (($tmpLines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))

$tmpCache = Join-Path $env:TEMP "claude-auto-cache-roundtrip-$(Get-Random).json"
$first  = @(Get-ClaudeSessions -ProjectsRoot $tmpRoot -Limit 10 -CachePath $tmpCache)
$second = @(Get-ClaudeSessions -ProjectsRoot $tmpRoot -Limit 10 -CachePath $tmpCache)
Assert-Equal $first[0].PromptCount $second[0].PromptCount 'a cache round-trip returns the same prompt count on the second call'

$cacheJson = Get-Content -LiteralPath $tmpCache -Raw | ConvertFrom-Json
$cacheKey = @($cacheJson.PSObject.Properties)[0].Name
Assert-Equal $true ($cacheKey.StartsWith("$script:SummaryVersion|")) "the cache key begins with the module's own SummaryVersion, not a hand-written literal"

Remove-Item -LiteralPath $tmpRoot -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $tmpCache -Force -ErrorAction SilentlyContinue

# 13. Fenced code blocks collapse to a compact marker BEFORE whitespace normalisation, while real
#     newlines still exist to count - by the time RecentMessages text is cleaned every newline is
#     already a single space, so the marker must be built here or the line count would be a lie.
$noFence = 'plain text with `inline` backticks, no fence at all'
Assert-Equal $noFence (Get-CodeFenceCollapsedText -Text $noFence) 'text with only inline backticks is untouched'

$fenceText = "Here:" + "`n" + '```powershell' + "`n" + "Write-Host 'a'" + "`n" + "Write-Host 'b'" + "`n" + "Write-Host 'c'" + "`n" + '```' + "`n" + "Done."
Assert-Equal "Here:`n[code: 3 lines]`nDone." (Get-CodeFenceCollapsedText -Text $fenceText) 'a fenced block collapses to a marker carrying its real line count'

$twoFences = ('```' + "`n" + "one" + "`n" + '```') + ' and ' + ('```' + "`n" + "x`ny" + "`n" + '```')
Assert-Equal '[code: 1 lines] and [code: 2 lines]' (Get-CodeFenceCollapsedText -Text $twoFences) 'multiple fenced blocks in the same message each collapse independently'

# The real transcript case: a fence buried in an assistant reply, read through the full summary
# pipeline (Get-ClaudeSessionSummary -> RecentMessages), not the helper called directly.
$s = Get-ClaudeSessionSummary -Path "$fx\code-fence.jsonl"
Assert-Equal 'Here: [code: 3 lines] Done.' $s.RecentMessages[1].Text 'a fenced block in a real transcript collapses to a compact marker in RecentMessages, with the real line count preserved from before whitespace cleanup'

# 14. deferred review finding: the session picker must read the SELECTED account's root, never a
#     hardcoded default - claude-auto.ps1 used to call Get-ClaudeSessions with no root at all, which
#     lists the canonical account's sessions no matter which account is chosen.
$roots = [ordered]@{ work = "$fx\multi-root\work"; second = "$fx\multi-root\second" }
Assert-Equal (Join-Path $roots['second'] 'projects') (Get-SessionsRootForAccount -Account 'second' -ProfileRoots $roots) 'resolves to the SELECTED account root, not the first one'
Assert-Equal (Join-Path $roots['work'] 'projects')   (Get-SessionsRootForAccount -Account 'work' -ProfileRoots $roots)   'and to the canonical root when that IS the selected account'
try { $null = Get-SessionsRootForAccount -Account 'nope' -ProfileRoots $roots; $threw = $false } catch { $threw = $true }
Assert-Equal $true $threw 'an unknown account throws rather than silently resolving somewhere'

$mrRoot = Join-Path $env:TEMP ("claude-auto-multiroot-$(Get-Random)")
$workProjects = Join-Path $mrRoot 'work\projects'; $secondProjects = Join-Path $mrRoot 'second\projects'
New-Item -ItemType Directory -Force (Join-Path $workProjects 'C--Users-someone-Desktop-Projects-WorkOnly') | Out-Null
New-Item -ItemType Directory -Force (Join-Path $secondProjects 'C--Users-someone-Desktop-Projects-SecondOnly') | Out-Null
$workLines = @('{"type":"user","message":{"content":"work session prompt"}}', '{"type":"assistant","message":{"content":[{"type":"text","text":"ok"}]}}')
$secondLines = @('{"type":"user","message":{"content":"second session prompt"}}', '{"type":"assistant","message":{"content":[{"type":"text","text":"ok"}]}}')
[IO.File]::WriteAllText((Join-Path $workProjects 'C--Users-someone-Desktop-Projects-WorkOnly\aaaaaaaa.jsonl'), (($workLines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))
[IO.File]::WriteAllText((Join-Path $secondProjects 'C--Users-someone-Desktop-Projects-SecondOnly\bbbbbbbb.jsonl'), (($secondLines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))

$rootsFake = [ordered]@{ work = (Join-Path $mrRoot 'work'); second = (Join-Path $mrRoot 'second') }
$sessionsForSecond = @(Get-ClaudeSessions -ProjectsRoot (Get-SessionsRootForAccount -Account 'second' -ProfileRoots $rootsFake) -Limit 40)
Assert-Equal 1 $sessionsForSecond.Count 'account second sees exactly one session'
Assert-Equal 'bbbbbbbb' $sessionsForSecond[0].SessionId 'and it is SECOND''s own session, not WORK''s'
Assert-Equal $false ($sessionsForSecond.SessionId -contains 'aaaaaaaa') 'the picker''s source list for account B does not contain A''s session'

# The default CachePath is keyed off the account's OWN root, not one hardcoded path shared by
# every account - otherwise every call's whole-file overwrite thrashes the other accounts' entries.
$sessionsForWork = @(Get-ClaudeSessions -ProjectsRoot (Get-SessionsRootForAccount -Account 'work' -ProfileRoots $rootsFake) -Limit 40)
Assert-Equal $true (Test-Path (Join-Path $rootsFake['second'] 'claude-auto-sessions.json')) 'account second gets its own cache file'
Assert-Equal $true (Test-Path (Join-Path $rootsFake['work'] 'claude-auto-sessions.json'))   'account work gets its own cache file, not a shared one that just got overwritten'
Remove-Item -LiteralPath $mrRoot -Recurse -Force -ErrorAction SilentlyContinue

# 15. Transcript text is UNTRUSTED: these files routinely hold fetched web pages and other repos'
#     source. Every summary field reaches the terminal through Write-Frame -> [Console]::Write, so a
#     transcript carrying ESC ] 0 ; ... BEL (window title / OSC 52 clipboard write), ESC [ 2 J
#     (clear screen) or SGR colour would drive the reader's terminal. '\s' in .NET does NOT match
#     ESC (0x1B) or BEL (0x07), so the summary's own whitespace cleanup let all of it through.
#     Written as \u escapes so this file stays plain ASCII; ConvertFrom-Json turns them into the
#     real control bytes, exactly as a hostile transcript would carry them.
$escRoot = Join-Path $env:TEMP "claude-auto-esc-$(Get-Random)"
$escSlug = Join-Path $escRoot 'C--Users-someone-Desktop-Projects-EscTest'
New-Item -ItemType Directory -Path $escSlug -Force | Out-Null
$escFile = Join-Path $escSlug 'cccc3333.jsonl'
# ESC and BEL are built here from their code points, never typed into this file: a source file
# carrying raw control bytes is unreadable in a diff and one careless editor away from being lost.
$esc = [char]27; $bel = [char]7
$escLines = @(
    ('{"type":"user","message":{"content":"' + $esc + ']0;pwned' + $bel + 'hello ' + $esc + '[2Jworld"}}'),
    ('{"type":"assistant","message":{"content":[{"type":"text","text":"' + $esc + '[31mred' + $esc + '[0m reply' + $bel + '"}]}}'),
    ('{"type":"user","message":{"content":"second ' + $esc + ']52;c;cGF5bG9hZA==' + $bel + 'prompt"}}')
)
[IO.File]::WriteAllText($escFile, (($escLines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))

$ctrl = '[\p{Cc}\p{Cf}]'
$s = Get-ClaudeSessionSummary -Path $escFile
Assert-Equal ']0;pwned hello [2Jworld' $s.Title 'control bytes in the title become spaces and collapse, leaving the printable text'
Assert-Equal $false ($s.Title -match $ctrl)         'no control character survives into Title'
Assert-Equal $false ($s.LastUser -match $ctrl)      'no control character survives into LastUser'
Assert-Equal $false ($s.LastAssistant -match $ctrl) 'no control character survives into LastAssistant'
Assert-Equal 0 (@($s.RecentMessages | Where-Object { $_.Text -match $ctrl }).Count) 'no control character survives into any RecentMessages entry'
Assert-Equal $true ($s.LastAssistant -match 'red') 'the printable text of a colour-escaped reply is kept, not dropped wholesale'
Remove-Item -LiteralPath $escRoot -Recurse -Force -ErrorAction SilentlyContinue

# 16. The tail walk stopped only when it had counted $Count newlines, so a file with few newlines
#     was read end to end - measured 14 127 ms and 2 515 MB of managed heap to return ONE line from
#     a 200 MB newline-free transcript (adversarial review, 2026-09-08). A byte budget bounds it.
#     MaxBytes is injected here so the assertion costs a 1 MB fixture instead of a 200 MB one; the
#     shipped default is 4 MB.
$tailPath = Join-Path $env:TEMP "claude-auto-tail-$(Get-Random).txt"
[IO.File]::WriteAllText($tailPath, (('x' * 1MB) + "`n" + 'tail line' + "`n"), (New-Object System.Text.UTF8Encoding($false)))
$tailLines = @(Get-FileTailLines -Path $tailPath -Count 5 -MaxBytes 65536)
$longest = (@($tailLines | ForEach-Object { $_.Length }) | Measure-Object -Maximum).Maximum
Assert-Equal 'tail line' $tailLines[-1] 'the real last line still comes back with a byte budget in force'
# `-le` alone would pass on NOTHING coming back at all ($null -le 70000 is true in PowerShell,
# which is exactly what an unknown -MaxBytes parameter produces), so the bound is asserted together
# with "a line actually came back".
Assert-Equal $true ($longest -gt 0 -and $longest -le 70000) "the walk stops at the byte budget instead of reading the whole file (longest line returned: $longest)"
Remove-Item -LiteralPath $tailPath -Force -ErrorAction SilentlyContinue

# 17. ConvertFrom-ClaudeProjectSlug's 'Projects-' heuristic renders a repo not
#     living under a folder called Projects wrong (e.g. 'my-cool-app' as just 'app'). The
#     transcript's own cwd is the reversible authority - Get-ClaudeSessionSummary must resolve the
#     name from it and carry the raw Slug alongside, and Get-ClaudeSessions must be filterable by
#     that slug. Called WITHOUT -ProjectPath, so the standalone (no-cache) path is covered too.
#     (Review round 2: the resolved cwd is kept only as the -ProjectPath PARAMETER used to compute
#     the name - it is not returned as a field. A path is a machine value: sanitising it corrupts a
#     genuine one - e.g. two consecutive spaces, or U+00AD, are legal in an NTFS path component and
#     not in $ctrl's pattern's complement - while a raw one is an injection vector with no consumer
#     to justify the risk. Get-ProjectRegistry is the one place that hands out a validated path.)
$sroot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$sdir  = Join-Path $sroot 'C--src-my-cool-app'
New-Item -ItemType Directory -Force -Path $sdir | Out-Null
Set-Content -LiteralPath (Join-Path $sdir 'x.jsonl') -Encoding utf8 -Value (
    @{ type='user'; cwd='C:\src\my-cool-app'; message=@{ role='user'; content='hello' } } | ConvertTo-Json -Compress)

$s = Get-ClaudeSessionSummary -Path (Join-Path $sdir 'x.jsonl')
Assert-Equal 'my-cool-app' $s.Project 'the project name is the real folder, not the last dash token'
Assert-Equal 'C--src-my-cool-app' $s.Slug 'the slug travels with the session'

$all = @(Get-ClaudeSessions -ProjectsRoot $sroot -CachePath (Join-Path $sroot 'c.json'))
Assert-Equal 1 $all.Count 'the fixture has exactly one session, pinned independently of the filter under test'
$one = @(Get-ClaudeSessions -ProjectsRoot $sroot -CachePath (Join-Path $sroot 'c.json') -ProjectSlug 'C--src-my-cool-app')
Assert-Equal $all.Count $one.Count 'filtering by the only slug returns everything'
$none = @(Get-ClaudeSessions -ProjectsRoot $sroot -CachePath (Join-Path $sroot 'c.json') -ProjectSlug 'C--other')
Assert-Equal 0 $none.Count 'filtering by an absent slug returns nothing'
Remove-Item -LiteralPath $sroot -Recurse -Force -ErrorAction SilentlyContinue

# 17b. Review round 1, CRITICAL: Project is now sourced from the transcript's own cwd (untrusted
#      input), and the protection it had before this task was structural (an NTFS directory name
#      cannot carry 0x00-0x1F) - a cwd taken from JSON has no such guarantee. It must be sanitised
#      exactly like Title/LastUser/LastAssistant. (Round 2 dropped the ProjectPath FIELD entirely -
#      see the section-17 comment - so only Project is asserted here now.)
$escProjRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-escproj-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$escProjDir  = Join-Path $escProjRoot 'C--src-esc-app'
New-Item -ItemType Directory -Force -Path $escProjDir | Out-Null
$escP = [char]27
Set-Content -LiteralPath (Join-Path $escProjDir 'x.jsonl') -Encoding utf8 -Value (
    @{ type='user'; cwd=("C:\src\pwn" + $escP + "ed"); message=@{ role='user'; content='hello' } } | ConvertTo-Json -Compress)
$sEsc = Get-ClaudeSessionSummary -Path (Join-Path $escProjDir 'x.jsonl')
Assert-Equal $false ($sEsc.Project -match $ctrl) 'no control character survives into Project'
Remove-Item -LiteralPath $escProjRoot -Recurse -Force -ErrorAction SilentlyContinue

# 17c. Review round 1, IMPORTANT: a cached summary must not keep serving a project name the
#      directory has since moved on from. The resolved cwd is decided by whichever transcript is
#      newest in the directory RIGHT NOW - a different file than the one a given cache key names.
#      Session 'aaa' is cached with no cwd (heuristic fallback); a newer sibling then arrives with a
#      real cwd, and both must report the real name on the next listing, not just the fresh one.
$stRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-stale-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$stDir  = Join-Path $stRoot 'C--src-stale-app'
New-Item -ItemType Directory -Force -Path $stDir | Out-Null
Set-Content -LiteralPath (Join-Path $stDir 'aaa.jsonl') -Encoding utf8 -Value (
    @{ type='user'; message=@{ role='user'; content='first' } } | ConvertTo-Json -Compress)   # no cwd
$stCache = Join-Path $stRoot 'c.json'
$b1 = @(Get-ClaudeSessions -ProjectsRoot $stRoot -CachePath $stCache)
Assert-Equal 'app' $b1[0].Project 'before a real cwd exists anywhere in the directory, the lone session falls back to the slug heuristic'

Start-Sleep -Milliseconds 20   # aaa.jsonl must not become the newest transcript by mtime tie
Set-Content -LiteralPath (Join-Path $stDir 'bbb.jsonl') -Encoding utf8 -Value (
    @{ type='user'; cwd='C:\src\new-name'; message=@{ role='user'; content='second' } } | ConvertTo-Json -Compress)
$b2 = @(Get-ClaudeSessions -ProjectsRoot $stRoot -CachePath $stCache)
$aaaSummary = $b2 | Where-Object { $_.SessionId -eq 'aaa' }
$bbbSummary = $b2 | Where-Object { $_.SessionId -eq 'bbb' }
Assert-Equal 'new-name' $aaaSummary.Project 'a CACHED session refreshes its project name from the directory, not the value baked in at first summarisation'
Assert-Equal 'new-name' $bbbSummary.Project 'the freshly summarised sibling reports the same real name'
Remove-Item -LiteralPath $stRoot -Recurse -Force -ErrorAction SilentlyContinue

# 17d. Review round 1, IMPORTANT: a -ProjectSlug call only ever enumerates one project's files, so
#      writing $fresh alone as the cache would evict every other project's entry from the shared
#      file on the very next launch.
$mgRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-merge-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$mgA = Join-Path $mgRoot 'C--src-proj-a'; $mgB = Join-Path $mgRoot 'C--src-proj-b'
New-Item -ItemType Directory -Force -Path $mgA | Out-Null
New-Item -ItemType Directory -Force -Path $mgB | Out-Null
Set-Content -LiteralPath (Join-Path $mgA 'a1.jsonl') -Encoding utf8 -Value (@{ type='user'; message=@{ role='user'; content='a' } } | ConvertTo-Json -Compress)
Set-Content -LiteralPath (Join-Path $mgB 'b1.jsonl') -Encoding utf8 -Value (@{ type='user'; message=@{ role='user'; content='b' } } | ConvertTo-Json -Compress)
$mgCache = Join-Path $mgRoot 'c.json'
$null = @(Get-ClaudeSessions -ProjectsRoot $mgRoot -CachePath $mgCache)
$before = @((Get-Content -LiteralPath $mgCache -Raw | ConvertFrom-Json).PSObject.Properties).Count
Assert-Equal 2 $before 'both projects are cached after an unfiltered call'
$null = @(Get-ClaudeSessions -ProjectsRoot $mgRoot -CachePath $mgCache -ProjectSlug 'C--src-proj-a')
$after = @((Get-Content -LiteralPath $mgCache -Raw | ConvertFrom-Json).PSObject.Properties).Count
Assert-Equal 2 $after 'a scoped call merges onto the existing cache instead of evicting the other project'
Remove-Item -LiteralPath $mgRoot -Recurse -Force -ErrorAction SilentlyContinue

# 17e. Review round 1, MINOR: the once-per-directory design point has no coverage without counting
#      actual resolver calls - a single-file fixture cannot tell one resolution from N. Wrap the
#      resolver to count invocations across three sibling session files in one directory.
$origResolve = ${function:Get-ProjectPathFromTranscript}
$script:ResolveCalls = 0
function Get-ProjectPathFromTranscript {
    param([string]$Directory)
    $script:ResolveCalls++
    & $origResolve -Directory $Directory
}
$mdRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-md-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$mdDir  = Join-Path $mdRoot 'C--src-multi-dir'
New-Item -ItemType Directory -Force -Path $mdDir | Out-Null
1..3 | ForEach-Object {
    Set-Content -LiteralPath (Join-Path $mdDir "s$_.jsonl") -Encoding utf8 -Value (
        @{ type='user'; cwd='C:\src\multi-dir'; message=@{ role='user'; content="hello $_" } } | ConvertTo-Json -Compress)
}
$mdSessions = @(Get-ClaudeSessions -ProjectsRoot $mdRoot -CachePath (Join-Path $mdRoot 'c.json'))
Assert-Equal 3 $mdSessions.Count 'three sibling sessions in one directory are all listed'
Assert-Equal 1 $script:ResolveCalls 'the directory resolves its project path exactly once on a cold cache, not once per session'
${function:Get-ProjectPathFromTranscript} = $origResolve
Remove-Item -LiteralPath $mdRoot -Recurse -Force -ErrorAction SilentlyContinue

# 17f. Review round 2, IMPORTANT: a FAILING directory resolution must also memoise, not just a
#      successful one - otherwise a throwing resolver is retried once per file in that directory,
#      undoing the once-per-directory guarantee from 17e under exactly the condition it exists to
#      protect against (measured before this fix: 3 files -> 3 invocations AND 0 rows returned,
#      because the unmemoised throw took the whole per-file try down with it).
$origResolve2 = ${function:Get-ProjectPathFromTranscript}
$script:ResolveCalls = 0
function Get-ProjectPathFromTranscript {
    param([string]$Directory)
    $script:ResolveCalls++
    throw 'resolver unavailable'
}
$failRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-fail-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$failDir  = Join-Path $failRoot 'C--src-failing-dir'
New-Item -ItemType Directory -Force -Path $failDir | Out-Null
1..3 | ForEach-Object {
    Set-Content -LiteralPath (Join-Path $failDir "f$_.jsonl") -Encoding utf8 -Value (
        @{ type='user'; message=@{ role='user'; content="hello $_" } } | ConvertTo-Json -Compress)
}
$failSessions = @(Get-ClaudeSessions -ProjectsRoot $failRoot -CachePath (Join-Path $failRoot 'c.json'))
Assert-Equal 1 $script:ResolveCalls 'a throwing resolver is invoked once per directory, not once per session, even on failure'
Assert-Equal 3 $failSessions.Count 'a directory whose resolver fails still lists every session in it, falling back to the slug heuristic'
${function:Get-ProjectPathFromTranscript} = $origResolve2
Remove-Item -LiteralPath $failRoot -Recurse -Force -ErrorAction SilentlyContinue

# 18. The summary parsed every head and tail line with ConvertFrom-Json, which was 69% of a cold
#     listing. A line that does not carry '"type":"user"' can never produce a title or a LastUser
#     (Get-ClaudeUserPrompt returns $null on any other type), and a tail line carrying neither
#     '"type":"user"' nor '"type":"assistant"' can contribute to nothing at all - so those parses
#     are pure cost. Skipping them must change how MANY lines are parsed and nothing else, which is
#     only assertable while the unfiltered walk is still runnable: -NoPreFilter keeps it.
function Get-SummaryFingerprint {
    param($S)
    $recent = @($S.RecentMessages | ForEach-Object { "$($_.Speaker)=$($_.Text)" }) -join '|'
    return (@(
        "SessionId=$($S.SessionId)", "Path=$($S.Path)", "Slug=$($S.Slug)", "Project=$($S.Project)",
        "Worktree=$($S.Worktree)", "Modified=$($S.Modified.Ticks)", "SizeBytes=$($S.SizeBytes)",
        "PromptCount=$($S.PromptCount)", "Title=$($S.Title)", "LastUser=$($S.LastUser)",
        "LastAssistant=$($S.LastAssistant)", "Recent=$recent"
    ) -join "`n")
}
$fixtureFiles = @(Get-ChildItem -LiteralPath $fx -Recurse -Filter *.jsonl -File)
# A comparison over an empty set passes for the wrong reason.
Assert-Equal $true ($fixtureFiles.Count -ge 9) "the fixture tree carries transcripts to compare ($($fixtureFiles.Count) found)"
$drifted = @()
foreach ($ff in $fixtureFiles) {
    $unfiltered = Get-SummaryFingerprint (Get-ClaudeSessionSummary -Path $ff.FullName -NoPreFilter)
    $filtered   = Get-SummaryFingerprint (Get-ClaudeSessionSummary -Path $ff.FullName)
    if ($unfiltered -ne $filtered) { $drifted += $ff.Name }
}
Assert-Equal 0 $drifted.Count "every summary field is identical with and without the line pre-filter (differing: $($drifted -join ', '))"

# And it really does skip parses - otherwise the assertion above passes on a filter that does
# nothing. None of the fixtures above can show it: every one of them is user and assistant records
# only, which is exactly the set the filter keeps. What a real transcript is full of - system,
# summary and hook records between the prompts - needs its own fixture, written here so the shape
# under test is visible beside the assertion.
$pfRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-prefilter-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$pfDir  = Join-Path $pfRoot 'C--src-prefilter'
New-Item -ItemType Directory -Force -Path $pfDir | Out-Null
$pfFile = Join-Path $pfDir 'pppp4444.jsonl'
$pfLines = @(
    '{"type":"system","subtype":"init","content":"boot"}',
    '{"type":"assistant","message":{"content":[{"type":"text","text":"noise before the prompt"}]}}',
    '{"type":"summary","summary":"an earlier session"}',
    '{"type":"user","message":{"content":"the real prompt"}}',
    '{"type":"assistant","message":{"content":[{"type":"text","text":"done"}]}}',
    '{"type":"system","subtype":"hook","content":"post"}'
)
[IO.File]::WriteAllText($pfFile, (($pfLines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))

$origConvert = ${function:ConvertFrom-JsonlLine}
$script:ParseCalls = 0
function ConvertFrom-JsonlLine {
    param([string]$Line)
    $script:ParseCalls++
    & $origConvert -Line $Line
}
$script:ParseCalls = 0; $sUnfiltered = Get-ClaudeSessionSummary -Path $pfFile -NoPreFilter
$parsesUnfiltered = $script:ParseCalls
$script:ParseCalls = 0; $sFiltered = Get-ClaudeSessionSummary -Path $pfFile
$parsesFiltered = $script:ParseCalls
${function:ConvertFrom-JsonlLine} = $origConvert
Assert-Equal $true ($parsesFiltered -gt 0 -and $parsesFiltered -lt $parsesUnfiltered) "the pre-filter parses fewer lines than the unfiltered walk ($parsesFiltered vs $parsesUnfiltered)"
Assert-Equal 'the real prompt' $sFiltered.Title 'and the filtered walk still finds the same title past the system and summary records'
Assert-Equal (Get-SummaryFingerprint $sUnfiltered) (Get-SummaryFingerprint $sFiltered) 'a transcript carrying system and summary records summarises identically either way'
Remove-Item -LiteralPath $pfRoot -Recurse -Force -ErrorAction SilentlyContinue

# 19. The cache version was the module's own LAST WRITE TIME, which moves without a byte of the
#     module changing: a clone, a checkout, a profile relink or a copy between the four account
#     roots all rewrite the stamp, and every cached summary on the machine is thrown away for
#     nothing. The module's CONTENT hash invalidates on exactly the thing that matters - an edit to
#     the summarising logic - and on nothing else.
$modPath = "$PSScriptRoot\..\claude-auto\Sessions.ps1"
function Get-ModuleContentVersion {
    param([string]$P)
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([IO.File]::ReadAllBytes($P))).Substring(0, 16)
}
Assert-Equal (Get-ModuleContentVersion $modPath) $script:SummaryVersion 'the cache version is the module CONTENT hash, not a timestamp'

$vRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-ver-" + [guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Force -Path $vRoot | Out-Null
$vCopy = Join-Path $vRoot 'Sessions.ps1'
Copy-Item -LiteralPath $modPath -Destination $vCopy
$vBefore = Get-ModuleContentVersion $vCopy
(Get-Item -LiteralPath $vCopy).LastWriteTimeUtc = (Get-Date).AddDays(1).ToUniversalTime()
Assert-Equal $vBefore (Get-ModuleContentVersion $vCopy) 'touching the module - a checkout, a relink - leaves the version alone'
[IO.File]::AppendAllText($vCopy, "`n# a real edit`n")
Assert-Equal $false ($vBefore -eq (Get-ModuleContentVersion $vCopy)) 'and an actual edit to the module still changes it'
Remove-Item -LiteralPath $vRoot -Recurse -Force -ErrorAction SilentlyContinue

# 20. The four account roots on this machine reach ONE projects directory through a junction, so a
#     cache keyed on whichever root the caller named is built cold four times over the same
#     physical files. Key it on the resolved physical directory instead.
$jRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-junc-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$jPhysicalHome = Join-Path $jRoot 'physical'
$jPhysical = Join-Path $jPhysicalHome 'projects'
New-Item -ItemType Directory -Force -Path $jPhysical | Out-Null
$jA = Join-Path $jRoot 'accountA'; $jB = Join-Path $jRoot 'accountB'
New-Item -ItemType Directory -Force -Path $jA | Out-Null
New-Item -ItemType Directory -Force -Path $jB | Out-Null
$jAProjects = Join-Path $jA 'projects'; $jBProjects = Join-Path $jB 'projects'
New-Item -ItemType Junction -Path $jAProjects -Target $jPhysical | Out-Null
New-Item -ItemType Junction -Path $jBProjects -Target $jPhysical | Out-Null

Assert-Equal (Join-Path $jPhysicalHome 'claude-auto-sessions.json') (Get-SessionsCachePath -ProjectsRoot $jAProjects) 'a junctioned root caches beside the PHYSICAL projects directory, not beside the junction'
Assert-Equal (Get-SessionsCachePath -ProjectsRoot $jAProjects) (Get-SessionsCachePath -ProjectsRoot $jBProjects) 'two account roots junctioned to one projects directory resolve to ONE cache path'
# A root that is not a link keeps exactly the behaviour this replaces - section 14 pins that each
# real account root still gets its own file.
$plainProjects = Join-Path $jRoot 'plain\projects'
New-Item -ItemType Directory -Force -Path $plainProjects | Out-Null
Assert-Equal (Join-Path $jRoot 'plain\claude-auto-sessions.json') (Get-SessionsCachePath -ProjectsRoot $plainProjects) 'a root that is not a junction still caches beside itself'

# 21. End to end: the second account must READ what the first one wrote instead of summarising the
#     same files again. Counting summarisations is the only way to tell a shared cache from two
#     caches that happen to hold the same rows.
New-Item -ItemType Directory -Force -Path (Join-Path $jPhysical 'C--src-shared-app') | Out-Null
Set-Content -LiteralPath (Join-Path $jPhysical 'C--src-shared-app\ssss5555.jsonl') -Encoding utf8 -Value (
    @{ type='user'; cwd='C:\src\shared-app'; message=@{ role='user'; content='shared prompt' } } | ConvertTo-Json -Compress)

$origSummary = ${function:Get-ClaudeSessionSummary}
$script:SummaryCalls = 0
function Get-ClaudeSessionSummary {
    param([Parameter(Mandatory)][string]$Path, [int]$HeadLines = 400, [int]$TailLines = 120,
          [string]$ProjectPath = $null, [switch]$NoPreFilter)
    $script:SummaryCalls++
    & $origSummary @PSBoundParameters
}
$jFirst = @(Get-ClaudeSessions -ProjectsRoot $jAProjects)
$callsAfterFirst = $script:SummaryCalls
$jSecond = @(Get-ClaudeSessions -ProjectsRoot $jBProjects)
$callsAfterSecond = $script:SummaryCalls
${function:Get-ClaudeSessionSummary} = $origSummary
Assert-Equal 1 $callsAfterFirst 'the first account summarises the one transcript cold'
Assert-Equal $callsAfterFirst $callsAfterSecond 'the second account root reads the first one''s cache instead of summarising again'
Assert-Equal $jFirst[0].Title $jSecond[0].Title 'and gets the same row back'

# 22. That shared file is now written by up to four launcher instances at once, so a truncate-in-
#     place write can hand a concurrent reader half a file. The write goes to a sibling temp and is
#     moved over, which is atomic on NTFS and leaves nothing behind.
$jArtefacts = @(Get-ChildItem -LiteralPath $jPhysicalHome -File | Where-Object { $_.Name -like 'claude-auto-sessions*' })
Assert-Equal 'claude-auto-sessions.json' (@($jArtefacts | ForEach-Object { $_.Name }) -join ',') 'the cache write leaves exactly the cache file - no temp sibling stranded beside it'
Remove-Item -LiteralPath $jAProjects -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $jBProjects -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $jRoot -Recurse -Force -ErrorAction SilentlyContinue

# 23. Paging. Summarising 40 transcripts before the picker can draw anything is what a cold launch
#     pays for; the picker now draws its first page and asks for the next only when the cursor
#     reaches the last row, so this function has to hand out a WINDOW of the same newest-first
#     order. The pages laid end to end must be exactly the unpaged listing - a duplicate row would
#     offer the same session twice, a gap would hide one - and -Skip has to compose with the
#     project scope, which is the only way the picker is ever called from the project screen.
$pgRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-page-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$pgA = Join-Path $pgRoot 'C--src-page-a'; $pgB = Join-Path $pgRoot 'C--src-page-b'
New-Item -ItemType Directory -Force -Path $pgA | Out-Null
New-Item -ItemType Directory -Force -Path $pgB | Out-Null
# Distinct mtimes, set explicitly: the order under test IS the mtime order, and files written in a
# loop can share a timestamp, which would make the page boundaries arbitrary.
$pgBase = Get-Date '2026-09-01 12:00:00'
foreach ($n in 1..5) {
    $p = Join-Path $pgA "a$n.jsonl"
    Set-Content -LiteralPath $p -Encoding utf8 -Value (@{ type='user'; cwd='C:\src\page-a'; message=@{ role='user'; content="prompt a$n" } } | ConvertTo-Json -Compress)
    (Get-Item -LiteralPath $p).LastWriteTime = $pgBase.AddMinutes($n)
}
foreach ($n in 1..3) {
    $p = Join-Path $pgB "b$n.jsonl"
    Set-Content -LiteralPath $p -Encoding utf8 -Value (@{ type='user'; cwd='C:\src\page-b'; message=@{ role='user'; content="prompt b$n" } } | ConvertTo-Json -Compress)
    (Get-Item -LiteralPath $p).LastWriteTime = $pgBase.AddMinutes(10 + $n)
}
$pgCache = Join-Path $pgRoot 'c.json'
$pgAll = @(Get-ClaudeSessions -ProjectsRoot $pgRoot -CachePath $pgCache -Limit 100 -ProjectSlug 'C--src-page-a')
Assert-Equal 5 $pgAll.Count 'the scoped project has five sessions when nothing is paged'
$pg1 = @(Get-ClaudeSessions -ProjectsRoot $pgRoot -CachePath $pgCache -Limit 2 -Skip 0 -ProjectSlug 'C--src-page-a')
$pg2 = @(Get-ClaudeSessions -ProjectsRoot $pgRoot -CachePath $pgCache -Limit 2 -Skip 2 -ProjectSlug 'C--src-page-a')
$pg3 = @(Get-ClaudeSessions -ProjectsRoot $pgRoot -CachePath $pgCache -Limit 2 -Skip 4 -ProjectSlug 'C--src-page-a')
$pg4 = @(Get-ClaudeSessions -ProjectsRoot $pgRoot -CachePath $pgCache -Limit 2 -Skip 6 -ProjectSlug 'C--src-page-a')
Assert-Equal 2 $pg1.Count 'the first page is one -Limit worth'
Assert-Equal 2 $pg2.Count 'and so is the second'
Assert-Equal 1 $pg3.Count 'the last page is whatever is left'
Assert-Equal 0 $pg4.Count 'and past the end there is nothing, rather than a wrap back to the start'
$pgPaged = @(@($pg1) + @($pg2) + @($pg3))
Assert-Equal (@($pgAll | ForEach-Object { $_.SessionId }) -join ',') (@($pgPaged | ForEach-Object { $_.SessionId }) -join ',') 'the pages laid end to end are exactly the unpaged listing - same order, no row twice, none missed'
Assert-Equal 'C--src-page-a' ((@($pgPaged | ForEach-Object { $_.Slug }) | Select-Object -Unique) -join ',') 'every paged row is still inside the scoped project'

# A paged call has only LOOKED at a window, so it must not prune the cache down to that window -
# the unfiltered whole-file replace is what prunes vanished sessions, and a page is not a survey.
$null = @(Get-ClaudeSessions -ProjectsRoot $pgRoot -CachePath $pgCache -Limit 100)
$pgKeysBefore = @((Get-Content -LiteralPath $pgCache -Raw | ConvertFrom-Json).PSObject.Properties).Count
Assert-Equal 8 $pgKeysBefore 'an unpaged unscoped call caches every session in the root'
$null = @(Get-ClaudeSessions -ProjectsRoot $pgRoot -CachePath $pgCache -Limit 2 -Skip 4)
$pgKeysAfter = @((Get-Content -LiteralPath $pgCache -Raw | ConvertFrom-Json).PSObject.Properties).Count
Assert-Equal $pgKeysBefore $pgKeysAfter 'a paged call merges onto the cache instead of evicting everything it did not look at'
Remove-Item -LiteralPath $pgRoot -Recurse -Force -ErrorAction SilentlyContinue

# 24. The prompt count streamed the WHOLE file, and the largest transcript on this machine is
#     48 MB - measured at a quarter of a cold listing. Same bound and same reason as
#     Get-FileTailLines: a walk must be limited by BYTES, not by what it is looking for. A file
#     inside the budget keeps its exact count, because "25 msgs" is the number the picker shows;
#     past it the count is reported as "N+", which is true, where a silently short number is a lie.
$capRoot = Join-Path ([IO.Path]::GetTempPath()) ("cap-sess-count-" + [guid]::NewGuid().ToString('N').Substring(0,8))
$capDir  = Join-Path $capRoot 'C--src-big-app'
New-Item -ItemType Directory -Force -Path $capDir | Out-Null
$capHead = @(
    '{"type":"user","message":{"content":"prompt one"}}',
    '{"type":"user","message":{"content":"prompt two"}}',
    '{"type":"user","message":{"content":"prompt three"}}'
)
# Five megabytes of assistant padding, past the shipped 4 MB budget, with two more real prompts
# behind it - so a bound that is not in force is visible as a different number, not as a pass.
$capPad = '{"type":"assistant","message":{"content":[{"type":"text","text":"' + ('p' * 1MB) + '"}]}}'
$capTail = @(
    '{"type":"user","message":{"content":"prompt four"}}',
    '{"type":"user","message":{"content":"prompt five"}}'
)
$capBig = Join-Path $capDir 'gggg7777.jsonl'
[IO.File]::WriteAllText($capBig, ((@($capHead) + @($capPad, $capPad, $capPad, $capPad, $capPad) + @($capTail)) -join "`n") + "`n", (New-Object System.Text.UTF8Encoding($false)))
$capSmall = Join-Path $capDir 'hhhh8888.jsonl'
[IO.File]::WriteAllText($capSmall, (($capHead -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))

Assert-Equal 3 (Measure-ClaudePrompts -Path $capBig) 'past the SHIPPED byte budget the count stops'
Assert-Equal $true (Measure-ClaudePromptDetail -Path $capBig).Capped 'and says so through a FLAG, not by turning the number into a string'
Assert-Equal $true ((Measure-ClaudePrompts -Path $capBig) -is [int]) 'the count is always an int, so a threshold or an ordering on it is numeric'
Assert-Equal 5 (Measure-ClaudePrompts -Path $capBig -MaxBytes 16MB) 'given room for the whole file the count is exact again - the bound is what stopped it, not a miscount'
Assert-Equal 3 (Measure-ClaudePrompts -Path $capSmall) 'a file inside the budget keeps its exact count, which is what the picker displays'
Assert-Equal $false (Measure-ClaudePromptDetail -Path $capSmall).Capped 'and is not marked capped'
$sCap = Get-ClaudeSessionSummary -Path $capBig
Assert-Equal 3 $sCap.PromptCount 'the summary carries the number'
Assert-Equal $true $sCap.PromptCountCapped 'and the capped flag beside it'
Assert-Equal '3+' (Format-PromptCount -Session $sCap) 'which is what the picker renders as "N+"'
Assert-Equal '3' (Format-PromptCount -Session (Get-ClaudeSessionSummary -Path $capSmall)) 'an uncapped count renders as a plain number'

# --- the cap BOUNDARY, both ends of it -------------------------------------------------------------
# Test 24 above uses a ~5 MB file against a 4 MB budget: it cannot tell -gt from -ge at either end.
# These fixtures sit exactly ON the boundary. Each prompt line below is exactly 50 bytes with its
# newline, so four of them are exactly 200.
$bnDir = Join-Path $env:TEMP ('claude-auto-bound-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force -Path $bnDir | Out-Null
$bnLine = { param([int]$I) '{"type":"user","message":{"content":"prompt-' + ('{0:00}' -f $I) + '"}}' }
$bnWrite = { param([string]$Name, [int]$Count)
    $p = Join-Path $bnDir $Name
    [IO.File]::WriteAllText($p, (@(1..$Count | ForEach-Object { & $bnLine $_ }) -join "`n") + "`n", (New-Object System.Text.UTF8Encoding($false)))
    return $p
}
$bnExact = & $bnWrite 'exact.jsonl' 4
Assert-Equal 200 (Get-Item -LiteralPath $bnExact).Length 'the boundary fixture is exactly the budget, to the byte'
$bnExactR = Measure-ClaudePromptDetail -Path $bnExact -MaxBytes 200
Assert-Equal 4 $bnExactR.Count 'a file exactly the size of the budget is read whole'
Assert-Equal $false $bnExactR.Capped 'and is not reported as capped - the bound engages ABOVE the budget, not at it'
$bnOver = & $bnWrite 'over.jsonl' 5
$bnOverR = Measure-ClaudePromptDetail -Path $bnOver -MaxBytes 200
Assert-Equal $true $bnOverR.Capped 'one byte over the budget and the bound does engage'
Assert-Equal 4 $bnOverR.Count 'and it stops on the line that reaches the budget exactly, not one line later'
# A record that BEGINS inside the budget and ends past it is counted: the budget is spent after the
# line is examined, not charged blind before it.
$bnStr = Join-Path $bnDir 'straddle.jsonl'
[IO.File]::WriteAllText($bnStr,
    ('{"type":"assistant","message":{"content":[{"type":"text","text":"' + ('p' * 84) + '"}]}}' + "`n" +
     '{"type":"user","message":{"content":"' + ('q' * 60) + '"}}' + "`n"),
    (New-Object System.Text.UTF8Encoding($false)))
$bnStrR = Measure-ClaudePromptDetail -Path $bnStr -MaxBytes 200
Assert-Equal 1 $bnStrR.Count 'a prompt whose line begins inside the budget is counted even though it ends past it'
Assert-Equal $true $bnStrR.Capped 'and the walk stops there'
# BYTES, not characters: the same prompt text in Cyrillic must stop at the same BYTE count.
$bnRu = [char]0x043F
$bnCyr = Join-Path $bnDir 'cyr.jsonl'
[IO.File]::WriteAllText($bnCyr, (@(1..40 | ForEach-Object { '{"type":"user","message":{"content":"' + ([string]$bnRu * 50) + '"}}' }) -join "`n") + "`n", (New-Object System.Text.UTF8Encoding($false)))
$bnCyrR = Measure-ClaudePromptDetail -Path $bnCyr -MaxBytes 1000
# Each line is 141 bytes and 91 characters, so a BYTE budget of 1000 stops after 8 records where a
# CHARACTER budget reads 11 - 1.55x the bytes it promised, and 1.94x was measured on real text.
Assert-Equal 8 $bnCyrR.Count 'the budget is counted in BYTES: a Cyrillic transcript stops after 8 records, not the 11 a character count would have read'
Remove-Item -LiteralPath $bnDir -Recurse -Force -ErrorAction SilentlyContinue

# --- the count agrees with its own authority -------------------------------------------------------
# Every rejection rule lives in Get-ClaudeUserPrompt. A private set of substrings beside it drifted:
# a wrapper delivered as a content ARRAY was counted as a human prompt while the title column said
# "(no prompt)" about the same session, and a prompt written as '{"type": "user"' was dropped
# (adversarial review 2026-09-16, A3).
$auDir = Join-Path $env:TEMP ('claude-auto-authority-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force -Path $auDir | Out-Null
$auCases = [ordered]@{
    'a plain string prompt'                   = '{"type":"user","message":{"role":"user","content":"fix the build"}}'
    'a tool result wearing the user role'     = '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}'
    'a sidechain user record'                 = '{"type":"user","isSidechain":true,"message":{"role":"user","content":"subagent chatter"}}'
    'a meta user record'                      = '{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}'
    'a system-reminder as string content'     = '{"type":"user","message":{"role":"user","content":"<system-reminder>noise</system-reminder>"}}'
    'a system-reminder as a text BLOCK'       = '{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<system-reminder>noise</system-reminder>"}]}}'
    'a local-command-stdout as a text BLOCK'  = '{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<local-command-stdout>build output</local-command-stdout>"}]}}'
    'a bare /clear as a text BLOCK'           = '{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<command-name>/clear</command-name><command-args></command-args>"}]}}'
    'a real prompt in non-compact JSON'       = '{"type": "user", "message": {"role": "user", "content": "a real prompt"}}'
    'an empty text block array'               = '{"type":"user","message":{"role":"user","content":[]}}'
    'an assistant record'                     = '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]}}'
    # Two shapes the parse-free shortcut used to take on its own (re-review 2026-09-16, W2).
    # Whitespace-only content is the B3 symptom exactly: the authority calls it no prompt
    # (IsNullOrWhiteSpace), so a session with nothing to resume into was offered as resumable.
    'whitespace-only string content'          = '{"type":"user","message":{"role":"user","content":"   "}}'
    # A toolUseResult sibling carries a "content":"..." of its own, beside a message whose real
    # content is a noise text BLOCK.
    'a toolUseResult sibling beside a noise text block' = '{"type":"user","toolUseResult":{"content":"tool output here"},"message":{"role":"user","content":[{"type":"text","text":"<system-reminder>noise</system-reminder>"}]}}'
    # The control for both: the shortcut must still fire on an ordinary prompt, or the agreement
    # above is bought by parsing everything.
    'a plain prompt after a toolUseResult sibling' = '{"type":"user","toolUseResult":{"content":"tool output here"},"message":{"role":"user","content":"fix the build"}}'
}
$auI = 0
foreach ($auName in $auCases.Keys) {
    $auI++
    $auFile = Join-Path $auDir "case$auI.jsonl"
    [IO.File]::WriteAllText($auFile, $auCases[$auName] + "`n", (New-Object System.Text.UTF8Encoding($false)))
    $auRec = ConvertFrom-JsonlLine -Line $auCases[$auName]
    $auAuthority = if ($auRec -and (Get-ClaudeUserPrompt -Record $auRec)) { 1 } else { 0 }
    Assert-Equal $auAuthority (Measure-ClaudePrompts -Path $auFile) "the count agrees with Get-ClaudeUserPrompt - $auName"
}
# A prompt that QUOTES the counter's own rejection substrings in its text is still a prompt: the
# structural patterns only match a quote that is not escaped.
$auQuote = Join-Path $auDir 'quoting.jsonl'
[IO.File]::WriteAllText($auQuote, '{"type":"user","message":{"role":"user","content":"why is ' + [char]92 + '"tool_result' + [char]92 + '" in the docs"}}' + "`n", (New-Object System.Text.UTF8Encoding($false)))
Assert-Equal 1 (Measure-ClaudePrompts -Path $auQuote) 'a prompt quoting "tool_result" in its own text is still counted'
Remove-Item -LiteralPath $auDir -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $capRoot -Recurse -Force -ErrorAction SilentlyContinue

# --- the paging SNAPSHOT --------------------------------------------------------------------------
# -Skip over a listing re-sorted by mtime on every call loses a row the moment a live session
# appends between two pages: the window slides down, the duplicate is caught by the picker's dedup
# and the GAP is not. Get-ClaudeSessionFile takes the order ONCE and -Files pages exactly that.
$snapRoot = Join-Path $env:TEMP ('claude-auto-snap-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$snapDir = Join-Path $snapRoot 'C--src-shift'
New-Item -ItemType Directory -Force -Path $snapDir | Out-Null
$snapBase = Get-Date '2026-09-01 12:00:00'
foreach ($n in 1..6) {
    $sp = Join-Path $snapDir "s$n.jsonl"
    [IO.File]::WriteAllText($sp, '{"type":"user","cwd":"C:\\src\\shift","message":{"role":"user","content":"prompt s' + $n + '"}}' + "`n", (New-Object System.Text.UTF8Encoding($false)))
    (Get-Item -LiteralPath $sp).LastWriteTime = $snapBase.AddMinutes($n)
}
$snapCache = Join-Path $snapRoot 'c.json'
$snapExpected = @(Get-ClaudeSessions -ProjectsRoot $snapRoot -CachePath $snapCache -Limit 100 | ForEach-Object SessionId)
Assert-Equal 's6,s5,s4,s3,s2,s1' ($snapExpected -join ',') 'the unpaged listing is newest-first'
$snapshot = @(Get-ClaudeSessionFile -ProjectsRoot $snapRoot)
Assert-Equal 6 $snapshot.Count 'the snapshot lists every transcript under the root'
Assert-Equal 's6.jsonl' (Split-Path -Path $snapshot[0] -Leaf) 'and hands them back newest-first, as paths'
Assert-Equal 6 (@(Get-ClaudeSessionFile -ProjectsRoot $snapRoot -ProjectSlug 'C--src-shift')).Count 'a scoped snapshot lists that project'
Assert-Equal 0 (@(Get-ClaudeSessionFile -ProjectsRoot $snapRoot -ProjectSlug 'C--src-absent')).Count 'a slug with no directory snapshots nothing'
Assert-Equal 0 (@(Get-ClaudeSessionFile -ProjectsRoot (Join-Path $snapRoot 'no-such-root'))).Count 'a missing root snapshots nothing rather than throwing'
$snapP1 = @(Get-ClaudeSessions -ProjectsRoot $snapRoot -CachePath $snapCache -Files $snapshot -Limit 2 -Skip 0)
# a live session appends between page 1 and page 2: the OLDEST transcript becomes the newest
(Get-Item -LiteralPath (Join-Path $snapDir 's1.jsonl')).LastWriteTime = $snapBase.AddMinutes(99)
$snapP2 = @(Get-ClaudeSessions -ProjectsRoot $snapRoot -CachePath $snapCache -Files $snapshot -Limit 2 -Skip 2)
$snapP3 = @(Get-ClaudeSessions -ProjectsRoot $snapRoot -CachePath $snapCache -Files $snapshot -Limit 2 -Skip 4)
$snapWalk = @(@($snapP1) + @($snapP2) + @($snapP3) | ForEach-Object SessionId)
Assert-Equal ($snapExpected -join ',') ($snapWalk -join ',') 'pages over the snapshot compose with no gap and no repeat, although a transcript was appended between two of them'
$snapGone = @($snapshot) + @(Join-Path $snapDir 'never-existed.jsonl')
Assert-Equal 6 (@(Get-ClaudeSessions -ProjectsRoot $snapRoot -CachePath $snapCache -Files $snapGone -Limit 100)).Count 'a transcript deleted since the snapshot was taken is dropped from the page, never fatal'
# PRESENCE, not emptiness: an empty snapshot is "this scope holds nothing", and falling through to
# the enumeration there is how a scoped picker was handed the whole account (re-review, C1).
$snapEmpty = [string[]]@()
Assert-Equal 0 (@(Get-ClaudeSessions -ProjectsRoot $snapRoot -CachePath $snapCache -Files $snapEmpty -Limit 100)).Count '-Files @() means the scope holds no transcripts, NOT that no snapshot was given'
Assert-Equal 6 (@(Get-ClaudeSessions -ProjectsRoot $snapRoot -CachePath $snapCache -Limit 100)).Count 'and the same call without -Files still enumerates the root'
# A snapshot already carries its scope, so a slug beside it is a contradiction, not a refinement.
$snapBoth = $false
try { $null = Get-ClaudeSessions -ProjectsRoot $snapRoot -CachePath $snapCache -Files $snapshot -ProjectSlug 'C--src-shift' } catch { $snapBoth = $true }
Assert-Equal $true $snapBoth '-ProjectSlug beside -Files is a parameter error, not a silently ignored argument'
# The scope is a LIST: one directory can own several slug folders.
Assert-Equal 6 (@(Get-ClaudeSessionFile -ProjectsRoot $snapRoot -ProjectSlug @('C--src-shift', 'C--src-absent'))).Count 'a multi-slug scope snapshots every one of its slug folders'
Assert-Equal 0 (@(Get-ClaudeSessionFile -ProjectsRoot $snapRoot -ProjectSlug @('C--src-absent', 'C--src-gone'))).Count 'and a scope whose slugs hold nothing snapshots nothing'
Remove-Item -LiteralPath $snapRoot -Recurse -Force -ErrorAction SilentlyContinue

# --- the shared cache: who may prune it, and what happens when it is busy --------------------------
$shRoot = Join-Path $env:TEMP ('claude-auto-share-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$shDir = Join-Path $shRoot 'C--src-share'
New-Item -ItemType Directory -Force -Path $shDir | Out-Null
foreach ($n in 1..6) {
    $sp = Join-Path $shDir "h$n.jsonl"
    [IO.File]::WriteAllText($sp, '{"type":"user","cwd":"C:\\src\\share","message":{"role":"user","content":"prompt h' + $n + '"}}' + "`n", (New-Object System.Text.UTF8Encoding($false)))
    (Get-Item -LiteralPath $sp).LastWriteTime = (Get-Date '2026-09-01 12:00:00').AddMinutes($n)
}
$shCache = Join-Path $shRoot 'c.json'
$shKeys = { @((Get-Content -LiteralPath $shCache -Raw | ConvertFrom-Json).PSObject.Properties).Count }
$null = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $shCache -Limit 100)
Assert-Equal 6 (& $shKeys) 'a survey - every transcript under the root - caches every session'
# The launcher's own call: -Limit 10 with no slug and no skip. It looked at a WINDOW, so it must not
# evict the rows it did not look at from a file all four accounts share.
$null = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $shCache -Limit 2)
Assert-Equal 6 (& $shKeys) 'a -Limit''ed call merges onto the cache instead of pruning it down to its own page'
$null = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $shCache -Limit 2 -Skip 2)
Assert-Equal 6 (& $shKeys) 'and so does a -Skip''ped page'
# and the point of that: page 2 is still warm after a fresh launch re-reads page 1
$shCalls = 0
$shOrig = ${function:Get-ClaudeSessionSummary}
function Get-ClaudeSessionSummary {
    param([Parameter(Mandatory)][string]$Path, [int]$HeadLines = 400, [int]$TailLines = 120, [string]$ProjectPath = $null, [switch]$NoPreFilter)
    $script:shCalls++; & $shOrig @PSBoundParameters
}
$script:shCalls = 0
$null = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $shCache -Limit 2)
$null = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $shCache -Limit 2 -Skip 2)
${function:Get-ClaudeSessionSummary} = $shOrig
Assert-Equal 0 $script:shCalls 'page 2 stays warm across a relaunch - nothing is re-summarised'
# A survey still prunes: that is what drops sessions that no longer exist.
Remove-Item -LiteralPath (Join-Path $shDir 'h1.jsonl') -Force
$null = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $shCache -Limit 100)
Assert-Equal 5 (& $shKeys) 'a survey still evicts the entries of transcripts that are gone'

# A temp stranded by a killed launcher is swept by the next write; a YOUNG one is another instance's
# work in flight and must survive.
$oldTmp = "$shCache.deadbeefdeadbeefdeadbeefdeadbeef.tmp"
[IO.File]::WriteAllText($oldTmp, '{"orphan":1}', (New-Object System.Text.UTF8Encoding($false)))
(Get-Item -LiteralPath $oldTmp).LastWriteTime = (Get-Date).AddMinutes(-5)
$youngTmp = "$shCache.00000000000000000000000000000000.tmp"
[IO.File]::WriteAllText($youngTmp, '{"inflight":1}', (New-Object System.Text.UTF8Encoding($false)))
$null = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $shCache -Limit 100)
Assert-Equal $false (Test-Path -LiteralPath $oldTmp) 'a temp stranded by a killed launcher is cleaned up by the next write'
Assert-Equal $true (Test-Path -LiteralPath $youngTmp) 'a temp younger than a minute is another instance mid-write and is left alone'
Remove-Item -LiteralPath $youngTmp -Force

# The write is a RENAME, not an overwrite-in-place. A hard link to the cache is the witness: a
# rename replaces the name and leaves the witness on the old content; a copy-over writes through it.
$hlRoot = Join-Path $env:TEMP ('claude-auto-hl-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force -Path $hlRoot | Out-Null
$hlCache = Join-Path $hlRoot 'c.json'
[IO.File]::WriteAllText($hlCache, '{"before":1}', (New-Object System.Text.UTF8Encoding($false)))
$hlWitness = Join-Path $hlRoot 'witness.json'
$null = New-Item -ItemType HardLink -Path $hlWitness -Target $hlCache
Write-SessionsCache -Path $hlCache -Entries @{ after = 1 }
Assert-Equal $true ([IO.File]::ReadAllText($hlCache) -match 'after') 'the cache write lands'
Assert-Equal '{"before":1}' ([IO.File]::ReadAllText($hlWitness)) 'and it lands as an atomic RENAME: a second link to the old file still holds the old bytes, which an overwrite-in-place could not leave'
Remove-Item -LiteralPath $hlRoot -Recurse -Force

# A cache entry with no Path property must still yield its row: `$o.Path = x` throws on a
# PSCustomObject that lacks it, and the enclosing catch dropped the session with no trace.
$npDir = Join-Path $shRoot 'C--src-nopath'
New-Item -ItemType Directory -Force -Path $npDir | Out-Null
$npFile = Join-Path $npDir 'nnnn9999.jsonl'
[IO.File]::WriteAllText($npFile, '{"type":"user","cwd":"C:\\src\\nopath","message":{"role":"user","content":"a prompt"}}' + "`n", (New-Object System.Text.UTF8Encoding($false)))
$npInfo = Get-Item -LiteralPath $npFile
$npKey = "$script:SummaryVersion|$($npInfo.FullName)|$($npInfo.LastWriteTimeUtc.Ticks)|$($npInfo.Length)"
$npEntry = [ordered]@{ SessionId = 'nnnn9999'; Slug = 'C--src-nopath'; Project = 'nopath'; Worktree = $null
                       Modified = $npInfo.LastWriteTime; SizeBytes = $npInfo.Length; PromptCount = 1
                       Title = 'cached title'; LastUser = 'u'; LastAssistant = 'a'; RecentMessages = @() }
$npCache = Join-Path $shRoot 'nopath.json'
[IO.File]::WriteAllText($npCache, (@{ $npKey = $npEntry } | ConvertTo-Json -Depth 6), (New-Object System.Text.UTF8Encoding($false)))
$npRows = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $npCache -ProjectSlug 'C--src-nopath')
Assert-Equal 1 $npRows.Count 'a cache entry lacking Path still yields its row'
Assert-Equal 'cached title' $npRows[0].Title 'and it is served from the cache, not re-summarised'
Assert-Equal $npInfo.FullName $npRows[0].Path 'and its Path is refreshed to the path the CALLER asked about, whatever root filled the cache'

# A cache another instance holds OPEN is busy, not corrupt: no error record, and no full cold start.
$busyCalls = 0
$busyOrig = ${function:Get-ClaudeSessionSummary}
function Get-ClaudeSessionSummary {
    param([Parameter(Mandatory)][string]$Path, [int]$HeadLines = 400, [int]$TailLines = 120, [string]$ProjectPath = $null, [switch]$NoPreFilter)
    $script:busyCalls++; & $busyOrig @PSBoundParameters
}
$null = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $shCache -Limit 100)   # warm the new rows in
$script:busyCalls = 0
$null = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $shCache -Limit 100)
$warmCalls = $script:busyCalls
$script:busyCalls = 0
$busyHandle = [IO.File]::Open($shCache, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
try { $busyOut = @(Get-ClaudeSessions -ProjectsRoot $shRoot -CachePath $shCache -Limit 100 2>&1) } finally { $busyHandle.Dispose() }
${function:Get-ClaudeSessionSummary} = $busyOrig
Assert-Equal 0 $warmCalls 'a second listing over an unchanged tree summarises nothing - the control for the next assertion'
Assert-Equal $warmCalls $script:busyCalls 'a cache another instance is holding open is not a silent full cold start'
Assert-Equal 0 @($busyOut | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] }).Count 'and the busy read never reaches the terminal as an error record over the picker frame'
Remove-Item -LiteralPath $shRoot -Recurse -Force -ErrorAction SilentlyContinue

# --- the physical projects directory, resolved through EVERY component -----------------------------
$phRoot = Join-Path $env:TEMP ('claude-auto-phys-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$phHome = Join-Path $phRoot 'phys'
$phProjects = Join-Path $phHome 'projects'
New-Item -ItemType Directory -Force -Path $phProjects | Out-Null
$phAccA = Join-Path $phRoot 'accA'
New-Item -ItemType Directory -Force -Path $phAccA | Out-Null
$null = New-Item -ItemType Junction -Path (Join-Path $phAccA 'projects') -Target $phProjects
Assert-Equal $phProjects (Get-PhysicalDirectoryPath -Path (Join-Path $phAccA 'projects')) 'a junction at the FINAL component resolves to its target'
# The junction one level UP: a junctioned profile root holding a real projects\ directory.
$phAccB = Join-Path $phRoot 'accB'
$null = New-Item -ItemType Junction -Path $phAccB -Target $phHome
Assert-Equal $phProjects (Get-PhysicalDirectoryPath -Path (Join-Path $phAccB 'projects')) 'a junction on an INTERMEDIATE component is resolved too, not left as the caller spelled it'
Assert-Equal (Get-SessionsCachePath -ProjectsRoot $phProjects) (Get-SessionsCachePath -ProjectsRoot (Join-Path $phAccB 'projects')) 'so a root reached through a junctioned profile root shares the one cache file'
Remove-Item -LiteralPath $phRoot -Recurse -Force -ErrorAction SilentlyContinue

# --- the pre-filter: each half pinned on its own, and what it may not change -----------------------
# Test 18 counts parses over the WHOLE summary, which either filter alone satisfies - so neither was
# individually pinned and deleting either one reddened nothing (adversarial review 2026-09-16, M8/M12).
# This fixture separates them: the noise before the first prompt is seen only by the HEAD walk, the
# noise after it only by the TAIL walk (the tail window is the last 120 lines).
$pfRoot = Join-Path $env:TEMP ('claude-auto-prefilter-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$pfDir = Join-Path $pfRoot 'C--src-pf'
New-Item -ItemType Directory -Force -Path $pfDir | Out-Null
$pfLines = @()
foreach ($n in 1..200) { $pfLines += ('{"type":"summary","summary":"head noise ' + $n + '"}') }
$pfLines += '{"type":"user","message":{"content":"the head prompt"}}'
foreach ($n in 1..119) { $pfLines += ('{"type":"summary","summary":"tail noise ' + $n + '"}') }
$pfFile = Join-Path $pfDir 'pppp1111.jsonl'
[IO.File]::WriteAllText($pfFile, ($pfLines -join "`n") + "`n", (New-Object System.Text.UTF8Encoding($false)))

$pfOrig = ${function:ConvertFrom-JsonlLine}
function ConvertFrom-JsonlLine { param([string]$Line) $script:pfSeen = @($script:pfSeen) + @($Line); & $pfOrig -Line $Line }
$script:pfSeen = @()
$pfFiltered = Get-ClaudeSessionSummary -Path $pfFile -ProjectPath 'C:\src\pf'
$pfHead = @($script:pfSeen | Where-Object { $_ -match 'head noise' }).Count
$pfTail = @($script:pfSeen | Where-Object { $_ -match 'tail noise' }).Count
$script:pfSeen = @()
$pfPlain = Get-ClaudeSessionSummary -Path $pfFile -ProjectPath 'C:\src\pf' -NoPreFilter
$pfHeadU = @($script:pfSeen | Where-Object { $_ -match 'head noise' }).Count
$pfTailU = @($script:pfSeen | Where-Object { $_ -match 'tail noise' }).Count
${function:ConvertFrom-JsonlLine} = $pfOrig
Assert-Equal 0 $pfHead 'the HEAD pre-filter alone: not one summary record before the first prompt is parsed'
Assert-Equal 0 $pfTail 'the TAIL pre-filter alone: not one summary record inside the tail window is parsed'
Assert-True ($pfHeadU -gt 0) 'control: without the filter the head walk really does parse them'
Assert-True ($pfTailU -gt 0) 'control: without the filter the tail walk really does parse them'
Assert-Equal $pfPlain.Title $pfFiltered.Title 'and the filter changes only HOW MANY lines are parsed, never which are accepted'

# The two shapes a byte-literal '"type":"user"' misses while ConvertFrom-Json accepts them.
$pfSpaced = Join-Path $pfDir 'ssss2222.jsonl'
[IO.File]::WriteAllText($pfSpaced, '{"type": "user", "message": {"content": "spaced key prompt"}}' + "`n" +
                                   '{"type": "assistant", "message": {"content": [{"type": "text", "text": "spaced key reply"}]}}' + "`n",
                        (New-Object System.Text.UTF8Encoding($false)))
$pfS = Get-ClaudeSessionSummary -Path $pfSpaced -ProjectPath 'C:\src\pf'
Assert-Equal 'spaced key prompt' $pfS.Title 'one space after the colon is JSON-legal and must not hide a prompt from the pre-filter'
Assert-Equal 'spaced key reply' $pfS.LastAssistant 'nor an assistant reply from the tail walk'
$bs = [char]92   # built from its code point: written literally, whatever writes the file decodes it
$pfEsc = Join-Path $pfDir 'eeee3333.jsonl'
[IO.File]::WriteAllText($pfEsc, '{"' + $bs + 'u0074ype":"user","message":{"content":"escaped key prompt"}}' + "`n",
                        (New-Object System.Text.UTF8Encoding($false)))
Assert-Equal 'escaped key prompt' (Get-ClaudeSessionSummary -Path $pfEsc -ProjectPath 'C:\src\pf').Title 'a \u-escaped type key is decoded by the parser, so the pre-filter must not skip the line on a literal miss'
Remove-Item -LiteralPath $pfRoot -Recurse -Force -ErrorAction SilentlyContinue

if ($script:Ran -ne 167) { Write-Host "COULD NOT RUN: expected 167 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
