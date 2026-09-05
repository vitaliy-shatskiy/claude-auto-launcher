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
    param([string]$Name, [string[]]$Extra, [int]$Expect)
    $f = Join-Path $env:TEMP "claude-auto-droplist-ref-$Name.txt"
    # inserted after the account line, i.e. exactly where the launcher would print them
    Set-Content -LiteralPath $f -Value (@($base[0]) + $Extra + $base[1..($base.Count - 1)]) -Encoding utf8
    $null = & pwsh -NoProfile -File $check -Reference $f -EvidenceDir $evDir 2>&1
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
Try-Ref 'cbm-started'     @('  cbm daemon started (watchdog retires it after claude exits)') 0
Try-Ref 'cbm-failed'      @('  cbm daemon start failed: address in use | already running') 0
Try-Ref 'doccounts'       @('  doc counts refreshed: 1 marker(s) were stale') 0
Try-Ref 'doccounts-exit2' @('  doc-count sync could not run (exit 2, not a pass): ENOENT') 0
Try-Ref 'git-committed'   @('  ~/.claude git: committed 5 file(s)') 0
Try-Ref 'git-indexlock'   @('  ~/.claude git: skipped: git add failed (fatal: Unable to create index.lock)') 0
Try-Ref 'all-of-them'     @(
    '  re-linked settings.json in .claude-acct2 (kept the newest copy, discarded ones saved as .pre-relink)'
    '  cbm daemon started (watchdog retires it after claude exits)'
    '  doc counts refreshed: 2 marker(s) were stale'
    '  ~/.claude git: committed 12 file(s)') 0

# --- lines that PERSIST across runs: must still fail (exit 1) ---
Try-Ref 'kept-real-directory' @('  skills is a real directory in .claude-acct2, not a junction - merge it by hand') 1
Try-Ref 'kept-shield'         @('  junction shield NOT applied for C:\Users\owner\.claude-acct2\skills (icacls exit 5) - the retention sweep can delete this junction') 1
Try-Ref 'kept-link-failed'    @('  link check failed for settings.json: access denied') 1
Try-Ref 'kept-mcp-skipped'    @('  MCP config skipped: boom') 1
Try-Ref 'kept-crc'            @('  crc not on PATH - plain session, not reachable from the phone (npm link in the server/ directory)') 1
Try-Ref 'kept-module-fail'    @('  module Env.ps1 failed to load: syntax error') 1
Try-Ref 'kept-git-autocommit' @('  ~/.claude git auto-commit failed: boom') 1

# --- anchoring: a REWORDED one-shot line must still fail ---
Try-Ref 'reworded-doccounts' @('  doc counts refreshed AGAIN: 1 marker(s) were stale') 1
Try-Ref 'reworded-cbm'       @('  cbm daemon started up (watchdog retires it after claude exits)') 1
Try-Ref 'reworded-relink'    @('  re-linked settings.json in .claude-acct2 (kept the oldest copy)') 1

if ($script:Fails -gt 0) { Write-Host "$($script:Fails) case(s) failed" -ForegroundColor Red; exit 1 }
Write-Host 'all droplist cases pass' -ForegroundColor Green
exit 0
