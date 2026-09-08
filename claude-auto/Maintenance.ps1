# Update, health and disk housekeeping for the Claude Code install.
#
# The reason this is a module rather than a shell-out: `claude update` on Windows prints
# "Successfully updated from X to version Y" and exits 0 even when the copy over the running
# claude.exe was denied - Windows refuses to write a loaded image. Measured 2026-08-10; a version
# sat stuck at 2.1.225 for two days that way. So the answer here always comes from Get-FileHash,
# never from the updater's output, and never from `claude --version` (which reports the image the
# running session already loaded).

function Get-OrderedClaudeBuilds {
    # Single source of truth for "newest", shared by Get-ClaudeInstallInfo and
    # Remove-OldClaudeVersions - version order beats file time because a rollback re-downloads an
    # older build, which then carries the newest timestamp and would otherwise look newest.
    param([array]$Builds)
    $parseable = @()
    $unparseable = @()
    foreach ($b in $Builds) {
        $v = $null
        if ([version]::TryParse($b.Name, [ref]$v)) { $parseable += [pscustomobject]@{ Item = $b; Version = $v } }
        else { $unparseable += $b }
    }
    return @($parseable | Sort-Object Version -Descending | ForEach-Object { $_.Item }) +
           @($unparseable | Sort-Object LastWriteTime -Descending)
}

$script:HashCachePath = Join-Path $HOME '.claude\claude-auto-hash-cache.json'
$script:HashCacheMinBytes = 1MB

function Get-CachedFileHash {
    # The hash above is load-bearing and cannot be replaced by a cheaper signal - the header says
    # why. It can be CACHED exactly, though, and it has to be: claude.exe is 305 MB, one Get-FileHash
    # costs 279 ms, this runs on TWO builds at every launch, and that was 569 ms of the launcher's
    # 1 560 ms (measured 2026-08-15).
    #
    # The key is length + write time, which is exact rather than approximate: a build swap always
    # rewrites the file, and the rename-swap in this very module changes both. Nothing else on this
    # machine edits a 305 MB binary in place while preserving its timestamp to the tick.
    #
    # Small files are NEVER cached. Their hash is instant, so there is nothing to win, and the
    # suites' fixtures are tiny files rewritten many times a second - caching those would put the
    # tests at the mercy of timestamp granularity, which is how a cache turns a green suite into a
    # liar.
    #
    # Up to four launchers can run at once on this machine, so this is last-writer-wins by design:
    # a torn read is caught and the entry is simply recomputed.
    param([Parameter(Mandatory)][string]$Path)
    $item = Get-Item -LiteralPath $Path -ErrorAction SilentlyContinue
    if (-not $item) { return $null }
    # A file that exists but cannot be READ - an antivirus or indexer holding a just-downloaded
    # build, or the other half of a swap in flight - answers $null, exactly like a missing one. The
    # bare Get-FileHash threw a four-line red PowerShell error straight into the maintenance screen,
    # and every caller here already treats $null as "unknown".
    if ($item.Length -lt $script:HashCacheMinBytes) {
        try { return (Get-FileHash -LiteralPath $Path -ErrorAction Stop).Hash } catch { return $null }
    }

    $key = '{0}|{1}|{2}' -f $item.FullName, $item.Length, $item.LastWriteTimeUtc.Ticks
    $cache = @{}
    try {
        (Get-Content -LiteralPath $script:HashCachePath -Raw -ErrorAction Stop | ConvertFrom-Json).PSObject.Properties |
            ForEach-Object { $cache[$_.Name] = $_.Value }
    } catch { $cache = @{} }
    if ($cache.ContainsKey($key)) { return $cache[$key] }

    # Same as the small-file path above: unreadable answers $null and nothing is cached, so the next
    # call retries instead of remembering a failure.
    try { $hash = (Get-FileHash -LiteralPath $Path -ErrorAction Stop).Hash } catch { return $null }
    $cache[$key] = $hash
    # Drop entries whose file is gone or has changed, so retired builds do not accumulate forever.
    $live = @{}
    foreach ($k in $cache.Keys) {
        $parts = $k -split '\|'
        if ($parts.Count -ne 3) { continue }
        $f = Get-Item -LiteralPath $parts[0] -ErrorAction SilentlyContinue
        if ($f -and "$($f.Length)" -eq $parts[1] -and "$($f.LastWriteTimeUtc.Ticks)" -eq $parts[2]) { $live[$k] = $cache[$k] }
    }
    try { $live | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $script:HashCachePath -Encoding utf8 } catch { }
    return $hash
}

function Get-DefaultClaudeBinPath {
    # deferred review finding: ~/.local/bin/claude.exe is only true for the NATIVE installer. An npm
    # or global install has neither that file nor ~/.local/share/claude/versions, so the maintenance
    # screen showed no hash and 'u' reported the download message even after a real update. Resolve
    # where `claude` actually runs from first, and fall back to the native path only when PATH has
    # nothing to say (e.g. this screen open before a first login).
    $cmd = Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) { return $cmd.Source }
    return (Join-Path $HOME '.local\bin\claude.exe')
}

function Get-ClaudeInstallInfo {
    param(
        [string]$BinPath = (Get-DefaultClaudeBinPath),
        [string]$VersionsDir = (Join-Path $HOME '.local\share\claude\versions')
    )
    $installedHash = $null
    if (Test-Path -LiteralPath $BinPath) { $installedHash = Get-CachedFileHash -Path $BinPath }

    # Whether the versions directory exists at all - not whether it has anything in it - is what
    # tells a native install (nothing downloaded yet) apart from an npm/global one (no such
    # directory, ever). Invoke-ClaudeUpdate's message depends on this distinction.
    $native = Test-Path -LiteralPath $VersionsDir
    $builds = @()
    if ($native) {
        $builds = @(Get-OrderedClaudeBuilds -Builds (Get-ChildItem -LiteralPath $VersionsDir -File -ErrorAction SilentlyContinue))
    }
    $newest = $builds | Select-Object -First 1
    $newestHash = if ($newest) { Get-CachedFileHash -Path $newest.FullName } else { $null }

    return [pscustomobject]@{
        BinPath       = $BinPath
        InstalledHash = $installedHash
        NewestVersion = if ($newest) { $newest.Name } else { $null }
        NewestPath    = if ($newest) { $newest.FullName } else { $null }
        NewestHash    = $newestHash
        Matches       = ($installedHash -and $newestHash -and $installedHash -eq $newestHash)
        VersionCount  = $builds.Count
        VersionsBytes = ($builds | Measure-Object Length -Sum).Sum
        VersionsDir   = $VersionsDir
        Native        = $native
    }
}

function Invoke-ClaudeUpdate {
    # Runs the updater, ignores what it said, re-hashes - and when the hash proves the copy did not
    # land, finishes the job by rename instead of handing the reader a chore.
    #
    # Measured again 2026-08-13: `claude update` exits 0 and prints "Successfully updated from
    # 2.1.229 to version 2.1.231" while claude.exe stays 2.1.229, because Windows refuses to
    # overwrite a loaded image and the updater never checks the copy. The download IS correct, so
    # everything needed to install it is already on disk - which is exactly what
    # Repair-ClaudeBinaryByRename does. Before this, pressing u reported that failure and told the
    # reader to press r; the message was longer than the box and got truncated before the word 'r',
    # so 'update' was a dead end with no visible way out.
    #
    # -Updater is injected so this whole chain is assertable against a fake tree - the real one
    # downloads ~300 MB and mutates the live installation.
    param([string]$BinPath = (Get-DefaultClaudeBinPath),
          [string]$VersionsDir = (Join-Path $HOME '.local\share\claude\versions'),
          [scriptblock]$Updater = { & claude update 2>&1 })
    $output = ''
    try { $output = (& $Updater | Out-String).Trim() }
    catch { return [pscustomobject]@{ Ran = $false; Output = $_.Exception.Message; Matches = $false; Swapped = $false; Message = "the updater could not be started: $($_.Exception.Message)" } }

    $info = Get-ClaudeInstallInfo -BinPath $BinPath -VersionsDir $VersionsDir
    if ($info.Matches) {
        return [pscustomobject]@{ Ran = $true; Output = $output; Matches = $true; Swapped = $false
            Message = "installed build now matches $($info.NewestVersion) (verified by hash)" }
    }

    $swap = Repair-ClaudeBinaryByRename -BinPath $BinPath -VersionsDir $VersionsDir
    $after = Get-ClaudeInstallInfo -BinPath $BinPath -VersionsDir $VersionsDir
    $message =
        if ($after.Matches) {
            "the updater could not overwrite the running claude.exe, so it was installed by rename: now $($after.NewestVersion), verified by hash. New windows get it; sessions already open keep the old build, and claude.exe.old stays locked until they all exit."
        } elseif (-not $info.Native) {
            # deferred review finding: an npm/global install has no versions directory at all, so
            # this used to show the download message even right after a real, successful update -
            # 'u' looked like a dead end instead of naming the actual situation.
            'not a native install; updates are handled by your installer'
        } elseif (-not $info.NewestPath) {
            'nothing to install: no build has been downloaded. Check the connection and press u again.'
        } else {
            "the updater did not land and the rename swap failed too: $($swap.Message). Close every Claude Code session - including any a scheduled task starts - and press u again."
        }
    return [pscustomobject]@{ Ran = $true; Output = $output; Matches = $after.Matches; Swapped = ($after.Matches -and -not $info.Matches); Message = $message }
}

function Invoke-MaintenanceScript {
    # Runs one configured maintenance action (a script the config names) from the maintenance
    # screen and reduces its output to a status line.
    #
    # These are typically minutes, not seconds. The caller draws a "running" frame first and, when
    # the action says so, asks for confirmation, because a menu that freezes for minutes with no
    # warning reads as a hung launcher.
    #
    # -Runner is injected so the whole path is assertable without running anything on the machine.
    param(
        [Parameter(Mandatory)][string]$ScriptPath,
        [string]$Label = 'script',
        [scriptblock]$Runner = $null
    )
    # Checked before any runner, injected or not: production semantics must not depend on a test seam.
    if (-not (Test-Path -LiteralPath $ScriptPath)) {
        return [pscustomobject]@{ Ran = $false; Ok = $false; Message = "$Label script not found at $ScriptPath - nothing was run." }
    }
    if (-not $Runner) {
        $Runner = { param($p) $out = & pwsh -NoProfile -File $p 2>&1; return [pscustomobject]@{ Output = $out; ExitCode = $LASTEXITCODE } }
    }
    try {
        $r = & $Runner $ScriptPath
    } catch {
        return [pscustomobject]@{ Ran = $false; Ok = $false; Message = "$Label could not start: $($_.Exception.Message)" }
    }
    $text = ConvertTo-StatusText $r.Output
    # Exit 2 is "could not check" everywhere in this toolchain and must never read as success.
    $verdict =
        if ($r.ExitCode -eq 0) { "$Label finished green" }
        elseif ($r.ExitCode -eq 2) { "$Label COULD NOT RUN (exit 2) - not a pass" }
        else { "$Label FAILED (exit $($r.ExitCode))" }
    $tail = if ($text) { ($text -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last 3) -join ' | ' } else { '' }
    return [pscustomobject]@{
        Ran = $true
        Ok = ($r.ExitCode -eq 0)
        Message = if ($tail) { "$verdict - $tail" } else { $verdict }
    }
}

function Repair-ClaudeBinaryByRename {
    # Windows forbids overwriting a running image but allows RENAMING it: the live session keeps
    # its old inode, new launches get the new build, and claude.exe.old stays locked until every
    # session exits.
    param([string]$BinPath = (Get-DefaultClaudeBinPath),
          [string]$VersionsDir = (Join-Path $HOME '.local\share\claude\versions'))
    $info = Get-ClaudeInstallInfo -BinPath $BinPath -VersionsDir $VersionsDir
    # BinPath is wherever `claude` RESOLVES, which on a machine with an npm or global install is a
    # shim - claude.cmd, claude.ps1 - and this function's payload is raw native build bytes. Copying
    # them over a shim destroys it and leaves nothing that runs. A leftover versions directory from
    # a native install tried once is all it takes to reach here, so the guard is on the target, not
    # on whether builds happen to exist.
    if ([IO.Path]::GetExtension($BinPath) -ne '.exe') {
        return [pscustomobject]@{ Ok = $false; Message = 'not a native install; updates are handled by your installer' }
    }
    if (-not $info.NewestPath) { return [pscustomobject]@{ Ok = $false; Message = 'no downloaded build to install' } }
    if ($info.Matches) { return [pscustomobject]@{ Ok = $true; Message = 'already on the newest build' } }
    # Copy FIRST, move second. The old order renamed the working binary aside and only then copied,
    # so any copy failure - an antivirus or a search indexer holding the just-downloaded build is the
    # everyday one - left the user with no claude at all and a claude.exe.old nobody mentioned. Now
    # nothing moves until the new build is on disk beside the old one, and a failure after that
    # renames the original back.
    $new = "$BinPath.new"
    $old = "$BinPath.old"
    try {
        if (Test-Path -LiteralPath $new) { Remove-Item -LiteralPath $new -Force -ErrorAction SilentlyContinue }
        Copy-Item -LiteralPath $info.NewestPath -Destination $new -ErrorAction Stop
    } catch {
        Remove-Item -LiteralPath $new -Force -ErrorAction SilentlyContinue
        return [pscustomobject]@{ Ok = $false; Message = "rename swap failed before anything moved, the installed build is untouched: $($_.Exception.Message)" }
    }
    $renamed = $false
    try {
        if (Test-Path -LiteralPath $old) { Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue }
        Rename-Item -LiteralPath $BinPath -NewName (Split-Path $old -Leaf) -ErrorAction Stop
        $renamed = $true
        Rename-Item -LiteralPath $new -NewName (Split-Path $BinPath -Leaf) -ErrorAction Stop
    } catch {
        $reason = $_.Exception.Message
        if ($renamed -and -not (Test-Path -LiteralPath $BinPath)) {
            try { Rename-Item -LiteralPath $old -NewName (Split-Path $BinPath -Leaf) -ErrorAction Stop } catch { }
        }
        Remove-Item -LiteralPath $new -Force -ErrorAction SilentlyContinue
        $back = if (Test-Path -LiteralPath $BinPath) { 'the working binary was put back' }
                else { "the working binary is at $old and must be renamed back by hand" }
        return [pscustomobject]@{ Ok = $false; Message = "rename swap failed: $reason; $back" }
    }
    $after = Get-ClaudeInstallInfo -BinPath $BinPath -VersionsDir $VersionsDir
    return [pscustomobject]@{ Ok = $after.Matches; Message = if ($after.Matches) { "swapped to $($after.NewestVersion), verified by hash" } else { 'the swap ran but the hashes still differ' } }
}

function Remove-OldClaudeVersions {
    # ~280 MB per build and nothing ever prunes them: four builds are 1.1 GB. Keep the current one
    # plus one rollback.
    param(
        [string]$VersionsDir = (Join-Path $HOME '.local\share\claude\versions'),
        [int]$Keep = 2,
        [string]$BinPath = (Get-DefaultClaudeBinPath)
    )
    # A build is a file whose NAME is a version, and nothing else. This directory belongs to the
    # updater, not to us: it also holds partial downloads a concurrent launcher is still writing,
    # and whatever the user has dropped there. Worse, Get-OrderedClaudeBuilds sorts unparseable
    # names LAST, so without this filter they were the first things pruned.
    $builds = @(Get-ChildItem -LiteralPath $VersionsDir -File -ErrorAction SilentlyContinue |
        Where-Object { $v = $null; [version]::TryParse($_.Name, [ref]$v) })
    $ordered = @(Get-OrderedClaudeBuilds -Builds $builds)

    # The installed build is the only one that cannot be re-obtained by rolling back, so it is never
    # a prune candidate regardless of its version - and if it cannot be IDENTIFIED, nothing is
    # pruned at all. Failing open here deleted the running build: the guard used to sit inside a
    # `Test-Path $BinPath`, and an absent binary is not exotic, it is the window between the two
    # renames of a swap, which a second launcher on the same machine can walk straight into.
    #
    # ONE guard, not two. A missing binary and a binary matching no build were separate checks, and
    # a mutation run showed the second silently masking the first - Get-FileHash on a missing file
    # is a non-terminating error whose $null hash then matches nothing. Two mechanisms for one
    # condition means neither is really tested. Get-CachedFileHash already answers $null for missing
    # AND unreadable, which is exactly the question being asked here.
    $unidentified = [pscustomobject]@{ Deleted = @(); FreedBytes = [long]0
        Message = 'could not identify the running build - nothing pruned' }
    $installedHash = Get-CachedFileHash -Path $BinPath
    $installedBuild = @($ordered | Where-Object { $installedHash -and (Get-CachedFileHash -Path $_.FullName) -eq $installedHash }) | Select-Object -First 1
    if (-not $installedBuild) { return $unidentified }

    $doomed = @($ordered | Select-Object -Skip $Keep | Where-Object { $_.FullName -ne $installedBuild.FullName })
    $freed = ($doomed | Measure-Object Length -Sum).Sum
    foreach ($d in $doomed) { Remove-Item -LiteralPath $d.FullName -Force -ErrorAction SilentlyContinue }
    return [pscustomobject]@{ Deleted = @($doomed.Name); FreedBytes = [long]$freed; Message = '' }
}
