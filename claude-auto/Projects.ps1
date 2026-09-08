# The project registry: which directories Claude Code has sessions in, where they really are, and
# when each was last touched. Pure apart from one derived cache file, so every function here is
# assertable without a console.

function Get-ProjectPathFromTranscript {
    # A slug directory name is NOT reversible: 'C--Users-x-Projects-Acme-Dashboard-Web-verdis-reports'
    # is 'C:\Users\x\Projects\Acme.Dashboard.Web-verdis-reports' - a '.', a literal '-' and a '\'
    # all render as '-'. The transcript carries the real path in its cwd field; that is the only
    # authority. Reads the NEWEST transcript because an old one may predate a folder rename.
    param([Parameter(Mandatory)][string]$Directory)
    $newest = Get-ChildItem -LiteralPath $Directory -Filter *.jsonl -File -Force -ErrorAction SilentlyContinue |
              Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $newest) { return $null }
    # -TotalCount, not -Tail: -Tail is pathological on these files (see Get-FileTailLines), and cwd
    # is on every record, so the first few lines answer it.
    foreach ($line in (Get-Content -LiteralPath $newest.FullName -TotalCount 40 -ErrorAction SilentlyContinue)) {
        $rec = ConvertFrom-JsonlLine -Line $line
        if ($rec -and $rec.cwd) { return "$($rec.cwd)" }
    }
    return $null
}

function Get-ProjectRegistry {
    # Every project slug directory, resolved and ordered by last activity. The cache holds only
    # DERIVED data - slug, resolved path, activity - so a lost or corrupt write costs one recompute
    # and never correctness. Atomic temp+move, last writer wins: up to four launchers run at once and
    # a mutex here would buy nothing a rebuild does not already give.
    param(
        [string]$ProjectsRoot = (Join-Path $HOME '.claude\projects'),
        [string]$CachePath = (Join-Path (Split-Path $ProjectsRoot -Parent) 'claude-auto-projects.json')
    )
    if (-not (Test-Path -LiteralPath $ProjectsRoot)) { return @() }

    $cache = @{}
    if (Test-Path -LiteralPath $CachePath) {
        try {
            (Get-Content -LiteralPath $CachePath -Raw | ConvertFrom-Json).PSObject.Properties |
                ForEach-Object { $cache[$_.Name] = $_.Value }
        } catch { $cache = @{} }
    }

    $out = @(); $fresh = @{}
    foreach ($d in (Get-ChildItem -LiteralPath $ProjectsRoot -Directory -Force -ErrorAction SilentlyContinue)) {
        $newest = Get-ChildItem -LiteralPath $d.FullName -Filter *.jsonl -File -Force -ErrorAction SilentlyContinue |
                  Sort-Object LastWriteTime -Descending | Select-Object -First 1
        # No transcript, no path and no activity. 8 of 46 directories on this machine are in this
        # state; they are dropped rather than shown as rows nothing can launch.
        if (-not $newest) { continue }
        $key = "$($d.Name)|$($newest.LastWriteTimeUtc.Ticks)"
        if ($cache.ContainsKey($key) -and $cache[$key].Path) {
            $path = "$($cache[$key].Path)"
        } else {
            $path = Get-ProjectPathFromTranscript -Directory $d.FullName
        }
        if (-not $path) { continue }
        $fresh[$key] = @{ Path = $path }
        # Checked every run, never cached: a folder can be deleted between two launches, and a row
        # that cannot be entered is worse than a missing one.
        if (-not (Test-Path -LiteralPath $path)) { continue }
        $names = ConvertFrom-ClaudeProjectSlug -Slug $d.Name
        $out += [pscustomobject]@{
            Slug         = $d.Name
            Path         = $path
            # -LiteralPath has no -Leaf parameter set (same trap noted in Sessions.ps1's
            # Test-ClaudeSessionFile: -LiteralPath alone returns the PARENT, no -Leaf switch exists
            # for it at all) - Split-Path -Path here, matching this codebase's existing convention.
            Name         = (Split-Path -Path $path -Leaf)
            Worktree     = $names.Worktree
            LastActivity = $newest.LastWriteTime
        }
    }

    $tmp = $null
    try {
        $tmp = "$CachePath.$PID-$([guid]::NewGuid().ToString('N').Substring(0,6)).tmp"
        $fresh | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $tmp -Encoding utf8
        [IO.File]::Move($tmp, $CachePath, $true)
        $tmp = $null
    } catch { Write-Verbose "Get-ProjectRegistry: could not write cache '$CachePath': $($_.Exception.Message)" }
    finally { if ($tmp) { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue } }

    return @($out | Sort-Object LastActivity -Descending)
}

function Select-ProjectMatch {
    # -like, not -match: mirrors the sibling filter Select-SessionMatch (Screens.ps1) so the two
    # filter boxes behave the same. A user pastes a real path into this one - backslashes and all -
    # and it must match literally; regex would turn '\w', '\a' or a bare '.' into escapes and
    # metacharacters no one typed.
    #
    # -like reads '[' as the start of a character class, and an unmatched one is a TERMINATING
    # WildcardPatternException that escapes Where-Object into the render loop - one '[' typed into
    # this filter would take the screen down. Escaping the filter text (not a try/catch) is the
    # fix: someone who typed '[' is looking for a literal '[', and Escape gives them that, where a
    # catch would return an empty list with no reason shown.
    param([Parameter(Mandatory)][AllowEmptyCollection()][array]$Projects, [string]$Filter)
    if ([string]::IsNullOrWhiteSpace($Filter)) { return $Projects }
    $f = [Management.Automation.WildcardPattern]::Escape($Filter.Trim())
    return @($Projects | Where-Object { "$($_.Name) $($_.Path) $($_.Worktree)" -like "*$f*" })
}

function Resolve-StartProject {
    # cwd wins when it is a project the machine already knows, or any directory holding a .git -
    # 'the folder I am standing in' is the launcher's existing behaviour and must not change. A
    # neutral directory (Desktop, home, a drive root, anything with neither) falls back to what this
    # account launched last. Neither: unresolved, and the screen says so rather than guessing.
    param(
        [string]$Cwd,
        [string]$Remembered,
        [Parameter(Mandatory)][AllowEmptyCollection()][array]$Projects
    )
    $norm = { param($p) if ($p) { "$p".TrimEnd('\', '/').ToLowerInvariant() } else { '' } }
    $c = & $norm $Cwd
    if ($c) {
        $known = @($Projects | Where-Object { (& $norm $_.Path) -eq $c })
        if ($known.Count -gt 0) { return [pscustomobject]@{ Path = $Cwd; Source = 'cwd' } }
        if (Test-Path -LiteralPath (Join-Path $Cwd '.git')) { return [pscustomobject]@{ Path = $Cwd; Source = 'cwd' } }
    }
    # $Remembered is a raw string out of the prefs file - it never passes through the registry, so
    # nothing upstream has checked it. A caller does 'Set-Location -LiteralPath $state.Project' with
    # no guard of its own; a project deleted between two launches must fail HERE, not take the
    # launcher down at start.
    if ($Remembered -and (Test-Path -LiteralPath $Remembered)) {
        return [pscustomobject]@{ Path = $Remembered; Source = 'remembered' }
    }
    return [pscustomobject]@{ Path = $null; Source = 'none' }
}
