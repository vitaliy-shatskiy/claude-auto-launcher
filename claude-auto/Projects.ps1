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
        # CLAUDE_AUTO_PROJECTS_ROOT overrides, same shape as Get-LaunchPrefsPath's CLAUDE_AUTO_PREFS
        # (Prefs.ps1): a test (or check-preview.ps1's fixture) points this at a throwaway tree so
        # driving the project screen never depends on - or is defeated by - the owner's real
        # ~/.claude/projects. Fix round 1 (SURVIVING MUTANT): without this, check-preview.ps1 could
        # only ever prove the project screen renders SOMETHING, never that a specific argument
        # (like -ProjectSlug) reached a specific downstream call, because the real registry's
        # content is neither controlled nor known ahead of time.
        [string]$ProjectsRoot = $(if ($env:CLAUDE_AUTO_PROJECTS_ROOT) { $env:CLAUDE_AUTO_PROJECTS_ROOT } else { Join-Path $HOME '.claude\projects' }),
        # Computed in the BODY, not as a default expression: `Split-Path 'C:\' -Parent` is '' and
        # Join-Path rejects an empty -Path, so a drive-root CLAUDE_AUTO_PROJECTS_ROOT threw during
        # parameter binding - before any guard in here could run (adversarial review 2026-09-16, B8).
        [string]$CachePath = ''
    )
    if (-not $CachePath) {
        $cacheParent = try { Split-Path -Path $ProjectsRoot -Parent } catch { '' }
        if (-not $cacheParent) { $cacheParent = $ProjectsRoot }
        $CachePath = Join-Path $cacheParent 'claude-auto-projects.json'
    }
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
        # A cwd is transcript content, so it can be anything. An embedded NUL makes Test-Path raise a
        # non-terminating ArgumentException rather than answering $false, and the launcher runs at
        # the default $ErrorActionPreference - so a four-line red dump reached the terminal for a row
        # that was correctly dropped anyway (adversarial review 2026-09-16, A1). Checked before the
        # guard rather than suppressed inside it: a path that cannot name a file is not found, and
        # saying so quietly is the whole of the fix.
        if (-not $path -or $path.IndexOf([char]0) -ge 0) { continue }
        $fresh[$key] = @{ Path = $path }
        # Checked every run, never cached: a folder can be deleted between two launches, and a row
        # that cannot be entered is worse than a missing one.
        if (-not (Test-Path -LiteralPath $path)) { continue }
        $names = ConvertFrom-ClaudeProjectSlug -Slug $d.Name
        # -LiteralPath has no -Leaf parameter set (same trap noted in Sessions.ps1's
        # Test-ClaudeSessionFile: -LiteralPath alone returns the PARENT, no -Leaf switch exists
        # for it at all) - Split-Path -Path here, matching this codebase's existing convention.
        $leaf = Split-Path -Path $path -Leaf
        # A drive root has no leaf: Split-Path -Leaf 'C:\' returns 'C:\', so the Name and Path columns
        # rendered the identical string (adversarial review 2026-09-16, A1). 'C:' is a name; 'C:\' is
        # the path, and the spec lists a drive root among the directories it expects to meet.
        if ($leaf -eq $path) { $leaf = $path.TrimEnd([char]92, [char]47) }
        if (-not $leaf) { $leaf = $path }
        $out += [pscustomobject]@{
            Slug         = $d.Name
            Path         = $path
            Name         = $leaf
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

    # One real DIRECTORY is one row. Two slug folders can name the same directory - a cwd recorded
    # with different separators, a folder renamed and renamed back - and a row per SLUG put two rows
    # with identical Name and indistinguishable Path columns on the screen, with every slug lookup
    # taking $hit[0] so half of that project's sessions could not be reached from the screen that had
    # just named it (adversarial review 2026-09-16, A12). ConvertTo-ProjectKey already proves the two
    # are one directory; here the same normaliser decides the rows, rather than a second, ad-hoc one.
    # The row keeps EVERY slug, because that is what the session picker has to scope on.
    $merged = @()
    foreach ($g in ($out | Group-Object { ConvertTo-ProjectKey $_.Path })) {
        $group = @($g.Group | Sort-Object LastActivity -Descending)
        $top = $group[0]
        $merged += [pscustomobject]@{
            Slug         = $top.Slug
            Slugs        = @($group | ForEach-Object { $_.Slug })
            Path         = $top.Path
            Name         = $top.Name
            Worktree     = $top.Worktree
            LastActivity = $top.LastActivity
        }
    }
    return @($merged | Sort-Object LastActivity -Descending)
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
    # @() on the way out too: `return $Projects` unrolls a one-element array on the pipeline, so the
    # empty-filter branch answered with a bare object where the filtering branch answers with an
    # array. Both call sites wrap the call themselves today; the next one would not know to
    # (adversarial review 2026-09-16, A4).
    if ([string]::IsNullOrWhiteSpace($Filter)) { return @($Projects) }
    $f = [Management.Automation.WildcardPattern]::Escape($Filter.Trim())
    return @($Projects | Where-Object { "$($_.Name) $($_.Path) $($_.Worktree)" -like "*$f*" })
}

function ConvertTo-ProjectKey {
    # The one place two paths are compared for "is this the same directory". Three sources feed a
    # comparison against a registry path - a raw cwd string, a prefs file value nobody has
    # validated, and whatever a user typed or a -Initial caller passed - and none of them agree on
    # trailing separator or slash direction. Without this shared normaliser each caller grew its own
    # ad-hoc TrimEnd/ToLower (Resolve-StartProject had one; Invoke-ProjectScreen had a second,
    # slightly different one) and a caller passing 'C:/w/beta/' where the registry has 'C:\w\beta'
    # silently failed to match in exactly one of them - the failure mode is not "no match anywhere",
    # it is "matches in some callers and not others", which is worse to debug.
    param([string]$Path)
    if (-not $Path) { return '' }
    return $Path.TrimEnd('\', '/').Replace('/', '\').ToLowerInvariant()
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
    $c = ConvertTo-ProjectKey $Cwd
    if ($c) {
        $known = @($Projects | Where-Object { (ConvertTo-ProjectKey $_.Path) -eq $c })
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

function Set-LaunchStartProject {
    # Applies Resolve-StartProject's pick onto $State.Project/$State.ProjectSlug in one place
    # (claude-auto.ps1, Task 9) - the registry lookup for the slug uses the same ConvertTo-ProjectKey
    # normaliser Resolve-StartProject and Invoke-ProjectScreen already share, so a raw path never
    # gets compared against the registry a third, ad-hoc way.
    #
    # Pulled out as its own function so this wiring has a UNIT SEAM: the decision loop in
    # claude-auto.ps1 has no console and cannot be driven by a test directly - Test-Ui.ps1's seams
    # cover the screens, not the top-level script that builds the registry and calls this.
    #
    # Returns the Source ('cwd'|'remembered'|'none') for the launch log - claude-auto.ps1 answers
    # "why did the project screen open where it did" from it.
    param(
        [Parameter(Mandatory)]$State,
        [string]$Cwd,
        [Parameter(Mandatory)][AllowEmptyCollection()][array]$Projects
    )
    # Recorded so the rule can be RE-APPLIED on a tab switch (Switch-LaunchAccount, Prefs.ps1). The
    # launched account is whichever tab the owner ends on, so rule 2's input changes with every
    # switch and rule 1 has to be weighed against it again; applied once before the screen loop, the
    # cwd preselection was silently overridden by the arriving account's remembered project, and
    # dropped altogether by an account that had none (adversarial review 2026-09-16, A5/A5b).
    $script:LaunchStartContext = @{ Cwd = "$Cwd"; Projects = @($Projects) }
    $startInfo = Resolve-StartProject -Cwd $Cwd -Remembered "$($State.Project)" -Projects $Projects
    $State.Project = $startInfo.Path
    $State.ProjectSlug = ''
    if ($null -ne $State.PSObject.Properties['ProjectSlugs']) { $State.ProjectSlugs = @() }
    if ($State.Project) {
        $key = ConvertTo-ProjectKey $State.Project
        $hit = @($Projects | Where-Object { (ConvertTo-ProjectKey $_.Path) -eq $key })
        if ($hit.Count -gt 0) {
            $State.ProjectSlug = $hit[0].Slug
            # EVERY slug of that directory: one row can carry several (Get-ProjectRegistry).
            if ($null -ne $State.PSObject.Properties['ProjectSlugs']) {
                $State.ProjectSlugs = @($hit | ForEach-Object { if ($_.Slugs) { $_.Slugs } else { $_.Slug } })
            }
        }
    }
    return $startInfo.Source
}

function Update-LaunchStartSelection {
    # Re-applies the start-selection rule after the launched account changed. Rule 1 (cwd, when it is
    # a real project) outranks rule 2 (the remembered project of the launched account), and rule 2's
    # input is exactly what a tab switch replaces - so the ordering has to be decided again, with
    # $State.Project now holding whatever the arriving account remembered.
    #
    # The launch context comes from what Set-LaunchStartProject recorded rather than from new
    # parameters: this runs inside Switch-LaunchAccount, whose caller is the screen loop, and a tab
    # switch has no business being handed a project registry. Without a recorded context - any
    # caller that never ran the start selection at all - it is a no-op.
    param([Parameter(Mandatory)]$State)
    if ($null -eq $script:LaunchStartContext) { return $State }
    $null = Set-LaunchStartProject -State $State -Cwd $script:LaunchStartContext.Cwd -Projects $script:LaunchStartContext.Projects
    return $State
}
