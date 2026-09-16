# Guards the launch screen's DECISION path - not just the headless --version path
# check-regression.ps1 already covers. Same shape as check-regression.ps1, same three outcomes.
#
# Why this exists: the 13-check gate asserted exactly ONE path through the 500+ line
# claude-auto.ps1 (the headless --version branch). tests\preview.ps1 already prints a full decision
# summary - account, CLAUDE_CONFIG_DIR, remote, launch args, command, argv count - for any scripted
# key sequence, and nothing read it. That is how the session-picker-ignores-the-chosen-account
# defect (Get-ClaudeSessions called with no root at all) lived in a repository with a green
# checkpoint: no check ever drove the launch screen past its default account.
#
# A FIXTURE config (CLAUDE_AUTO_CONFIG) is used so the reference does not depend on the owner's own
# accounts.json - only its 'work' entry needs to be the real ~/.claude (the canonical-root rule in
# Config.ps1 requires exactly one such account; its CLAUDE_CONFIG_DIR line is always the same empty
# value regardless of what that directory actually contains). preview.ps1 already redirects
# CLAUDE_AUTO_PREFS to its own throwaway file per run, so remembered choices cannot change the output.
#
# Exit 2 is "could not run", never a pass: a missing launcher, a missing fixture or a missing
# reference all return 2, same as check-regression.ps1.
#
# Refresh the reference deliberately, only after confirming the new output is correct:
#   pwsh -File tests/check-preview.ps1 -Record

[CmdletBinding()]
param(
    [string]$Preview = (Join-Path $PSScriptRoot 'preview.ps1'),
    [string]$Launcher = (Join-Path $PSScriptRoot '..\claude-auto.ps1'),
    [string]$FixtureConfig = (Join-Path $PSScriptRoot 'fixtures\config-preview.json'),
    [string]$Reference = (Join-Path $PSScriptRoot 'preview-reference.local.txt'),
    # The real entry every launch goes through. Its invocation line is lifted into the scratch shim
    # (New-ForwarderShim), so the second shape is this machine's actual forwarder shape, not a
    # retyped guess at it.
    [string]$Forwarder = (Join-Path $HOME 'bin\claude-auto.ps1'),
    # Capture a fresh reference from the launcher's current preview output instead of comparing.
    [switch]$Record
)

$ErrorActionPreference = 'Stop'

# One run at the default (Enter alone) and one that CHANGES THE ACCOUNT - the exact shape of the
# scenario finding 1 needed and nothing exercised. Named so a failing line names its own run.
#
# Task 9: the project screen now sits between the launch screen and everything else, so every run
# needs one more Enter (or, for the resume run, its own hotkey) to get past it.
$script:Runs = [ordered]@{
    'default-enter'     = 'Enter,Enter'
    'switch-account'    = 'RightArrow,Enter,Enter'
    # Account, then Enter through the launch screen, then 'r' on the project screen resumes the
    # fixture project sorted first (fixture-slug-a) and opens the picker scoped to its slug; Escape
    # cancels it. See Initialize-ProjectSlugFixture below for why the session COUNT this prints is
    # the actual regression guard, not merely "picker cancelled" (which reads identically whichever
    # way the scoping went).
    #
    # Task 9 correction (its own report, "Concerns for Task 10/11" 1): row 0 became the CURRENT
    # DIRECTORY (spec D6), so fixture-slug-a - still sorted first among the REGISTRY rows - moved to
    # row 1. One 's' before 'r' is what still lands the hotkey on it instead of resuming the cwd.
    'switch-and-resume' = 'RightArrow,Enter,s,r,Escape'
    # A project the account has NO sessions for must open an EMPTY picker, never the account's
    # list. Reached through the 'current directory' row: Get-ProjectRegistry drops a slug
    # directory with no *.jsonl outright ("No transcript, no path and no activity"), so a
    # transcript-less project is never a row to select at all. The cwd row needs no navigation at
    # all to reach it, over an account that HAS two sessions. Correct output is "0 sessions"; the
    # failure it guards is that frame reading "2 sessions".
    #
    # Task 9 correction (its own report, "Concerns for Task 10/11" 1): row 0 is now the current
    # directory itself, so the two 's' keys that used to walk past both fixture rows to reach it
    # would instead land the hotkey on the second fixture project - dropped outright.
    #
    # What it does NOT cover, measured rather than assumed (review W2): a path outside the registry
    # has no slugs, so claude-auto.ps1 hands the fetcher an EMPTY slug list and the fetch is
    # unscoped - the filtering that empties this frame is the picker's own -ProjectName scope. The
    # fetcher's two [string[]] casts are therefore not on this path: dropping both, dropping
    # -Files entirely, and dropping the @() around the first page each leave this run GREEN
    # (Get-ClaudeSessions keys on $PSBoundParameters.ContainsKey('Files'), presence not emptiness,
    # and the snapshot reaches it through a VARIABLE, which cannot unroll). Those casts are pinned
    # in source by Test-Maintenance instead, which is the only guard that can fail for them.
    'unknown-project'   = 'RightArrow,Enter,r,Escape'
}

# Per-run working directory. Only the unknown-project run needs one (it is selected BY the cwd), and it
# has to be a path with a fixed leaf - the picker frame prints that leaf as the project name, so
# running from wherever the check happened to be invoked would put a machine-dependent word in the
# reference. Filled in by Initialize-ProjectSlugFixture, which owns the tree it points into.
$script:RunCwd = @{}

# Fix round 1 (SURVIVING MUTANT, closed the reviewer's way): the project screen used to read the
# REAL ~/.claude/projects registry with no fixture override at all, so this check could only ever
# prove the screen renders SOMETHING - never that -ProjectSlug specifically reached
# Invoke-SessionPicker, because a lost -ProjectSlug and a kept one produced the IDENTICAL "0
# sessions" text against the fixture 'second' account's empty session root. CLAUDE_AUTO_PROJECTS_ROOT
# (Get-ProjectRegistry, Projects.ps1) now lets this point the registry at an isolated fixture tree -
# never the owner's real registry, never the real machine's session history - and that same tree
# doubles as the fixture 'second' account's own session root (Get-SessionsRootForAccount already
# resolves ~/.claude-preview-fixture/projects for it), exactly as a real launch keeps them the same
# kind of directory for the account actually selected.
#
# Two sessions, two DIFFERENT slugs, the SAME display Project name ("Shared", two different real
# repos both named that on disk) - the exact shape Invoke-SessionPicker's own Test-Ui.ps1 coverage
# already proves distinguishes slug-scoping from the name-fallback branch. With -ProjectSlug kept,
# scoping to fixture-slug-a's session shows "1 sessions"; drop it and the call falls back to
# matching by Project name alone, which BOTH sessions share, so the same run prints "2 sessions"
# instead - a REGRESSION line this check's own diff catches, deterministically, on any machine.
function Initialize-ProjectSlugFixture {
    param([string]$Root = (Join-Path $HOME '.claude-preview-fixture'))
    $projectsRoot = Join-Path $Root 'projects'
    $reposRoot = Join-Path $Root 'fixture-repos'
    # Wiped and rebuilt every run - idempotent, so a stale run can never leave a third session lying
    # around to confuse a later one, and a machine running this for the first time gets a clean tree.
    foreach ($p in @($projectsRoot, $reposRoot)) {
        if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Recurse -Force }
        New-Item -ItemType Directory -Force -Path $p | Out-Null
    }
    # The DERIVED caches too (claude-auto-projects.json, claude-auto-sessions.json - both land beside
    # the projects root, Get-ProjectRegistry and Get-SessionsCachePath). Wiping only the tree left
    # them to accumulate across invocations and, worse, to carry state from the direct shape into the
    # forwarder shape: two sequential runs over ONE fixture, the second always warm. They hold
    # derived data only, so this is not a correctness fix - it is what keeps a difference between
    # the shapes attributable to the shapes (review suspicion).
    foreach ($c in @(Get-ChildItem -LiteralPath $Root -Filter 'claude-auto-*.json' -File -Force -ErrorAction SilentlyContinue)) {
        Remove-Item -LiteralPath $c.FullName -Force -ErrorAction SilentlyContinue
    }
    $repoA = Join-Path $reposRoot 'repo-a\Shared'
    $repoB = Join-Path $reposRoot 'repo-b\Shared'
    New-Item -ItemType Directory -Force -Path $repoA | Out-Null
    New-Item -ItemType Directory -Force -Path $repoB | Out-Null

    $slugADir = Join-Path $projectsRoot 'fixture-slug-a'
    $slugBDir = Join-Path $projectsRoot 'fixture-slug-b'
    New-Item -ItemType Directory -Force -Path $slugADir | Out-Null
    New-Item -ItemType Directory -Force -Path $slugBDir | Out-Null

    # A real message body, not a bare {type;cwd} record: Get-ClaudeSessions needs a genuine user
    # prompt to count PromptCount > 0, or Select-ResumableSessions drops the session and both slugs
    # would print "0 sessions" regardless of the mutation under test.
    $recA = @{ type = 'user'; cwd = $repoA; sessionId = 'fixture-a'; timestamp = '2026-01-01T00:00:00Z'
               message = @{ role = 'user'; content = 'fixture prompt for slug A' } } | ConvertTo-Json -Compress -Depth 5
    $recB = @{ type = 'user'; cwd = $repoB; sessionId = 'fixture-b'; timestamp = '2026-01-01T00:00:00Z'
               message = @{ role = 'user'; content = 'fixture prompt for slug B' } } | ConvertTo-Json -Compress -Depth 5
    Set-Content -LiteralPath (Join-Path $slugADir 'fixture-a.jsonl') -Value $recA -Encoding utf8 -NoNewline
    Set-Content -LiteralPath (Join-Path $slugBDir 'fixture-b.jsonl') -Value $recB -Encoding utf8 -NoNewline
    # fixture-slug-a sorts first (Get-ProjectRegistry orders by LastActivity descending) - R14:
    # since Task 9 put the current directory at row 0, switch-and-resume's scripted keys press one
    # 's' to reach row 1 (the first REGISTRY row) before 'r', so which project sorts first there
    # still has to be pinned, not left to whatever order the filesystem happens to enumerate.
    (Get-Item -LiteralPath (Join-Path $slugADir 'fixture-a.jsonl')).LastWriteTime = (Get-Date)
    (Get-Item -LiteralPath (Join-Path $slugBDir 'fixture-b.jsonl')).LastWriteTime = (Get-Date).AddMinutes(-5)

    # A third repo with NO slug directory anywhere under the projects root: the empty-scope run is
    # driven from inside it, so the picker opens on a project this account has never held a session
    # for. Named 'Empty' because the picker frame prints the leaf as the project name and the
    # reference has to be the same word on every machine.
    $repoEmpty = Join-Path $reposRoot 'repo-c\Empty'
    New-Item -ItemType Directory -Force -Path $repoEmpty | Out-Null
    $script:RunCwd['unknown-project'] = $repoEmpty

    return $projectsRoot
}
# NOT called here at script load: the wipe/rebuild it does (Remove-Item -Recurse -Force outside the
# repo) must never run on a path that is about to exit 2 anyway (Task 10 review - a missing launcher
# or fixture config was wiping the fixture tree on every invocation, -Record included, before the
# guards below even ran). It is called once each guard it needs has already passed, right before the
# one call site that actually reads it (Get-AllRunOutput, in each of the two branches below).

# preview.ps1's own (non--Full) summary filter does not include CLAUDE_CONFIG_DIR or argv count -
# both are named explicitly by the finding this check exists for (the session picker's account
# resolution shows up in CLAUDE_CONFIG_DIR, not in the launch args), so this check runs -Full and
# applies its own, wider filter rather than silently missing them.
#
# 'sessions' (the picker frame's own title, "resume - N sessions", and its "no sessions found"
# message) is the one line that actually distinguishes a regression of finding 1 from the fix: an
# Escape always cancels the picker regardless of what it found, so "picker cancelled" alone reads
# identically whether the list was empty (correct, for a fixture account with no projects
# directory) or full of the CANONICAL account's real sessions (the regression) - the session COUNT
# printed in the frame's own title is what tells the two apart without completing a pick.
$script:WantedPattern = 'launch args\s*:|command\s*:|remote\s*:|account\s*:|CLAUDE_CONFIG_DIR\s*:|argv count\s*:|picker cancelled|remote off for this session|crc |sessions|no sessions found'

# The launcher is NOT started by its own path in real life. Everything that starts Claude Code -
# Rider's plugin, claude-auto.cmd, PATH, the nightly audit - points at ~\bin\claude-auto.ps1, a
# forwarder that runs `& $target @args`. That one level of indirection is not cosmetic: under
# `pwsh -File claude-auto.ps1` the launcher's body IS the global scope, so its dot-sourced helpers
# land in global session state; called as `& $target` the body gets a child script scope and they
# do not. A .GetNewClosure() scriptblock resolves commands against GLOBAL only, so the session
# picker's page fetcher threw "Get-ClaudeSessionFile is not recognized" through the forwarder and
# ONLY through the forwarder - the shape every real launch uses and no check ever drove.
#
# Written per run rather than committed: it must point at whichever launcher is under test, and a
# checked-in copy would rot against ~\bin\claude-auto.ps1. The invocation line is LIFTED from that
# file rather than retyped, minus its missing-checkout fallback (irrelevant here - the guard above
# already proved the launcher exists). No param() block, for the same reason the launcher has none:
# adding one changes how --resume, --continue and -p bind.
#
# Lifted and not retyped because a retyped shim is unpinned: changed to `pwsh -File $target @args`
# it ran the -File shape TWICE while the summary still reported "2 invocation shape(s)", leaving the
# gate blind to the exact defect it exists for (review W1). Matched as a whole line, not searched
# for as a substring, and a forwarder that no longer has that line STOPS this check (exit 2 through
# the caller's catch) instead of silently weakening it.
function New-ForwarderShim {
    param(
        [Parameter(Mandatory)][string]$LauncherPath,
        [string]$Forwarder = (Join-Path $HOME 'bin\claude-auto.ps1')
    )
    if (-not (Test-Path -LiteralPath $Forwarder)) {
        throw "the real forwarder is not at $Forwarder - the shim cannot be pinned against it"
    }
    $invocation = @(Get-Content -LiteralPath $Forwarder | Where-Object { $_ -match '^\s*&\s*\$target\s+@args\s*$' })
    if ($invocation.Count -ne 1) {
        throw "$Forwarder does not invoke the launcher as '& `$target @args' ($($invocation.Count) matching line(s)) - the shim this check builds would no longer be its shape"
    }
    $path = Join-Path $env:TEMP ('claude-auto-fwd-' + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.ps1')
    $literal = (Resolve-Path -LiteralPath $LauncherPath).Path.Replace("'", "''")
    $text = "# Scratch forwarder - the invocation shape of ~\bin\claude-auto.ps1. Deliberately no param().`r`n" +
            "`$target = '$literal'`r`n" +
            $invocation[0].Trim() + "`r`n" +
            "exit `$LASTEXITCODE`r`n"
    [IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding($false)))
    return $path
}

function Invoke-PreviewRun {
    param([Parameter(Mandatory)][string]$Keys, [Parameter(Mandatory)][string]$LauncherPath, [string]$WorkingDirectory)
    $savedConfig = $env:CLAUDE_AUTO_CONFIG
    $savedProjectsRoot = $env:CLAUDE_AUTO_PROJECTS_ROOT
    $pushed = $false
    try {
        $env:CLAUDE_AUTO_CONFIG = $FixtureConfig
        $env:CLAUDE_AUTO_PROJECTS_ROOT = $script:ProjectsFixtureRoot
        # The child inherits this, and the project screen's 'current directory' row is built from it.
        if ($WorkingDirectory) { Push-Location -LiteralPath $WorkingDirectory; $pushed = $true }
        $out = & pwsh -NoProfile -File $Preview -Keys $Keys -Launcher $LauncherPath -Full 2>&1
        $code = $LASTEXITCODE
        $filtered = @($out | ForEach-Object { "$_" } | Where-Object { $_ -match $script:WantedPattern })
        return @($filtered) + @("preview.ps1 exit: $code")
    } finally {
        if ($pushed) { Pop-Location }
        if ($null -eq $savedConfig) { Remove-Item Env:CLAUDE_AUTO_CONFIG -ErrorAction SilentlyContinue }
        else { $env:CLAUDE_AUTO_CONFIG = $savedConfig }
        if ($null -eq $savedProjectsRoot) { Remove-Item Env:CLAUDE_AUTO_PROJECTS_ROOT -ErrorAction SilentlyContinue }
        else { $env:CLAUDE_AUTO_PROJECTS_ROOT = $savedProjectsRoot }
    }
}

function Get-AllRunOutput {
    param([Parameter(Mandatory)][string]$LauncherPath)
    $lines = New-Object System.Collections.Generic.List[string]
    foreach ($name in $script:Runs.Keys) {
        $lines.Add("=== $name ===")
        foreach ($l in (Invoke-PreviewRun -Keys $script:Runs[$name] -LauncherPath $LauncherPath -WorkingDirectory $script:RunCwd[$name])) { $lines.Add($l) }
    }
    return , @($lines)
}

# Positional, not set semantics: order (which run's block comes first, and each line within it) is
# part of the decision path being pinned.
#
# Emits the differing pairs one by one and the caller wraps in @(). NOT `return , @($pairs)`: that
# idiom plus the caller's own @() double-wraps - the pipeline unrolls the outer array and hands back
# ONE object that is the whole pair list, so the report said "1 line(s) differ" and printed all
# twenty line numbers on that one row (member enumeration). Caught by this check's own first run.
function Compare-ToReference {
    param($Reference, $Actual)
    $ref = @($Reference); $act = @($Actual)
    $max = [Math]::Max($ref.Count, $act.Count)
    for ($i = 0; $i -lt $max; $i++) {
        $e = if ($i -lt $ref.Count) { $ref[$i] } else { $null }
        $a = if ($i -lt $act.Count) { $act[$i] } else { $null }
        if ($e -cne $a) { [pscustomobject]@{ Line = $i + 1; Expected = $e; Actual = $a } }
    }
}

if (-not (Test-Path -LiteralPath $Launcher)) {
    Write-Host "CANNOT CHECK: launcher not found at $Launcher" -ForegroundColor Red
    exit 2
}
if (-not (Test-Path -LiteralPath $Preview)) {
    Write-Host "CANNOT CHECK: preview.ps1 not found at $Preview" -ForegroundColor Red
    exit 2
}
if (-not (Test-Path -LiteralPath $FixtureConfig)) {
    Write-Host "CANNOT CHECK: fixture config not found at $FixtureConfig" -ForegroundColor Red
    exit 2
}

if ($Record) {
    try {
        $script:ProjectsFixtureRoot = Initialize-ProjectSlugFixture
        # Recorded from the DIRECT shape only. One reference, both shapes compared against it: that
        # is what makes a shape-dependent defect a diff instead of two references drifting apart.
        $lines = Get-AllRunOutput -LauncherPath $Launcher
        if ($lines.Count -eq 0) {
            Write-Host "CANNOT RECORD: the preview runs produced no output at all" -ForegroundColor Red
            exit 2
        }
        Set-Content -LiteralPath $Reference -Value $lines -Encoding utf8
        Write-Host "recorded $($lines.Count) line(s) to $Reference" -ForegroundColor Green
        exit 0
    } catch {
        Write-Host "CANNOT RECORD: $($_.Exception.Message)" -ForegroundColor Red
        exit 2
    }
}

if (-not (Test-Path -LiteralPath $Reference)) {
    Write-Host "CANNOT CHECK: no reference output at $Reference" -ForegroundColor Red
    Write-Host "Capture one first: pwsh -File tests\check-preview.ps1 -Record - do not treat this as a pass." -ForegroundColor Red
    exit 2
}

$refLines = @(Get-Content -LiteralPath $Reference -ErrorAction SilentlyContinue)
if ($refLines.Count -eq 0) {
    Write-Host "CANNOT CHECK: the reference at $Reference is empty" -ForegroundColor Red
    exit 2
}

$shim = $null
try {
    $shim = New-ForwarderShim -LauncherPath $Launcher -Forwarder $Forwarder
    # Both shapes, same keys, same reference - and a fixture rebuilt BEFORE EACH of them. Sharing one
    # tree across the two sequential passes left the second always warm and any mutation by the first
    # invisible, so a difference between the shapes would not have been attributable to the shapes
    # (review suspicion). Rebuilding is idempotent and costs one directory wipe.
    # No @() around these calls: Get-AllRunOutput already returns `, @($lines)` and the pipeline
    # unrolls that one wrapper - adding another would hand the comparer a single nested object.
    $byShape = [ordered]@{}
    # Direct first so a defect common to both reads as an ordinary regression, not a forwarder problem.
    $script:ProjectsFixtureRoot = Initialize-ProjectSlugFixture
    $byShape['direct'] = Get-AllRunOutput -LauncherPath $Launcher
    $script:ProjectsFixtureRoot = Initialize-ProjectSlugFixture
    $byShape['forwarder'] = Get-AllRunOutput -LauncherPath $shim
} catch {
    Write-Host "CANNOT CHECK: $($_.Exception.Message)" -ForegroundColor Red
    exit 2
} finally {
    if ($shim) { Remove-Item -LiteralPath $shim -Force -ErrorAction SilentlyContinue }
}

$none = '(no such line)'
$bad = 0
foreach ($shape in $byShape.Keys) {
    $pairs = @(Compare-ToReference -Reference $refLines -Actual $byShape[$shape])
    if ($pairs.Count -eq 0) { continue }
    $bad++
    Write-Host "REGRESSION [$shape shape]: the preview decision summary changed - $($pairs.Count) line(s) differ" -ForegroundColor Red
    foreach ($p in $pairs) {
        Write-Host "  line $($p.Line)" -ForegroundColor Red
        Write-Host "    expected: $(if ($null -eq $p.Expected) { $none } else { $p.Expected })" -ForegroundColor DarkGray
        Write-Host "    actual  : $(if ($null -eq $p.Actual) { $none } else { $p.Actual })" -ForegroundColor Yellow
    }
}

if ($bad -gt 0) {
    Write-Host "reference: $Reference" -ForegroundColor DarkGray
    exit 1
}
Write-Host "OK: preview decision summary unchanged ($($refLines.Count) comparable line(s), $($script:Runs.Count) run(s), $($byShape.Count) invocation shape(s))" -ForegroundColor Green
exit 0
