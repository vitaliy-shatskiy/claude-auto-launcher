# Assertions for Sessions.ps1. Run: pwsh -File Test-Sessions.ps1
try { . "$PSScriptRoot\..\claude-auto\Sessions.ps1" } catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }

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

if ($script:Ran -ne 50) { Write-Host "COULD NOT RUN: expected 50 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
