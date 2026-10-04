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

function Remove-StalePreviewProjectsCache {
    # A preview run keeps its projects cache in a per-PID file under TEMP (claude-auto.ps1) and removes
    # it in its finally; this sweeps what a killed run left behind. Older than a day only, so a preview
    # running right now keeps its file, and only the exact name the launcher writes (a pid between
    # the prefix and .json) - anything else matching the wildcard is not ours to delete.
    param([string]$Directory = $env:TEMP, [datetime]$Now = (Get-Date), [TimeSpan]$MaxAge = [TimeSpan]::FromDays(1))
    $cutoff = $Now - $MaxAge
    foreach ($f in @(Get-ChildItem -LiteralPath $Directory -Filter 'claude-auto-projects-preview-*.json' -File -ErrorAction SilentlyContinue)) {
        if ($f.Name -notmatch '^claude-auto-projects-preview-\d+\.json$' -or $f.LastWriteTime -ge $cutoff) { continue }
        Remove-Item -LiteralPath $f.FullName -Force -ErrorAction SilentlyContinue
    }
}

function ConvertTo-ProjectScratchKey {
    # ConvertTo-ProjectKey for the scratch check, which is a PREFIX comparison and so has to see one
    # spelling of a directory: the \\?\ prefix is dropped, and '..' segments and doubled separators
    # are resolved. String work only - GetFullPath on a rooted path never touches the disk or the
    # network. A path that is not rooted is left to ConvertTo-ProjectKey: resolving it would read
    # this process's current directory into a cwd some other process recorded.
    param([string]$Path)
    if (-not $Path) { return '' }
    $p = $Path.Replace('/', '\')
    if ($p.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) { $p = '\\' + $p.Substring(8) }
    elseif ($p.StartsWith('\\?\') -or $p.StartsWith('\\.\')) { $p = $p.Substring(4) }
    # GetFullPath also expands an 8.3 name ('~') to the long one - which is what makes both spellings
    # of a temp root one key, and is a disk lookup: never asked of a network path.
    if (($p.Length -ge 3 -and $p[1] -eq ':' -and $p[2] -eq '\') -or ($p.StartsWith('\\') -and -not $p.Contains('~'))) {
        try { $p = [IO.Path]::GetFullPath($p) } catch { Write-Verbose "ConvertTo-ProjectScratchKey: '$Path' is not a path: $($_.Exception.Message)" }
    }
    return ConvertTo-ProjectKey $p
}

function Test-ProjectRootKeyDeep {
    # A drive root or a share root is not a scratch root, whoever names it: "everything on B:" is
    # not a temp directory. At least one directory below the drive (or the share) is required.
    param([string]$Key)
    if (-not $Key) { return $false }
    $parts = @($Key.Split('\') | Where-Object { $_ })
    if ($Key.StartsWith('\\')) { return $parts.Count -ge 3 }
    return $parts.Count -ge 2
}

function Get-ProjectTempRoots {
    # Where a throwaway cwd lives, as keys. Both spellings: GetTempPath follows TEMP/TMP, which a
    # shell can point anywhere, and the profile's own Temp is where the other tools on the machine
    # wrote. A candidate is IGNORED when it could not be a temp directory: with TEMP/TMP unset
    # GetTempPath answers the user profile, and a TEMP at a drive root is the whole drive - taken at
    # their word, either would hide every real project. So: never a drive or share root, and never
    # the home directory or anything above it.
    param([string[]]$Candidates, [string]$HomePath = $HOME)
    if (-not $PSBoundParameters.ContainsKey('Candidates')) {
        $Candidates = @([IO.Path]::GetTempPath())
        if ($env:LOCALAPPDATA) { $Candidates += (Join-Path $env:LOCALAPPDATA 'Temp') }
    }
    $homeKey = ConvertTo-ProjectScratchKey $HomePath
    $keys = @()
    foreach ($c in $Candidates) {
        $k = ConvertTo-ProjectScratchKey $c
        if (-not (Test-ProjectRootKeyDeep -Key $k)) { continue }
        if ($homeKey -and ($homeKey -eq $k -or $homeKey.StartsWith($k + '\', [StringComparison]::Ordinal))) { continue }
        if ($keys -notcontains $k) { $keys += $k }
    }
    return @($keys)
}

function Get-ProjectProfileRoots {
    # The profile roots a job tmp directory can sit under, when the caller names none: the default
    # root and the one this process was started for. The launcher passes its whole roster instead.
    $roots = @(Join-Path $HOME '.claude')
    if ($env:CLAUDE_CONFIG_DIR) { $roots += $env:CLAUDE_CONFIG_DIR }
    return @($roots)
}

function Test-ProjectScratchPath {
    # Is this cwd a scratch directory rather than a project: anything at or under a temp root, or a
    # job's tmp directory (<profile root>\jobs\<id>\tmp) and anything under it. String work only -
    # nothing here touches the disk, and both sides go through ConvertTo-ProjectScratchKey, so case,
    # slash direction, a trailing separator, '..', doubled separators and the \\?\ prefix do not
    # matter. -RootsAreKeys: the caller normalised AND depth-checked the roots once (Get-ProjectRegistry,
    # per build).
    param([string]$Path, [string[]]$TempRoots = @(), [string[]]$ProfileRoots = @(), [switch]$RootsAreKeys)
    $key = ConvertTo-ProjectScratchKey $Path
    if (-not $key) { return $false }
    foreach ($root in $TempRoots) {
        $r = if ($RootsAreKeys) { $root } else { ConvertTo-ProjectScratchKey $root }
        # A drive or share root is never a temp root, however it got here (Get-ProjectTempRoots).
        if (-not $RootsAreKeys -and -not (Test-ProjectRootKeyDeep -Key $r)) { continue }
        if ($key -eq $r -or $key.StartsWith($r + '\', [StringComparison]::Ordinal)) { return $true }
    }
    foreach ($root in $ProfileRoots) {
        $r = if ($RootsAreKeys) { $root } else { ConvertTo-ProjectScratchKey $root }
        if (-not $r) { continue }
        $jobs = $r + '\jobs\'
        if (-not $key.StartsWith($jobs, [StringComparison]::Ordinal)) { continue }
        # <id>\tmp[\...]: the id is one segment, and tmp is the whole of the next one.
        $rest = $key.Substring($jobs.Length).Split('\')
        if ($rest.Count -ge 2 -and $rest[0] -and $rest[1] -eq 'tmp') { return $true }
    }
    return $false
}

function Test-ProjectPathPresent {
    # Test-Path that never waits on a network. A real network path - \\host\share, \\?\UNC\..., a
    # path on a network drive - is taken as present WITHOUT being probed: an unreachable host costs a
    # timeout of seconds per row, on the screen the launcher opens with, and picking the row checks it
    # anyway (Invoke-ProjectScreen). \\?\X:\... is a LOCAL path under a prefix and is probed like one.
    # A drive letter that is not there, or a local drive that is not ready, is absent without a look
    # at the path. -DriveCache carries the per-drive answer across one registry build.
    param([string]$Path, [hashtable]$DriveCache = @{})
    if (-not $Path) { return $false }
    $p = $Path
    if ($p -match '^[\\/]{2}[?.][\\/]') {
        $p = $p.Substring(4)
        if ($p -match '^UNC[\\/]') { return $true }
    } elseif ($p.StartsWith('\\') -or $p.StartsWith('//')) { return $true }
    if ($p.Length -ge 2 -and $p[1] -eq ':' -and [char]::IsLetter($p[0])) {
        $letter = [string][char]::ToLowerInvariant($p[0])
        if (-not $DriveCache.ContainsKey($letter)) {
            $DriveCache[$letter] =
                try {
                    $drive = [IO.DriveInfo]::new($letter)
                    # DriveType is answered from the drive table; IsReady on a network drive is the
                    # network call this function exists to avoid, so it is asked of local drives only.
                    if ($drive.DriveType -eq [IO.DriveType]::Network) { 'skip' }
                    elseif ($drive.DriveType -eq [IO.DriveType]::NoRootDirectory -or -not $drive.IsReady) { 'absent' }
                    else { 'probe' }
                } catch { 'absent' }
        }
        if ($DriveCache[$letter] -eq 'skip') { return $true }
        if ($DriveCache[$letter] -eq 'absent') { return $false }
    }
    return [bool](Test-Path -LiteralPath $p)
}

function Get-ProjectRegistry {
    # Every project slug directory, resolved and ordered by last activity. The cache holds only
    # DERIVED data - slug, resolved path, activity - so a lost or corrupt write costs one recompute
    # and never correctness. Atomic temp+move, last writer wins: up to four launchers run at once and
    # a mutex here would buy nothing a rebuild does not already give.
    [CmdletBinding()]
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
        [string]$CachePath = '',
        # What makes a row a scratch directory (Test-ProjectScratchPath). Parameters, so a suite can
        # keep its fixture under the real TEMP and still say which part of it is "temp".
        [string[]]$TempRoots = @(Get-ProjectTempRoots),
        [string[]]$ProfileRoots = @(Get-ProjectProfileRoots)
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

    $out = @(); $fresh = @{}; $drives = @{}
    # The roots, normalised once per build rather than once per row.
    $tempKeys = @($TempRoots | ForEach-Object { ConvertTo-ProjectScratchKey $_ } | Where-Object { Test-ProjectRootKeyDeep -Key $_ })
    $profileKeys = @($ProfileRoots | ForEach-Object { ConvertTo-ProjectScratchKey $_ } | Where-Object { $_ })
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
        # that cannot be entered is worse than a missing one. Never across a network, though
        # (Test-ProjectPathPresent): a dead share must not hold the launcher's first screen.
        if (-not (Test-ProjectPathPresent -Path $path -DriveCache $drives)) { continue }
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
            # A scratch directory - every headless run in a temporary cwd leaves a slug folder - is
            # KNOWN but not listed: the row stays in the registry, so its sessions, its slug and the
            # start-project rule all still work, and the screen leaves it out until a filter asks
            # (Select-ProjectMatch). Judged on the recorded cwd, never on where a link leads.
            Hidden       = ((-not $path.StartsWith('\\')) -and $drives["$($path[0])".ToLowerInvariant()] -ne 'skip' -and (Test-ProjectScratchPath -Path $path -TempRoots $tempKeys -ProfileRoots $profileKeys -RootsAreKeys))
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
            Hidden       = [bool]$top.Hidden
        }
    }
    return @($merged | Sort-Object LastActivity -Descending)
}

function Get-ProjectCount {
    # The title's two numbers, from the same rule that decides the rows: Known is what
    # Select-ProjectMatch lists for this filter and cwd, Hidden is the hidden projects it left out.
    param([Parameter(Mandatory)][AllowEmptyCollection()][array]$Projects, [string]$Filter = '', [string]$Cwd = '')
    $listed = @(Select-ProjectMatch -Projects $Projects -Filter $Filter -Cwd $Cwd)
    $cwdKey = ConvertTo-ProjectKey $Cwd
    # Two linear passes, no membership test: the listed rows are a subset of the registry, so what
    # was left out is the hidden rows of the one minus the hidden rows of the other. The cwd's own
    # project is listed whatever its flag, and is counted in neither.
    $hidden = 0
    foreach ($p in $Projects) { if ($p.Hidden -and -not ($cwdKey -and (ConvertTo-ProjectKey $p.Path) -eq $cwdKey)) { $hidden++ } }
    foreach ($p in $listed) { if ($p.Hidden -and -not ($cwdKey -and (ConvertTo-ProjectKey $p.Path) -eq $cwdKey)) { $hidden-- } }
    return [pscustomobject]@{ Known = $listed.Count; Hidden = $hidden }
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
    #
    # A Hidden project (a scratch directory, Get-ProjectRegistry) is left out of the UNFILTERED list
    # and found by any filter that matches it: the filter box is how a hidden row is reached, so the
    # screen needs no key for it. -Cwd names the one exception - the directory the launcher was
    # started in is always listed, hidden or not.
    param([Parameter(Mandatory)][AllowEmptyCollection()][array]$Projects, [string]$Filter, [string]$Cwd = '')
    # @() on the way out too: `return $Projects` unrolls a one-element array on the pipeline, so the
    # empty-filter branch answered with a bare object where the filtering branch answers with an
    # array. Both call sites wrap the call themselves today; the next one would not know to
    # (adversarial review 2026-09-16, A4).
    if ([string]::IsNullOrWhiteSpace($Filter)) {
        $cwdKey = ConvertTo-ProjectKey $Cwd
        return @($Projects | Where-Object { -not $_.Hidden -or ($cwdKey -and (ConvertTo-ProjectKey $_.Path) -eq $cwdKey) })
    }
    $f = [Management.Automation.WildcardPattern]::Escape($Filter.Trim())
    # The visible matches first, the hidden ones after them, each group in registry order: a scratch
    # path embeds its parent project's slug and is usually the newer of the two, and the cursor parks
    # on the first match - which has to be the project, not a scratch directory that carries its name.
    $shown = [Collections.Generic.List[object]]::new(); $scratch = [Collections.Generic.List[object]]::new()
    foreach ($p in $Projects) {
        if ("$($p.Name) $($p.Path) $($p.Worktree)" -notlike "*$f*") { continue }
        if ($p.Hidden) { $scratch.Add($p) } else { $shown.Add($p) }
    }
    return @($shown) + @($scratch)
}

function Get-ProjectRows {
    # The project screen's rows in cursor order: the current directory, the registry matches, the
    # free-path row. The ONE builder Get-ProjectFrame draws and Invoke-ProjectScreen picks from, so a
    # pick can never land on a row other than the one the cursor is drawn on.
    #
    # One directory, one row: when the current directory IS a known project, the cwd row carries that
    # project (its name, age, worktree mark and slugs) and the project leaves the list below - the
    # same folder listed twice, once as 'current directory' and once by name, was the owner's report.
    # Match says the cwd's project is among the filter's matches, so a search for it parks on row 0.
    param([Parameter(Mandatory)][AllowEmptyCollection()][array]$Projects, [string]$Filter = '', [string]$Cwd = '')
    $cwdKey = ConvertTo-ProjectKey $Cwd
    $here = if ($cwdKey) { @($Projects | Where-Object { (ConvertTo-ProjectKey $_.Path) -eq $cwdKey }) | Select-Object -First 1 } else { $null }
    $matched = @(Select-ProjectMatch -Projects $Projects -Filter $Filter -Cwd $Cwd)
    $cwdItem = if ($here) {
        [pscustomobject]@{ Name = "$($here.Name) (current directory)"; Path = $Cwd; LastActivity = $here.LastActivity; Worktree = $here.Worktree
                           Slug = $here.Slug; Slugs = @(if ($here.Slugs) { $here.Slugs } else { $here.Slug })
                           Match = [bool]($Filter -and @($matched | Where-Object { (ConvertTo-ProjectKey $_.Path) -eq $cwdKey }).Count) }
    } else {
        [pscustomobject]@{ Name = 'current directory'; Path = $Cwd; LastActivity = $null; Worktree = $null; Slug = ''; Slugs = @(); Match = $false }
    }
    $rows = @([pscustomobject]@{ Kind = 'cwd'; Item = $cwdItem })
    $rows += @($matched | Where-Object { -not $here -or (ConvertTo-ProjectKey $_.Path) -ne $cwdKey } | ForEach-Object { [pscustomobject]@{ Kind = 'project'; Item = $_ } })
    $rows += [pscustomobject]@{ Kind = 'path'; Item = [pscustomobject]@{ Name = 'enter a path...'; Path = ''; LastActivity = $null; Slug = ''; Slugs = @() } }
    return @($rows)
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
    # Through Test-ProjectPathPresent: this runs before the first screen and again on every tab
    # switch, and a remembered project on an unreachable share must not cost a network timeout each
    # time. The launch itself still refuses a directory it cannot enter.
    if ($Remembered -and (Test-ProjectPathPresent -Path $Remembered)) {
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
    # Written unconditionally, exactly like Project and ProjectSlug above it. A guard on
    # PSObject.Properties looked defensive and was worse: on a state that did not carry the property
    # it SILENTLY dropped the write instead of failing (re-review 2026-09-16, W3). Add-Member -Force
    # is one statement that sets it whether or not the property is already there.
    $slugs = @()
    if ($State.Project) {
        $key = ConvertTo-ProjectKey $State.Project
        $hit = @($Projects | Where-Object { (ConvertTo-ProjectKey $_.Path) -eq $key })
        if ($hit.Count -gt 0) {
            $State.ProjectSlug = $hit[0].Slug
            # EVERY slug of that directory: one row can carry several (Get-ProjectRegistry).
            $slugs = @($hit | ForEach-Object { if ($_.Slugs) { $_.Slugs } else { $_.Slug } })
        }
    }
    $State | Add-Member -NotePropertyName ProjectSlugs -NotePropertyValue $slugs -Force
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
