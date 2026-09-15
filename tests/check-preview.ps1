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
    'switch-and-resume' = 'RightArrow,Enter,r,Escape'
}

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
    # fixture-slug-a sorts first (Get-ProjectRegistry orders by LastActivity descending) - the
    # scripted keys press 'r' at row 0 without navigating, so which one sorts first has to be
    # pinned, not left to whatever order the filesystem happens to enumerate.
    (Get-Item -LiteralPath (Join-Path $slugADir 'fixture-a.jsonl')).LastWriteTime = (Get-Date)
    (Get-Item -LiteralPath (Join-Path $slugBDir 'fixture-b.jsonl')).LastWriteTime = (Get-Date).AddMinutes(-5)

    return $projectsRoot
}
$script:ProjectsFixtureRoot = Initialize-ProjectSlugFixture

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

function Invoke-PreviewRun {
    param([Parameter(Mandatory)][string]$Keys)
    $savedConfig = $env:CLAUDE_AUTO_CONFIG
    $savedProjectsRoot = $env:CLAUDE_AUTO_PROJECTS_ROOT
    try {
        $env:CLAUDE_AUTO_CONFIG = $FixtureConfig
        $env:CLAUDE_AUTO_PROJECTS_ROOT = $script:ProjectsFixtureRoot
        $out = & pwsh -NoProfile -File $Preview -Keys $Keys -Launcher $Launcher -Full 2>&1
        $code = $LASTEXITCODE
        $filtered = @($out | ForEach-Object { "$_" } | Where-Object { $_ -match $script:WantedPattern })
        return @($filtered) + @("preview.ps1 exit: $code")
    } finally {
        if ($null -eq $savedConfig) { Remove-Item Env:CLAUDE_AUTO_CONFIG -ErrorAction SilentlyContinue }
        else { $env:CLAUDE_AUTO_CONFIG = $savedConfig }
        if ($null -eq $savedProjectsRoot) { Remove-Item Env:CLAUDE_AUTO_PROJECTS_ROOT -ErrorAction SilentlyContinue }
        else { $env:CLAUDE_AUTO_PROJECTS_ROOT = $savedProjectsRoot }
    }
}

function Get-AllRunOutput {
    $lines = New-Object System.Collections.Generic.List[string]
    foreach ($name in $script:Runs.Keys) {
        $lines.Add("=== $name ===")
        foreach ($l in (Invoke-PreviewRun -Keys $script:Runs[$name])) { $lines.Add($l) }
    }
    return , @($lines)
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
        $lines = Get-AllRunOutput
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

try {
    $actual = Get-AllRunOutput
} catch {
    Write-Host "CANNOT CHECK: $($_.Exception.Message)" -ForegroundColor Red
    exit 2
}

$refLines = @(Get-Content -LiteralPath $Reference -ErrorAction SilentlyContinue)
if ($refLines.Count -eq 0) {
    Write-Host "CANNOT CHECK: the reference at $Reference is empty" -ForegroundColor Red
    exit 2
}

# Positional, not set semantics: order (which run's block comes first, and each line within it) is
# part of the decision path being pinned.
$max = [Math]::Max($refLines.Count, $actual.Count)
$pairs = @()
for ($i = 0; $i -lt $max; $i++) {
    $e = if ($i -lt $refLines.Count) { $refLines[$i] } else { $null }
    $a = if ($i -lt $actual.Count) { $actual[$i] } else { $null }
    if ($e -cne $a) { $pairs += [pscustomobject]@{ Line = $i + 1; Expected = $e; Actual = $a } }
}

if ($pairs.Count -eq 0) {
    Write-Host "OK: preview decision summary unchanged ($($refLines.Count) comparable line(s), $($script:Runs.Count) run(s))" -ForegroundColor Green
    exit 0
}

$none = '(no such line)'
Write-Host "REGRESSION: the preview decision summary changed - $($pairs.Count) line(s) differ" -ForegroundColor Red
foreach ($p in $pairs) {
    Write-Host "  line $($p.Line)" -ForegroundColor Red
    Write-Host "    expected: $(if ($null -eq $p.Expected) { $none } else { $p.Expected })" -ForegroundColor DarkGray
    Write-Host "    actual  : $(if ($null -eq $p.Actual) { $none } else { $p.Actual })" -ForegroundColor Yellow
}
Write-Host "reference: $Reference" -ForegroundColor DarkGray
exit 1
