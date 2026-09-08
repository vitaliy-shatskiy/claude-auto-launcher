# Privacy scan: no owner identifier may be in the tree. Patterns = the current Windows username plus
# an optional private list (one regex per line; CLAUDE_AUTO_CLEAN_PATTERNS or -Patterns). The private
# list never enters the repository - it IS the list of things that must not.
# Exit: 0 clean · 1 a hit · 2 could not run (not a pass).
#
# SCOPE: the git-tracked worktree only. It never reads commit history and never reads commit
# authorship, so a name or address baked into a commit header passes this scan untouched. Check
# those separately before publishing: `git log --all --format='%an <%ae>'`.
[CmdletBinding()]
param([string]$Root = (Join-Path $PSScriptRoot '..'), [string]$Patterns = $env:CLAUDE_AUTO_CLEAN_PATTERNS)
$ErrorActionPreference = 'Stop'
try {
    $Root = (Resolve-Path $Root).Path
    # Word-bounded: a username that is a prefix of the author's name must not flag LICENSE. An
    # EMPTY username must not become '\b\b', which matches at every word boundary and would flag
    # every line of every file - a service account or a CI runner with no USERNAME set does that.
    $list = @()
    if ($env:USERNAME) { $list += '\b' + [regex]::Escape($env:USERNAME) + '\b' }
    if ($Patterns) {
        if (-not (Test-Path -LiteralPath $Patterns)) { Write-Host "pattern file not found: $Patterns"; exit 2 }
        $list += @(Get-Content -LiteralPath $Patterns | Where-Object { $_.Trim() -and -not $_.StartsWith('#') })
    }
    # Nothing to look for is not a clean tree. Fail closed, the same way a missing pattern file does.
    if ($list.Count -eq 0) { Write-Host 'no patterns: USERNAME is empty and no pattern file was given'; exit 2 }
    # Scoped to what git would ship, not the whole disk: a gitignored local execution ledger
    # (review packages, a recorded regression reference) is expected to carry things that never
    # leave this machine, and scanning it would turn a real privacy gate into permanent noise.
    # -z with core.quotePath=false: git's default quotes a non-ASCII name as octal escapes when
    # printed as text, which Join-Path/Test-Path then cannot resolve - a plain Where-Object filter
    # here silently DROPPED that file from the scan (measured: tests\fixtures\ф.txt, containing the
    # username, read as clean). NUL-separated output also survives a filename with a newline in it.
    $raw = & git -c core.quotePath=false -C $Root ls-files -z --cached --others --exclude-standard
    if ($LASTEXITCODE -ne 0) { throw "git ls-files failed (exit $LASTEXITCODE) - is $Root a git repository?" }
    $nul = [char]0
    $relPaths = @(($raw -join $nul) -split $nul | Where-Object { $_ })
    $paths = @($relPaths | ForEach-Object { Join-Path $Root $_ })
    # A path git lists that is not a readable leaf on disk (deleted-but-staged, a directory entry,
    # a broken link) must FAIL CLOSED - silently dropping it (the old bug) is how a real file went
    # unscanned. Counted rather than skipped one by one so the message says how many, not which.
    $unreadable = @($paths | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) })
    if ($unreadable.Count -gt 0) {
        Write-Host "COULD NOT RUN: $($unreadable.Count) listed path(s) could not be read"
        exit 2
    }
    $files = @($paths |
        ForEach-Object { Get-Item -LiteralPath $_ -Force } |
        Where-Object { $_.Extension -notin @('.dll', '.pdb') })
    $hits = 0
    foreach ($f in $files) {
        $i = 0
        foreach ($line in [IO.File]::ReadLines($f.FullName)) {
            $i++
            foreach ($p in $list) { if ($line -match $p) { Write-Host "$($f.FullName.Substring($Root.Length)):$i  /$p/"; $hits++; break } }
        }
    }
    if ($hits) { Write-Host "check-clean: $hits hit(s)" -ForegroundColor Red; exit 1 }
    Write-Host "check-clean: clean ($($files.Count) files, $($list.Count) pattern(s))" -ForegroundColor Green; exit 0
} catch { Write-Host "check-clean could not run: $($_.Exception.Message)"; exit 2 }
