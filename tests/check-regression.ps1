# Guards the one path in claude-auto.ps1 that must never change: the non-interactive launch.
#
# Why it exists: a scheduled task feeds the launcher `< NUL` to take the defaults, so a launcher
# that starts asking questions - or dies - takes that automation with it silently. This comparison
# is needed on every future launcher edit.
#
# It fails LOUDLY when it cannot check. A missing reference file exits 2, never "no regression":
# a guard that reports success when it did not run is worse than no guard. Anything unexpected that
# throws is also 2 - a locked file must not read as a regression.
#
# Refresh the reference deliberately, only after confirming the new output is correct:
#   pwsh -File tests/check-regression.ps1 -Record
#
# WHERE A RED RUN LEAVES ITS EVIDENCE (checkpoint.ps1 swallows this script's console output, so the
# file is the only survivor of a regression found by the checkpoint):
#   %TEMP%\claude-auto-regression-<yyyyMMdd-HHmmss>-<pid>.txt   - the full report, kept 30 days
#   %TEMP%\claude-auto-regression-<pid>.txt                     - the raw launcher output, per PID
#
# Three defects fixed, each with its own section below:
#   (a) a red run ate its own evidence. checkpoint.ps1 runs this with `$null = & pwsh ... 2>&1`,
#       so INSIDE checkpoint the printed diff never existed at all, and `Format-Table | Out-String`
#       wrapped what did print at 80 columns in a non-console host. Now every differing line is
#       printed as an expected/actual pair AND the whole run is written to a timestamped evidence
#       file that the next run cannot overwrite.
#   (b) the first run after any config change was red, because the preamble carries lines that
#       report a side effect THIS run performed. Reproduced live: run 1 exit 0, run 2 exit 1 on
#       `doc counts refreshed: 1 marker(s) were stale`.
#   (c) several launchers can run on this machine at once by design, so a line another launcher
#       printed seconds earlier flips this one's output. Same fix as (b): one drop list.
# Not run twice-and-compare-the-second: run 2 above was the RED one, so "the second run is clean"
# is not true, it doubles the config repository's auto-commits, and it does nothing for (c).

[CmdletBinding()]
param(
    [string]$Launcher = (Join-Path $PSScriptRoot '..\claude-auto.ps1'),
    # Parameterised so the negative control can point at a deliberately corrupted COPY instead of
    # dirtying the reference file. checkpoint.ps1 calls this bare; the defaults are the contract.
    [string]$Reference = (Join-Path $PSScriptRoot 'reference-output.local.txt'),
    [string]$EvidenceDir = $env:TEMP,
    # Capture a fresh reference from the launcher's current headless output instead of comparing.
    [switch]$Record
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------------------------
# Lines dropped before comparing.
#
# The rule: drop a line that reports an action THIS run performed, or whose presence depends on
# what another launcher did seconds earlier. Everything that PERSISTS across runs is kept and still
# fails the check - `... is a real directory ... merge it by hand`, `shield NOT applied`,
# `module <m> failed to load`, `MCP config skipped`, the remote/crc lines, `link check failed`.
# Every pattern is anchored at the start of the line so a reworded launcher message still fails.
#
# The cbm/doc-counts/`~/.claude git:` patterns below are not printed by THIS launcher on its own -
# they are owner-side hook output. With `launchHooks` configured, Invoke-LaunchHooks (Env.ps1)
# prints a hook's stdout verbatim, and on a machine that runs such a hook these are exactly the
# lines it emits. Kept for whoever configures a hook that does the same thing again - do not delete
# them for looking unreachable on a clean checkout with no hooks configured.
# ---------------------------------------------------------------------------------------------
$OneShotLines = @(
    # claude-auto\Env.ps1 - the hardlink repair. THE line this whole check kept tripping over:
    # launcher A repairs and prints it, the standalone run seconds later has nothing to repair.
    # The old filter here read `kept the newer copy` while the launcher says `kept the NEWEST copy`
    # (reworded at some point after the filter was written), so it matched nothing - the script's
    # header claimed four normalised lines and only three were live. Both spellings now.
    '^\s*re-linked .+ \(kept the new(er|est) copy'
    # Env.ps1 - fsutil racing a concurrent launcher that is deleting/recreating the same link.
    '^\s*could not read the File ID of .+ - link state unknown for '
    # Env.ps1 - a junction is created once and then exists.
    '^\s*junction created and shielded: '
    # Env.ps1 - a profile root and its hardlinks are created once, on the first launch after a new
    # account appears. This is defect (b)'s "created profile root ..." literal.
    '^\s*linked \S+ into \S+\s*$'
    '^\s*created profile root .+ - log in there on the first session with that account\s*$'
    # Env.ps1 - whether the CBM daemon was already up is decided by every other claude process on
    # the machine, and two launchers starting it at once make one of them fail.
    '^\s*cbm daemon started \(watchdog retires it after claude exits\)\s*$'
    '^\s*cbm daemon start failed: '
    # Env.ps1 - doc-count sync --write. Reproduced as a live red.
    '^\s*doc counts refreshed: \d+ marker\(s\) were stale\s*$'
    '^\s*doc-count sync could not run \(exit 2, not a pass\): '
    # claude-auto.ps1 - `~/.claude git: committed N file(s)` after any config change, and
    # `~/.claude git: skipped: ... index.lock` when another instance is mid-commit. Both are the
    # state of a repository, never the launcher's launch path. The launcher's OWN failure line
    # (`~/.claude git auto-commit failed:`) has no colon after `git` and is deliberately kept.
    '^\s*~/\.claude git: '
)

function Get-ComparableLines {
    # Rider assigns its MCP port dynamically and records it nowhere, so the reference captured one
    # particular port and every Rider restart made this guard cry regression on an unchanged
    # launcher. Compare the line's shape, not its digits: a missing or renamed rider line still
    # fails, only the port is normalised.
    # Whether Rider is RUNNING is environmental too - the reference was captured with Rider open
    # (URL form) and cried regression the first time the probe ran with Rider closed ("not found"
    # form). Collapse both variants to one token.
    # The Claude Code version number is environmental as well: `--version` is the probe this check
    # runs, so every CLI update rewrote that line. Normalise the DIGITS, keep the line: a launcher
    # that stops printing a version still fails.
    param([string]$Path)
    $lines = @(Get-Content -LiteralPath $Path |
        Where-Object { $line = $_; -not ($OneShotLines | Where-Object { $line -match $_ }) } |
        ForEach-Object { $_ -replace '127\.0\.0\.1:\d+', '127.0.0.1:<port>' } |
        ForEach-Object { $_ -replace '^(\s*rider MCP: )(http://127\.0\.0\.1:<port>/stream|not found \(is the IDE MCP server enabled\?\))$', '$1<rider-state>' } |
        ForEach-Object { $_ -replace '^\d+\.\d+\.\d+(\S*) \(Claude Code\)$', '<version> (Claude Code)' })
    # Trailing blank lines are an artefact of how each file was captured, never launcher behaviour.
    # Counted down rather than sliced: `$lines[0..($n-2)]` with one element is `0..-1`, which
    # returns BOTH ends of the array instead of an empty one.
    $n = $lines.Count
    while ($n -gt 0 -and [string]::IsNullOrWhiteSpace($lines[$n - 1])) { $n-- }
    if ($n -eq 0) { return , @() }
    return , @($lines[0..($n - 1)])
}

function Invoke-LauncherHeadless {
    # --version makes Claude print and exit, so the launcher runs its whole preamble - profile,
    # secrets, link repair, MCP discovery - without starting a session. `echo.` redirects stdin,
    # which is what puts the launcher on the non-interactive path in the first place.
    #
    # The cwd is PINNED to $HOME. Import-ProjectSecrets derives its secrets slug from $PWD, so the
    # `secrets loaded:` line lists whatever the CALLER's directory has a store for - run from a
    # project with project-tier secrets this check reported a regression on an unchanged launcher.
    # $HOME has no project store, which is what the reference was captured against.
    param([string]$OutFile)
    Push-Location -LiteralPath $HOME
    $previousCurrentDir = [Environment]::CurrentDirectory
    $previousEap = $ErrorActionPreference
    try {
        [Environment]::CurrentDirectory = $HOME
        # 'Continue' for this one call only: on PowerShell 7.4+ a native command's non-zero exit
        # throws under 'Stop', which would turn "the launcher failed" into an exception rather than
        # into the comparison the next lines make. The emptiness check below is the real guard.
        $ErrorActionPreference = 'Continue'
        cmd /c "cd /d `"$HOME`" && echo. | pwsh -NoProfile -File `"$Launcher`" --version" > $OutFile 2>&1
    } finally {
        $ErrorActionPreference = $previousEap
        [Environment]::CurrentDirectory = $previousCurrentDir
        Pop-Location
    }
}

function Invoke-RegressionCheck {
    # Returns the exit code instead of calling exit, so the caller's try/catch below is the single
    # place that decides 2. `exit` inside a try is a flow-control exception and reasoning about
    # whether catch sees it is not something a guard should depend on.
    if (-not (Test-Path -LiteralPath $Launcher)) {
        Write-Host "CANNOT CHECK: launcher not found at $Launcher" -ForegroundColor Red
        return 2
    }
    if (-not (Test-Path -LiteralPath $Reference)) {
        Write-Host "CANNOT CHECK: no reference output at $Reference" -ForegroundColor Red
        Write-Host "Capture one first (see the header of this script) - do not treat this as a pass." -ForegroundColor Red
        return 2
    }

    # Per-PID, because several launchers/checks can run at once by design and a single shared
    # %TEMP%\claude-auto-regression.txt was both overwritten before anyone read it (defect (a))
    # and clobberable mid-read by a concurrent check.
    $actual = Join-Path $EvidenceDir "claude-auto-regression-$PID.txt"
    Invoke-LauncherHeadless -OutFile $actual

    if (-not (Test-Path -LiteralPath $actual) -or (Get-Item -LiteralPath $actual).Length -eq 0) {
        Write-Host "CANNOT CHECK: the launcher produced no output at all" -ForegroundColor Red
        return 2
    }

    $refLines = Get-ComparableLines $Reference
    $actLines = Get-ComparableLines $actual

    # Positional, not Compare-Object's set semantics: the preamble's ORDER is part of the launch
    # path, and a set comparison calls two reordered lines equal. It also gives an expected/actual
    # pair per line, which is the whole point of (a).
    $max = [Math]::Max($refLines.Count, $actLines.Count)
    $pairs = @()
    for ($i = 0; $i -lt $max; $i++) {
        $e = if ($i -lt $refLines.Count) { $refLines[$i] } else { $null }
        $a = if ($i -lt $actLines.Count) { $actLines[$i] } else { $null }
        if ($e -cne $a) { $pairs += [pscustomobject]@{ Line = $i + 1; Expected = $e; Actual = $a } }
    }

    if ($pairs.Count -eq 0) {
        Write-Host "OK: launcher non-interactive path unchanged ($($refLines.Count) comparable line(s))" -ForegroundColor Green
        return 0
    }

    # ---- (a) evidence, written BEFORE anything is printed -------------------------------------
    # checkpoint.ps1 swallows this script's console output entirely, so the file is the only
    # survivor of a red run inside the checkpoint.
    $none = '(no such line)'
    $report = New-Object System.Collections.Generic.List[string]
    $report.Add("claude-auto regression - REGRESSION")
    $report.Add("when          : $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')")
    $report.Add("check pid     : $PID")
    $report.Add("launcher      : $Launcher")
    $report.Add("reference     : $Reference")
    $report.Add("launcher cwd  : $HOME (pinned)")
    $report.Add("caller cwd    : $((Get-Location).Path)")
    # (c) context, recorded rather than gated on: refusing to run whenever another session is alive
    # would turn the normal state of this machine into a permanent exit 2.
    $report.Add("claude.exe running: $(@(Get-Process claude -ErrorAction SilentlyContinue).Count)")
    $report.Add("pwsh.exe running  : $(@(Get-Process pwsh -ErrorAction SilentlyContinue).Count)")
    $report.Add('')
    $report.Add('--- differing lines (expected = reference, actual = this run) ---')
    foreach ($p in $pairs) {
        $report.Add("line $($p.Line)")
        $report.Add("  expected: $(if ($null -eq $p.Expected) { $none } else { $p.Expected })")
        $report.Add("  actual  : $(if ($null -eq $p.Actual) { $none } else { $p.Actual })")
    }
    $report.Add('')
    $report.Add('--- reference, after normalisation ---')
    $refLines | ForEach-Object { $report.Add($_) }
    $report.Add('')
    $report.Add('--- actual, after normalisation ---')
    $actLines | ForEach-Object { $report.Add($_) }
    $report.Add('')
    $report.Add('--- actual, RAW launcher output ---')
    Get-Content -LiteralPath $actual | ForEach-Object { $report.Add($_) }

    $evidence = Join-Path $EvidenceDir "claude-auto-regression-$(Get-Date -Format 'yyyyMMdd-HHmmss')-$PID.txt"
    $evidenceWritten = $true
    try {
        Set-Content -LiteralPath $evidence -Value $report -Encoding utf8
    } catch {
        $evidenceWritten = $false
        Write-Host "  could not write the evidence file: $($_.Exception.Message)" -ForegroundColor DarkYellow
    }

    # ---- the verdict, printed as pairs and never through Format-Table --------------------------
    # Format-Table -AutoSize | Out-String wraps at 80 columns in a non-console host, which is how
    # the old diff arrived truncated in exactly the runs that needed reading.
    Write-Host "REGRESSION: the non-interactive launch path changed - $($pairs.Count) line(s) differ" -ForegroundColor Red
    foreach ($p in $pairs) {
        Write-Host "  line $($p.Line)" -ForegroundColor Red
        Write-Host "    expected: $(if ($null -eq $p.Expected) { $none } else { $p.Expected })" -ForegroundColor DarkGray
        Write-Host "    actual  : $(if ($null -eq $p.Actual) { $none } else { $p.Actual })" -ForegroundColor Yellow
    }
    Write-Host "reference: $Reference" -ForegroundColor DarkGray
    Write-Host "actual:    $actual (overwritten by the next check on this PID)" -ForegroundColor DarkGray
    if ($evidenceWritten) {
        Write-Host "evidence:  $evidence" -ForegroundColor DarkGray
    }
    return 1
}

function Remove-OldEvidence {
    # Timestamped files would otherwise accumulate forever. Never touches anything but its own
    # name shape, and a failure here can never change the verdict.
    try {
        Get-ChildItem -LiteralPath $EvidenceDir -Filter 'claude-auto-regression-*-*.txt' -File -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTime -lt (Get-Date).AddDays(-30) } |
            ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue }
    } catch { }
}

if ($Record) {
    try {
        if (-not (Test-Path -LiteralPath $Launcher)) {
            Write-Host "CANNOT RECORD: launcher not found at $Launcher" -ForegroundColor Red
            exit 2
        }
        Invoke-LauncherHeadless -OutFile $Reference
        if (-not (Test-Path -LiteralPath $Reference) -or (Get-Item -LiteralPath $Reference).Length -eq 0) {
            Write-Host "CANNOT RECORD: the launcher produced no output at all" -ForegroundColor Red
            exit 2
        }
        $lineCount = @(Get-Content -LiteralPath $Reference).Count
        Write-Host "recorded $lineCount line(s) to $Reference" -ForegroundColor Green
        exit 0
    } catch {
        Write-Host "CANNOT RECORD: $($_.Exception.Message)" -ForegroundColor Red
        exit 2
    }
}

Remove-OldEvidence
$code = 2
try {
    $code = Invoke-RegressionCheck
} catch {
    # Anything unexpected - an unreadable reference, a locked temp file, a launcher that throws
    # into this process - is "could not check", never "regression" and never a pass.
    Write-Host "CANNOT CHECK: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "at: $($_.InvocationInfo.PositionMessage)" -ForegroundColor DarkGray
    $code = 2
}
exit $code
