# Drop-list control for check-regression.ps1: every one-shot launcher line must be dropped by
# Get-ComparableLines, every persistent or reworded line must still fail the check.
# Exit 0 all pass / 1 a case failed. Scratch files go to %TEMP%.
$ErrorActionPreference = 'Continue'
$check = Join-Path $PSScriptRoot 'check-regression.ps1'
$live  = Join-Path $PSScriptRoot 'reference-output.local.txt'
$evDir = Join-Path $env:TEMP 'claude-auto-droplist-evidence'
if (-not (Test-Path -LiteralPath $evDir)) { New-Item -ItemType Directory -Path $evDir | Out-Null }
# A missing reference is the DEFAULT state of a fresh clone (it is gitignored) - report "did not
# run", not 23 fabricated FAILs against an empty base line.
if (-not (Test-Path -LiteralPath $live)) {
    Write-Host "CANNOT RUN: no reference at $live - run check-regression.ps1 -Record first"
    exit 2
}
$base = @(Get-Content -LiteralPath $live -ErrorAction SilentlyContinue)
if ($base.Count -eq 0) {
    Write-Host "CANNOT RUN: no reference at $live - run check-regression.ps1 -Record first"
    exit 2
}

$script:Fails = 0
function Try-Ref {
    param([string]$Name, [string[]]$Extra, [int]$Expect, [string]$Patterns)
    $f = Join-Path $env:TEMP "claude-auto-droplist-ref-$Name.txt"
    # inserted after the account line, i.e. exactly where the launcher would print them
    Set-Content -LiteralPath $f -Value (@($base[0]) + $Extra + $base[1..($base.Count - 1)]) -Encoding utf8
    # Only passed when a case names one. Every case compares a LIVE launcher run against the
    # reference, so this machine's own hook lines have to be dropped on both sides or nothing
    # matches at all - written the other way first, passing an empty value to keep the cases
    # "pure", and all seven drop cases went red. The mechanism cases below therefore supply a file
    # that EXTENDS the machine's list rather than replacing it.
    $extraArgs = if ($PSBoundParameters.ContainsKey('Patterns')) { @('-VolatilePatterns', $Patterns) } else { @() }
    $null = & pwsh -NoProfile -File $check -Reference $f -EvidenceDir $evDir @extraArgs 2>&1
    $code = $LASTEXITCODE
    if ($code -eq $Expect) { $verdict = 'PASS' } else { $verdict = 'FAIL'; $script:Fails++ }
    Write-Host ("{0,-6} {1,-34} expected={2} got={3}" -f $verdict, $Name, $Expect, $code)
}

# --- one-shot / concurrency lines: MUST be dropped, so the reference still matches (exit 0) ---
Try-Ref 'relink-newest'   @('  re-linked settings.json in .claude-acct2, .claude-shared (kept the newest copy, discarded ones saved as .pre-relink)') 0
Try-Ref 'relink-newer'    @('  re-linked settings.json in .claude-acct2 (kept the newer copy, discarded ones saved as .pre-relink)') 0
Try-Ref 'fileid-unknown'  @('  could not read the File ID of C:\Users\owner\.claude\settings.json - link state unknown for settings.json') 0
Try-Ref 'junction'        @('  junction created and shielded: .claude-acct2 skills -> work') 0
Try-Ref 'linked-into'     @('  linked settings.json into .claude-low') 0
Try-Ref 'created-root'    @('  created profile root C:\Users\owner\.claude-low - log in there on the first session with that account') 0
Try-Ref 'all-of-them'     @(
    '  re-linked settings.json in .claude-acct2 (kept the newest copy, discarded ones saved as .pre-relink)'
    '  linked settings.json into .claude-low'
    '  junction created and shielded: .claude-acct2 skills -> work') 0

# --- the machine-local pattern file ------------------------------------------------------------
# A machine that runs launchHooks has their stdout in the preamble, one-shot in exactly the same
# way. Five such patterns used to be hardcoded above, naming one owner's tooling inside a repository
# meant to be published; they come from CLAUDE_AUTO_VOLATILE_PATTERNS now. What these cases assert is
# the MECHANISM, against a synthetic line no tool prints - so they keep working on any machine and
# name nothing.
$machineVp = "$env:CLAUDE_AUTO_VOLATILE_PATTERNS"
$machineLines = if ($machineVp -and (Test-Path -LiteralPath $machineVp)) { @(Get-Content -LiteralPath $machineVp) } else { @() }
# Two files: this machine's list alone, and the same list plus one synthetic pattern. The pair is
# what makes the case a control - the only difference between them is the line under test.
$vpBase = Join-Path $env:TEMP 'claude-auto-droplist-volatile-base.txt'
$vpPlus = Join-Path $env:TEMP 'claude-auto-droplist-volatile.txt'
Set-Content -LiteralPath $vpBase -Encoding utf8 -Value $machineLines
Set-Content -LiteralPath $vpPlus -Encoding utf8 -Value ($machineLines + @(
    '# a comment, which the loader must ignore',
    '',
    '^\s*synthetic hook line: \d+ things\s*$'
))
Try-Ref 'external-dropped'  @('  synthetic hook line: 3 things') 0 -Patterns $vpPlus
# The same line with the pattern REMOVED must still fail, or the case above proves nothing.
Try-Ref 'external-absent'   @('  synthetic hook line: 3 things') 1 -Patterns $vpBase
# Anchored like every pattern in the repo's own list: a reworded hook line is still a regression.
Try-Ref 'external-reworded' @('  synthetic hook line: three things') 1 -Patterns $vpPlus
# Named but unreadable is NOT "no extra patterns". Comparing against a shorter drop list is how a
# real regression gets filtered away, so the check refuses to run instead.
Try-Ref 'external-missing'  @('  synthetic hook line: 3 things') 2 -Patterns (Join-Path $env:TEMP 'claude-auto-no-such-patterns.txt')

# --- lines that PERSIST across runs: must still fail (exit 1) ---
Try-Ref 'kept-real-directory' @('  skills is a real directory in .claude-acct2, not a junction - merge it by hand') 1
Try-Ref 'kept-shield'         @('  junction shield NOT applied for C:\Users\owner\.claude-acct2\skills (icacls exit 5) - the retention sweep can delete this junction') 1
Try-Ref 'kept-link-failed'    @('  link check failed for settings.json: access denied') 1
Try-Ref 'kept-mcp-skipped'    @('  MCP config skipped: boom') 1
Try-Ref 'kept-crc'            @('  crc not on PATH - plain session, not reachable from the phone (npm link in the server/ directory)') 1
Try-Ref 'kept-module-fail'    @('  module Env.ps1 failed to load: syntax error') 1
Try-Ref 'kept-git-autocommit' @('  ~/.claude git auto-commit failed: boom') 1

# --- anchoring: a REWORDED one-shot line must still fail ---
Try-Ref 'reworded-relink'    @('  re-linked settings.json in .claude-acct2 (kept the oldest copy)') 1
Try-Ref 'reworded-junction'  @('  junction created but not shielded: .claude-acct2 skills -> work') 1
Try-Ref 'reworded-root'      @('  created profile root C:\Users\owner\.claude-low - log in there later') 1

if ($script:Fails -gt 0) { Write-Host "$($script:Fails) case(s) failed" -ForegroundColor Red; exit 1 }
Write-Host 'all droplist cases pass' -ForegroundColor Green
exit 0
