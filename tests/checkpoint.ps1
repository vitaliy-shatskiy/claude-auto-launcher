# Runs the claude-auto verification checkpoint: every test suite plus the launcher regression check
# and the privacy scan.
#
# Why this exists: the checkpoint was run by hand eight times during the v2 rebuild and restated in
# prose in every subagent brief. Two traps it encodes so nobody has to remember them:
#   - each suite's exit code is read on its own line. A pipeline reports the LAST command's status,
#     so `Test-Ui.ps1 | tail` hides a failure behind tail's 0.
#   - the regression check has THREE outcomes. Exit 2 means it could not run (missing launcher or
#     reference) and must never render as a pass - that mistake was made twice on this machine.

[CmdletBinding()]
param(
    [string]$RegressionScript = (Join-Path $PSScriptRoot 'check-regression.ps1'),
    [string]$CleanScript = (Join-Path $PSScriptRoot 'check-clean.ps1'),
    [string]$DroplistScript = (Join-Path $PSScriptRoot 'check-regression.droplist.ps1'),
    [string]$PreviewScript = (Join-Path $PSScriptRoot 'check-preview.ps1')
)

$suites = 'Theme', 'Layout', 'Sessions', 'Ui', 'Maintenance', 'Prefs', 'Env', 'Config', 'Input', 'Install', 'Mirror'
$rows = @()
$failed = 0
$unverified = 0

foreach ($name in $suites) {
    $path = Join-Path $PSScriptRoot "Test-$name.ps1"
    if (-not (Test-Path $path)) {
        $rows += [pscustomobject]@{ Check = "Test-$name"; Code = '-'; Verdict = 'MISSING' }
        $unverified++
        continue
    }
    # Input's live half (mouse, VT-swallow, console-mode assertions) is skipped without -Live - the
    # suite runs it correctly on its own (self-spawns a hidden child, reports the child's exit code),
    # it is just never asked to. Every other suite has no live/bare distinction.
    $extraArgs = if ($name -eq 'Input') { @('-Live') } else { @() }
    $null = & pwsh -NoProfile -File $path @extraArgs 2>&1
    $code = $LASTEXITCODE
    # 0 pass, 1 a real failure, 2 the suite could not run (a missing module, a broken dot-source).
    # Two is NOT a pass and it is NOT the same as a failing assertion - it must count toward
    # $unverified, never $failed, or a suite that could not even load would read as tested-and-red.
    $verdict = switch ($code) { 0 { 'pass' } 1 { 'FAIL' } 2 { 'DID NOT RUN' } default { "UNKNOWN($code)" } }
    if ($code -eq 1) { $failed++ } elseif ($code -ne 0) { $unverified++ }
    $rows += [pscustomobject]@{ Check = "Test-$name"; Code = $code; Verdict = $verdict }
}

$cleanEvidence = @()
$cleanCode = $null
if (-not (Test-Path $CleanScript)) {
    $rows += [pscustomobject]@{ Check = 'check-clean'; Code = '-'; Verdict = 'MISSING' }
    $unverified++
} else {
    # Output captured, not discarded: a bare "pass" row cannot say whether the private pattern
    # list was even in play - "clean" reads identically whether 1 pattern ran (username only) or
    # 17 did. The pattern count from the tool's own summary line rides along in the verdict, and
    # every hit line (or the summary line on a pass) is printed under the table as evidence.
    $cleanEvidence = @(& pwsh -NoProfile -File $CleanScript 2>&1 | ForEach-Object { "$_" })
    $code = $LASTEXITCODE
    $cleanCode = $code
    $patternCount = $null
    foreach ($line in $cleanEvidence) {
        if ($line -match 'clean \(\d+ files, (\d+) pattern') { $patternCount = $Matches[1]; break }
    }
    # 0 clean, 1 a hit, 2 could-not-run. Two is NOT a pass.
    $verdict = switch ($code) {
        0 { if ($patternCount) { "pass ($patternCount patterns)" } else { 'pass (pattern count unknown)' } }
        1 { 'FAIL' }
        2 { 'DID NOT RUN' }
        default { "UNKNOWN($code)" }
    }
    if ($code -eq 1) { $failed++ } elseif ($code -ne 0) { $unverified++ }
    $rows += [pscustomobject]@{ Check = 'check-clean'; Code = $code; Verdict = $verdict }
}

if (-not (Test-Path $RegressionScript)) {
    $rows += [pscustomobject]@{ Check = 'regression'; Code = '-'; Verdict = 'MISSING' }
    $unverified++
} else {
    $null = & pwsh -NoProfile -File $RegressionScript 2>&1
    $code = $LASTEXITCODE
    # 0 unchanged, 1 regression, 2 could-not-check. Two is NOT a pass.
    $verdict = switch ($code) { 0 { 'pass' } 1 { 'REGRESSION' } 2 { 'DID NOT RUN' } default { "UNKNOWN($code)" } }
    if ($code -eq 1) { $failed++ } elseif ($code -ne 0) { $unverified++ }
    $rows += [pscustomobject]@{ Check = 'regression'; Code = $code; Verdict = $verdict }
}

if (-not (Test-Path $DroplistScript)) {
    $rows += [pscustomobject]@{ Check = 'droplist'; Code = '-'; Verdict = 'MISSING' }
    $unverified++
} else {
    $null = & pwsh -NoProfile -File $DroplistScript 2>&1
    $code = $LASTEXITCODE
    # 0 all cases pass, 1 a case failed, 2 could-not-run (no reference yet - see (F2)). Two is NOT a pass.
    $verdict = switch ($code) { 0 { 'pass' } 1 { 'FAIL' } 2 { 'DID NOT RUN' } default { "UNKNOWN($code)" } }
    if ($code -eq 1) { $failed++ } elseif ($code -ne 0) { $unverified++ }
    $rows += [pscustomobject]@{ Check = 'droplist'; Code = $code; Verdict = $verdict }
}

if (-not (Test-Path $PreviewScript)) {
    $rows += [pscustomobject]@{ Check = 'preview'; Code = '-'; Verdict = 'MISSING' }
    $unverified++
} else {
    $null = & pwsh -NoProfile -File $PreviewScript 2>&1
    $code = $LASTEXITCODE
    # 0 the decision summary is unchanged, 1 it changed (a REGRESSION), 2 could-not-run (no
    # reference yet, or the launcher/fixture went missing). Two is NOT a pass - same contract as
    # regression and droplist above.
    $verdict = switch ($code) { 0 { 'pass' } 1 { 'REGRESSION' } 2 { 'DID NOT RUN' } default { "UNKNOWN($code)" } }
    if ($code -eq 1) { $failed++ } elseif ($code -ne 0) { $unverified++ }
    $rows += [pscustomobject]@{ Check = 'preview'; Code = $code; Verdict = $verdict }
}

$rows | Format-Table -AutoSize | Out-String | Write-Host

if ($cleanEvidence.Count -gt 0) {
    if ($cleanCode -eq 0) {
        # A pass: the tool's own summary line is the evidence (file count and pattern count).
        $cleanEvidence | Where-Object { $_ -match '^check-clean: clean' } | ForEach-Object { Write-Host "  $_" -ForegroundColor DarkGray }
    } else {
        # A hit or a could-not-run: every captured line, since each one is a `path:line /pattern/`
        # hit (or the could-not-run reason) that the row alone does not name.
        Write-Host "check-clean output:" -ForegroundColor DarkGray
        $cleanEvidence | ForEach-Object { Write-Host "  $_" }
    }
    Write-Host ""
}

if ($failed -gt 0) {
    $msg = "CHECKPOINT FAILED - $failed check(s) failed"
    if ($unverified -gt 0) { $msg += ", $unverified did not run" }
    Write-Host $msg -ForegroundColor Red
    exit 1
}
if ($unverified -gt 0) {
    Write-Host "CHECKPOINT INCOMPLETE - $unverified check(s) did not run. This is NOT a pass." -ForegroundColor Red
    exit 2
}
Write-Host "checkpoint green - $($rows.Count) checks, all verified" -ForegroundColor Green
exit 0
