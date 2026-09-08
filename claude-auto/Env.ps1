# Profile, secrets, shared-link repair and MCP discovery for the claude-auto launcher.
#
# Everything here was lifted out of claude-auto.ps1 unchanged, comments included - those comments
# record measurements (why two hardlink names mean "still shared", why only the MCP endpoint
# answers text/event-stream, why the port scan runs in parallel) and rewording them loses the
# evidence. Behaviour must stay byte-identical: the reference output captured before the move is
# the check.

. (Join-Path $PSScriptRoot 'Config.ps1')
$script:LauncherConfig = Read-LauncherConfig
$Accounts = @($script:LauncherConfig.Accounts)
$CanonicalAccount = (@($Accounts | Where-Object Canonical) | Select-Object -First 1).Key
# 'work' in the old tables = the canonical root = the absence of CLAUDE_CONFIG_DIR. Same names, now derived.
$WorkRoot = (@($Accounts | Where-Object Canonical) | Select-Object -First 1).Root
# Every root that shares the canonical root's files. The canonical one is deliberately absent: it
# is the link target, not a link.
$SecondaryRoots = @($Accounts | Where-Object { -not $_.Canonical } | ForEach-Object Root)
# Key -> root / launch label / tint, from the config. A key missing from a table would print an
# empty label and a $null tint, so all three are filled from the same roster in one pass.
$ProfileRoots = [ordered]@{}; $ProfileLabels = @{}; $ProfileTints = @{}
foreach ($a in $Accounts) { $ProfileRoots[$a.Key] = $a.Root; $ProfileLabels[$a.Key] = $a.Label; $ProfileTints[$a.Key] = $a.Tint }
# CLAUDE.md is deliberately NOT shared: Claude Code reads ~/.claude/CLAUDE.md even
# when CLAUDE_CONFIG_DIR points elsewhere, so a personal copy made every personal
# session load the same ~4.6k tokens twice (found 2026-07-31). Single copy in ~/.claude.
$SharedFiles = @('settings.json', 'statusline.js')
$SharedDirs = @('projects', 'plugins', 'hooks', 'agents', 'skills', 'rules', 'sessions', 'file-history', 'session-env', 'tasks', 'shell-snapshots')

# One JSONL file per day under ~/.claude/launcher-logs. Deliberately NOT under either profile's
# config directory: the launcher runs before the account is chosen, and a log that moves with the
# profile could not answer "which account did this launch pick".
$LauncherLogRoot = Join-Path $WorkRoot 'launcher-logs'

function Get-LauncherParentChain {
    # Who started this launcher. The single most valuable field in the log: a session that
    # misbehaves at startup is usually spawned by something the terminal does not show - the Rider
    # plugin (claudeCommand=claude-auto), the ACP agent Rider ships with its own bundled claude.exe,
    # crc, or a bare `claude` typed by hand. Only the parent chain tells those apart afterwards.
    #
    # Starts at THIS process, not the parent: the pwsh command line records how the launcher itself
    # was invoked (`claude-auto` shim vs `pwsh -File ... -p`), which is half the answer on its own.
    param([int]$From = $PID, [int]$Depth = 6, [int]$CommandLineLimit = 400)
    $chain = @()
    try {
        $id = $From
        for ($i = 0; $i -lt $Depth -and $id -gt 0; $i++) {
            $p = Get-CimInstance Win32_Process -Filter "ProcessId=$id" -ErrorAction Stop
            if (-not $p) { break }
            $cmd = [string]$p.CommandLine
            if ($cmd.Length -gt $CommandLineLimit) { $cmd = $cmd.Substring(0, $CommandLineLimit) + '...' }
            $chain += [ordered]@{ pid = [int]$p.ProcessId; name = [string]$p.Name; cmd = $cmd }
            $id = [int]$p.ParentProcessId
        }
    } catch { }   # a chain we could not walk is still a log worth writing
    # Plain return, not `return ,$chain`: the comma wraps the array in another array, and the
    # pipeline then unrolls only the outer one - so `@(Get-LauncherParentChain)` came back as a
    # single Object[] element instead of the six process records. Callers force array context
    # themselves with @(...), which is also what makes the empty case read as an empty list.
    return $chain
}

function Remove-OldLauncherLogs {
    param([string]$Root = $LauncherLogRoot, [int]$Days = 14)
    try {
        if (-not (Test-Path -LiteralPath $Root)) { return }
        $cut = (Get-Date).AddDays(-$Days)
        Get-ChildItem -LiteralPath $Root -Filter 'claude-auto-*.jsonl' -File -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTime -lt $cut } |
            Remove-Item -Force -ErrorAction SilentlyContinue
    } catch { }
}

function Write-LauncherLog {
    # Append-only, one JSON object per line, fail-open and SILENT.
    #
    # Silent is a hard requirement, not a preference: check-launcher-regression.ps1 compares the
    # launcher's console output against a stored reference, so a single Write-Host here would redden
    # a check that exists to catch real regressions.
    #
    # Append rather than read-modify-write, with a short retry: several Claude Code instances can
    # launch at once, so two launchers WILL write to the same file within milliseconds. AppendAllText opens for append and closes; the retry covers the
    # sharing violation window. Losing a line is acceptable; blocking or throwing at launch is not.
    param(
        [Parameter(Mandatory)][string]$Stage,
        [hashtable]$Data = @{},
        [string]$RunId,
        [string]$Root = $LauncherLogRoot
    )
    try {
        # -ErrorAction Stop on both: a bad root (an unusable path, a name Windows rejects) raises a
        # NON-terminating error by default, which the catch below would miss and the console would
        # then carry the noise this logger must never produce.
        if (-not (Test-Path -LiteralPath $Root -ErrorAction Stop)) {
            New-Item -ItemType Directory -Force -Path $Root -ErrorAction Stop | Out-Null
        }
        $record = [ordered]@{
            ts    = (Get-Date).ToString('o')
            stage = $Stage
            run   = $RunId
            pid   = $PID
        }
        foreach ($k in $Data.Keys) { $record[$k] = $Data[$k] }
        # -Compress keeps one record on one line, which is what makes the file greppable and what
        # every reader below assumes. Depth 6 covers the parent chain (array of maps) with room.
        $line = ($record | ConvertTo-Json -Depth 6 -Compress) + "`r`n"
        $file = Join-Path $Root ('claude-auto-{0:yyyy-MM-dd}.jsonl' -f (Get-Date))
        $utf8 = New-Object System.Text.UTF8Encoding($false)
        for ($i = 0; $i -lt 6; $i++) {
            try {
                [IO.File]::AppendAllText($file, $line, $utf8)
                return $true
            } catch [IO.IOException] {
                Start-Sleep -Milliseconds (15 * ($i + 1))
            }
        }
        return $false
    } catch { return $false }
}

function Set-ClaudeProfile {
    # The canonical account is the absence of CLAUDE_CONFIG_DIR, which is also why it stays the
    # default: a launcher fed `< NUL` takes the defaults and runs under that account.
    # -Mirror is injected so the sharing path is assertable without running node against a real
    # profile root - the same seam shape as -Updater and -Runner in Maintenance.ps1.
    param(
        [Parameter(Mandatory)][string]$Account,
        [switch]$Preview,
        [scriptblock]$Mirror = { param($Root) & node (Join-Path $PSScriptRoot '..\sharing\claude-mirror-mcp.mjs') $Root 2>&1 }
    )
    if (-not $ProfileRoots.Contains($Account)) { throw "unknown account '$Account' (roster: $($ProfileRoots.Keys -join ', '))" }
    $label = $ProfileLabels[$Account]; $tint = $ProfileTints[$Account]
    if ($Account -eq $CanonicalAccount) {
        Remove-Item Env:CLAUDE_CONFIG_DIR -ErrorAction SilentlyContinue
        Write-Host "-> $label" -ForegroundColor $tint
        return
    }
    $root = $ProfileRoots[$Account]
    $env:CLAUDE_CONFIG_DIR = $root
    Write-Host "-> $label" -ForegroundColor $tint
    if (-not $script:LauncherConfig.Sharing) { return }
    # Setting CLAUDE_CONFIG_DIR above is this process's own environment and dies with it, so preview
    # may do it. The mirror below is different: it REWRITES the target account's .claude.json. Three
    # comments in this repo assert the preview path is side-effect-free, and this was the place where
    # it was not - a dry run rewrote a real profile's file wholesale, through node, on a machine
    # where the reader had asked for nothing to happen.
    if ($Preview) { return }
    # projects[*].mcpServers cannot live in a shared file (.claude.json also holds the account identity):
    # carry it over here, just before the session starts. One direction, canonical root is the source.
    # The target root is passed explicitly: hardcoding one root is how a third account would have
    # silently lost per-project servers, which is the exact bug this mirror was written for.
    try {
        $mirrored = & $Mirror $root
        if ($LASTEXITCODE -eq 0) {
            if ($mirrored) { Write-Host "  project MCP mirrored: $mirrored" -ForegroundColor DarkGray }
        } else {
            # A non-zero exit used to print NOTHING - the exact silent failure this mirror exists to
            # prevent, since the owner would never learn a project's MCP servers stopped syncing.
            # The script's own stdout (merged with stderr above) already carries a one-line reason.
            Write-Host "  project MCP mirror failed: $mirrored" -ForegroundColor DarkYellow
        }
    } catch { Write-Host "  project MCP mirror skipped: $($_.Exception.Message)" -ForegroundColor DarkYellow }
}

function Resolve-ClaudeExecutable {
    # `claude` missing from PATH used to end in two raw PowerShell errors and exit 0: `.Source` on a
    # $null match (no -ErrorAction on Get-Command) is $null, `& $null @args` throws, and the caller's
    # `exit $claudeExit` with that variable never set exits 0 - a failed launch reporting success.
    # -Resolver is injected so this is assertable without depending on the real PATH.
    param([scriptblock]$Resolver = { Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1 })
    $cmd = & $Resolver
    if ($cmd) { return [pscustomobject]@{ Ok = $true; Path = $cmd.Source; Message = $null } }
    return [pscustomobject]@{ Ok = $false; Path = $null; Message = 'claude is not on PATH - install Claude Code first' }
}

function Get-AccountPrompt {
    # The fallback (no-UI) account prompt, generated from the roster. Hidden accounts stay typeable
    # through Map but are not advertised in Text; fewer than two visible accounts means no prompt.
    param([Parameter(Mandatory)][object[]]$Accounts, [Parameter(Mandatory)][string]$Default)
    $map = @{}
    foreach ($a in $Accounts) { $map[$a.Key.Substring(0, 1).ToLowerInvariant()] = $a.Key }
    $visible = @($Accounts | Where-Object { -not $_.Hidden })
    if ($visible.Count -lt 2) { return @{ Text = ''; Map = $map } }
    # The bracketed letter must match the case Map keys on (lower), or a mixed-case config key
    # (e.g. "Work") advertises "[W]ork" for a letter the map only accepts as lower-case 'w'.
    $parts = @($visible | ForEach-Object { "[$($_.Key.Substring(0,1).ToLowerInvariant())]$($_.Key.Substring(1))" })
    return @{ Text = "Claude account: $($parts -join ' / '), Enter = $Default : "; Map = $map }
}

function Resolve-AccountAnswer {
    # What the no-UI fallback prompt does with a typed answer: trim, lower-case, and look up only
    # the FIRST character - so typing a key in full ('personal') resolves the same as its one-letter
    # shorthand ('p'), and a hidden account (typeable but not advertised in Text) resolves the same
    # way as a visible one, since -Prompt (Get-AccountPrompt's Map) carries every account's letter.
    # An empty, whitespace-only or unmapped answer returns -Default unchanged - the bare-Enter case.
    param([string]$Answer, [Parameter(Mandatory)][hashtable]$Prompt, [string]$Default)
    $a = "$Answer".Trim().ToLowerInvariant()
    if ($a -and $Prompt.ContainsKey($a.Substring(0, 1))) { return $Prompt[$a.Substring(0, 1)] }
    return $Default
}

function Import-ProjectSecrets {
    # One place holds a credential, and it is never a config file. The secrets store (a directory
    # beside the profile, `secretsRoot` in the config) uses the same slug Claude Code uses for
    # ~/.claude/projects, so the two line up when you go looking.
    #
    # The convention is deliberately trivial: FILE NAME IS THE ENVIRONMENT VARIABLE NAME, content
    # is the value. No mapping table to keep in sync, and adding a secret is creating a file.
    #
    # Why this exists at all: a token written literally into `.claude.json` spreads, because Claude
    # Code snapshots that file into backups, file-history and every session transcript - one such
    # token was later found in 491 places across 45 files, including transcripts of an unrelated
    # repository. A value referenced as ${VAR} cannot spread that way.
    #
    # Three tiers, later beating earlier: shared -> org -> project. The org tier is opted into by a
    # JUNCTION at <slug>/org, never by the loader guessing which repositories belong to which
    # organization. While only the project tier was read, a shared-tier secret was silently dead:
    # correctly installed, never loaded, and nothing said so.
    #
    # Tests: Test-Env.ps1 (15 assertions, throwaway store under $env:TEMP).
    param(
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [string]$Root = $script:LauncherConfig.SecretsRoot
    )
    $secretsSlug = ($WorkingDirectory -replace '[:\\/.]', '-')
    $tiers = [ordered]@{
        shared  = Join-Path $Root 'shared'
        org     = Join-Path $Root "$secretsSlug\org"
        project = Join-Path $Root $secretsSlug
    }

    $winner = [ordered]@{}
    foreach ($tier in $tiers.Keys) {
        $dir = $tiers[$tier]
        if (-not (Test-Path $dir)) { continue }
        # One environment variable per FILE, named after the file - so an index file placed in a
        # tier would become $env:README.md. The charter lives at the tree root for
        # that reason, and this skip is the guard, because a convention nobody can break is not a
        # convention that depends on nobody adding a readme.
        foreach ($f in Get-ChildItem $dir -File | Where-Object { $_.Extension -ne '.md' }) {
            # -Raw then Trim: a trailing newline is not part of a token, and an invisible one turns
            # HTTP Basic auth into a 401 that reads like the credential is wrong.
            #
            # The null check is load-bearing, not defensive: `Get-Content -Raw` on a ZERO-BYTE file
            # returns $null, `.Trim()` on it raises a NON-terminating error, and $value then still
            # holds the PREVIOUS file's value - so an empty secret file silently inherited another
            # secret's value. Present in this function since it was written; caught 2026-08-13 by
            # Test-Env.ps1's "an empty file sets nothing".
            $value = Get-Content $f.FullName -Raw
            if ($null -eq $value) { continue }
            $value = $value.Trim()
            if (-not $value) { continue }
            Set-Item -Path "Env:$($f.Name)" -Value $value
            $winner[$f.Name] = $tier
        }
    }

    # Some checkers take a token's PATH rather than its value, so this one name also gets a _FILE
    # variable. Resolved against the tier that actually won, so a project token is not silently
    # shadowed by a shared one.
    $sonarFile = @($tiers.Values) |
        ForEach-Object { Join-Path $_ 'SONARQUBE_TOKEN' } |
        Where-Object { Test-Path $_ } |
        Select-Object -Last 1
    if ($sonarFile) { $env:SONAR_TOKEN_FILE = $sonarFile }

    # Report the tier beside each name: an unexpected shared value is then visible at launch rather
    # than diagnosed later as a wrong credential.
    $loaded = @($winner.Keys | ForEach-Object { "$_($($winner[$_]))" })
    if ($loaded.Count -gt 0) {
        Write-Host "  secrets loaded: $($loaded -join ', ')" -ForegroundColor DarkGray
    }
    return $loaded
}

function Get-SharedFileId {
    # NTFS File ID = identity. Equal contents do NOT mean one file, and two paths listed by
    # `fsutil hardlink list` only prove that SOME pair shares an inode - see the comment in
    # Repair-SharedLink for why that distinction stopped being cosmetic at three accounts.
    param([string]$Path)
    $raw = & fsutil.exe file queryfileid $Path 2>&1
    if ($LASTEXITCODE -ne 0) { return $null }
    $m = [regex]::Match(($raw -join ' '), '0x[0-9a-fA-F]+')
    if (-not $m.Success) { return $null }
    return $m.Value
}

function Repair-SharedLink {
    # Same file in every profile, or the profiles have started to drift silently; re-link the
    # divergent copies from whichever one is NEWEST.
    #
    # This used to test `@(fsutil hardlink list $work).Count -ge 2` and return early. That is
    # correct for exactly two accounts and WRONG for three: with work+personal still linked and
    # the third copy replaced by an atomic write, the count is still 2, the check passes, and the
    # third profile drifts forever - the precise failure the whole mechanism exists to prevent.
    # File IDs are compared per root instead, which is identity rather than "somebody shares".
    param([string]$Name, [string[]]$Roots = (@($WorkRoot) + $SecondaryRoots))
    $paths = @($Roots | ForEach-Object { Join-Path $_ $Name } | Where-Object { Test-Path $_ })
    if ($paths.Count -lt 2) { return }

    $ids = @{}
    foreach ($p in $paths) {
        $id = Get-SharedFileId -Path $p
        # No File ID means the question was not answered - never treat that as "linked".
        if (-not $id) { Write-Host "  could not read the File ID of $p - link state unknown for $Name" -ForegroundColor DarkYellow; return }
        $ids[$p] = $id
    }
    if (@($ids.Values | Sort-Object -Unique).Count -eq 1) { return }

    # One winner for the whole set, not a pairwise repair: relinking pair by pair would undo the
    # previous pair's work whenever three roots hold three different inodes.
    $src = @($paths | Sort-Object { (Get-Item $_).LastWriteTimeUtc } -Descending)[0]
    $srcId = $ids[$src]
    $relinked = @()
    foreach ($dst in $paths) {
        if ($ids[$dst] -eq $srcId) { continue }
        Copy-Item $dst "$dst.pre-relink" -Force
        Remove-Item $dst -Force
        New-Item -ItemType HardLink -Path $dst -Target $src | Out-Null
        $relinked += (Split-Path -Parent $dst | Split-Path -Leaf)
    }
    if ($relinked.Count -gt 0) {
        Write-Host "  re-linked $Name in $($relinked -join ', ') (kept the newest copy, discarded ones saved as .pre-relink)" -ForegroundColor Yellow
    }
}

function Repair-SharedProfiles {
    # Everything the sharing feature repairs at launch, gathered in one place so Preview can guard
    # ALL of it. deferred review finding: only the New-ClaudeProfileRoot loop was ever wrapped in
    # `if (-not $Preview)` - Repair-SharedLink and Repair-SharedJunction ran unconditionally, so
    # every preview run on a sharing machine deleted and re-hardlinked settings.json/statusline.js
    # and could create a junction with its icacls deny ACEs. Preview must be side-effect-free.
    param([switch]$Preview)
    if (-not $script:LauncherConfig.Sharing) { return }
    if ($Preview) { return }
    # Every account's root exists from the first launch onwards, so the owner can pick it and log in
    # rather than discovering a missing directory mid-launch.
    foreach ($r in $SecondaryRoots) {
        try { $null = New-ClaudeProfileRoot -Root $r } catch { Write-Host "  profile root check failed for ${r}: $($_.Exception.Message)" -ForegroundColor DarkYellow }
    }
    foreach ($f in $SharedFiles) {
        # One call per file, not per pair: Repair-SharedLink compares the File ID of every root at
        # once, because a pairwise check passes while a third copy drifts.
        try { Repair-SharedLink -Name $f } catch { Write-Host "  link check failed for ${f}: $($_.Exception.Message)" -ForegroundColor DarkYellow }
    }
    foreach ($r in $SecondaryRoots) {
        foreach ($d in $SharedDirs) {
            try { Repair-SharedJunction -Name $d -Root $r } catch { Write-Host "  junction check failed for ${d}: $($_.Exception.Message)" -ForegroundColor DarkYellow }
        }
    }
}

function Invoke-LaunchHooks {
    # Config-listed scripts, after the account choice, each in its own try: a hook prints what it
    # wants, a failure is one line, and nothing here can stop a session. Owner tooling lives here.
    param([string[]]$Hooks = @())
    foreach ($h in @($Hooks | Where-Object { $_ })) {
        $leaf = Split-Path $h -Leaf
        if (-not (Test-Path -LiteralPath $h)) { Write-Host "  hook $leaf not found at $h" -ForegroundColor DarkYellow; continue }
        try {
            # Reset first: a hook that runs nothing would otherwise report the PREVIOUS hook's code.
            $global:LASTEXITCODE = 0
            $ext = [IO.Path]::GetExtension($h).ToLowerInvariant()
            $out = switch ($ext) {
                '.ps1' { & pwsh -NoProfile -ExecutionPolicy Bypass -File $h 2>&1 }
                '.js'  { & node $h 2>&1 }
                '.mjs' { & node $h 2>&1 }
                '.cmd' { & cmd /c $h 2>&1 }
                # Never & $h here: an unlisted extension would ShellExecute its desktop association.
                default { Write-Host "  hook ${leaf}: unsupported extension '$ext' (use .ps1, .js, .mjs or .cmd)" -ForegroundColor DarkYellow }
            }
            $code = $LASTEXITCODE
            $text = @($out | ForEach-Object { "$_" } | Where-Object { $_.Trim() })
            if ($code -ne 0) {
                $tail = ($text | Select-Object -Last 2) -join ' | '
                Write-Host "  hook $leaf exit ${code}: $tail" -ForegroundColor DarkYellow
            } else {
                $text | ForEach-Object { Write-Host $_ -ForegroundColor DarkGray }
            }
        } catch { Write-Host "  hook $leaf failed: $($_.Exception.Message -replace '\r?\n', ' | ')" -ForegroundColor DarkYellow }
    }
}

function Protect-SharedJunction {
    # Claude Code's own retention sweep ends in an unconditional `rmdir` on four of these
    # directories with the error swallowed (`U5(e,t){try{await t.rmdir(e)}catch{}}`). On a real
    # directory that is a harmless tidy-up-if-empty; on a JUNCTION it deletes the link and always
    # succeeds, however full the target is - which is why the share kept dying and why re-merging
    # on a cadence loses the race against every running session (measured 2026-08-15).
    #
    # The swallowed catch is also the fix: make the rmdir FAIL and the sweep walks on. Windows
    # authorises a directory delete by DELETE on the object OR FILE_DELETE_CHILD on the parent, so
    # BOTH must be denied - denying either alone was measured and the junction still died. With
    # both denied rmdir throws EPERM while read, mkdir of a new session dir, write and per-file
    # prune THROUGH the link all keep working, so the sweep still reclaims what it should.
    #
    # `/L` is load-bearing: without it icacls follows the link and rewrites the WORK-side ACL.
    #
    # -Root is the profile root that OWNS $Path. It used to be hardcoded to the personal root,
    # which silently stopped protecting anything the moment a third account appeared: the DC deny
    # would have landed on the personal root while the sweep deleted the third profile's
    # junctions, and both ACEs are required - each alone was measured with the junction dying.
    param([string]$Path, [string]$Root = (Split-Path -Parent $Path))
    $who = "$env:USERDOMAIN\$env:USERNAME"
    # The root deny is per ROOT, the junction deny is per LINK - so applying the root one on every
    # junction stacked eleven identical ACEs on a fresh profile root. Same effect, unreadable ACL:
    # check first and add it only when it is genuinely absent.
    $rootCalls = @()
    $existing = & icacls.exe $Root 2>$null
    if (-not (($existing -join ' ') -match [regex]::Escape($who) + '\s*:\s*\(DENY\)\(DC\)')) {
        $rootCalls += @{ What = 'profile root'; Args = @($Root, '/deny', "${who}:(DC)") }
    }
    # icacls is a native exe: it does NOT throw, so try/catch alone would only see a missing
    # binary and a refusal would pass silently - the same swallowed-error shape as the bug this
    # guards against. Every call's exit code is read into its own variable and reported.
    foreach ($call in @($rootCalls + @(
            @{ What = 'junction';     Args = @($Path, '/L', '/deny', "${who}:(DE)") }))) {
        try {
            & icacls.exe @($call.Args) *> $null
            $code = $LASTEXITCODE
        } catch {
            Write-Host "  could not run icacls for the $($call.What) shield on ${Path}: $($_.Exception.Message)" -ForegroundColor DarkYellow
            continue
        }
        if ($code -ne 0) {
            Write-Host "  $($call.What) shield NOT applied for $Path (icacls exit $code) - the retention sweep can delete this junction" -ForegroundColor DarkYellow
        }
    }
}

function Repair-SharedJunction {
    # Junctions survive file edits (unlike hardlinks), so the only failure mode is a
    # missing one - create it. A real directory in its place means the profiles have
    # diverged; merging directories automatically is how data gets lost, so warn only.
    # A missing junction hides a whole directory silently: `skills\` was absent from one secondary
    # root until 2026-07-31 and user skills were invisible under that account.
    param([string]$Name, [Parameter(Mandatory)][string]$Root)
    $w = Join-Path $WorkRoot $Name
    $p = Join-Path $Root $Name
    if (-not (Test-Path $w)) { return }
    # A root that does not exist yet is not a broken share: New-ClaudeProfileRoot creates it, and
    # creating a junction under a missing parent would only raise a warning at every launch.
    if (-not (Test-Path $Root)) { return }
    $rootName = Split-Path -Leaf $Root
    if (Test-Path $p) {
        if (-not ((Get-Item $p -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            Write-Host "  $Name is a real directory in $rootName, not a junction - merge it by hand" -ForegroundColor DarkYellow
        }
        return
    }
    New-Item -ItemType Junction -Path $p -Target $w | Out-Null
    # Shielded at creation only, not on every start: an icacls spawn per directory per launch is
    # the kind of startup cost that had to be removed once already. The recovery is only PARTIAL,
    # so do not read it as self-healing: if the shield is ever lost, the next sweep deletes the
    # junction and the harness recreates it as a REAL directory within minutes (measured: 16:12),
    # so the launcher then finds a diverged directory and warns instead of relinking - back to a
    # hand merge. It self-heals only when nothing writes to the path before the next launch.
    Protect-SharedJunction -Path $p -Root $Root
    Write-Host "  junction created and shielded: $rootName $Name -> work" -ForegroundColor Yellow
}

function New-ClaudeProfileRoot {
    # Brings a profile root into existence so the owner can log into it. Creating the directory is
    # all this does beyond the sharing the launcher already maintains: `.claude.json` holds the
    # account identity and must stay per-profile, so the OAuth login is the owner's own first run
    # under that account - nothing here fabricates credentials.
    #
    # Idempotent and fail-soft: called on every launch for every secondary root, so the normal
    # case must be a Test-Path and nothing else.
    param([Parameter(Mandatory)][string]$Root)
    $created = $false
    if (-not (Test-Path -LiteralPath $Root)) {
        New-Item -ItemType Directory -Force -Path $Root | Out-Null
        $created = $true
    }
    # Hardlink, never a copy: two copies of settings.json is the drift this whole layer prevents,
    # and a copy would read as shared while being a snapshot.
    foreach ($name in $SharedFiles) {
        $w = Join-Path $WorkRoot $name
        $t = Join-Path $Root $name
        if (-not (Test-Path -LiteralPath $w) -or (Test-Path -LiteralPath $t)) { continue }
        New-Item -ItemType HardLink -Path $t -Target $w | Out-Null
        Write-Host "  linked $name into $(Split-Path -Leaf $Root)" -ForegroundColor Yellow
    }
    if ($created) {
        Write-Host "  created profile root $Root - log in there on the first session with that account" -ForegroundColor Yellow
    }
    return $created
}

function Test-McpPort {
    # Only the MCP endpoint answers 200 with text/event-stream; the other Rider ports
    # (built-in server on 63342, etc.) return 404/401 or accept the connection and then
    # never reply. Verified by probing all twelve listening ports of a live instance.
    param([int]$Port)
    try {
        $r = Invoke-WebRequest "http://127.0.0.1:$Port/stream" -TimeoutSec 1 -ErrorAction Stop
        return ($r.StatusCode -eq 200 -and $r.Headers['Content-Type'] -match 'text/event-stream')
    } catch { return $false }
}

function Get-RiderMcpUrl {
    # Scanning serially cost 14s: seven of Rider's ports accept TCP and then stall
    # until the timeout. Hence a cached hit first (the normal case, ~50ms) and a
    # parallel scan only when the cache misses.
    $rider = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match '^rider' })
    if (-not $rider) { return $null }
    $ports = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue |
        Where-Object { $rider.Id -contains $_.OwningProcess -and $_.LocalAddress -eq '127.0.0.1' } |
        Select-Object -ExpandProperty LocalPort | Sort-Object -Unique)
    if (-not $ports) { return $null }

    $cacheFile = Join-Path $env:TEMP 'claude-rider-mcp-port.txt'
    if (Test-Path $cacheFile) {
        $cached = [int](Get-Content $cacheFile -First 1 -ErrorAction SilentlyContinue)
        if ($ports -contains $cached -and (Test-McpPort -Port $cached)) {
            return "http://127.0.0.1:$cached/stream"
        }
    }

    $probe = ${function:Test-McpPort}.ToString()
    $hit = $ports | ForEach-Object -Parallel {
        $t = [scriptblock]::Create($using:probe)
        if (& $t -Port $_) { $_ }
    } -ThrottleLimit 16 | Select-Object -First 1
    if (-not $hit) { return $null }
    Set-Content $cacheFile $hit
    return "http://127.0.0.1:$hit/stream"
}

function Get-McpConfigPaths {
    $mcpConfigs = @()
    try {
        foreach ($extra in @($script:LauncherConfig.ExtraMcpConfigs)) {
            if ($extra -and (Test-Path -LiteralPath $extra)) { $mcpConfigs += $extra }
        }
        # off: never look for the IDE. auto: only when a rider process runs (no line when it does
        # not). on: always scan, and say so when nothing answers.
        $mode = "$($script:LauncherConfig.RiderMcp)"
        $riderRunning = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match '^rider' }).Count -gt 0
        if ($mode -eq 'off' -or ($mode -eq 'auto' -and -not $riderRunning)) { return $mcpConfigs }
        $riderUrl = Get-RiderMcpUrl
        if ($riderUrl) {
            # One file per launch would otherwise accumulate forever; a live session still
            # holds its own, so only sweep yesterday's.
            Get-ChildItem (Join-Path $env:TEMP 'claude-mcp-rider-*.json') -ErrorAction SilentlyContinue |
                Where-Object { $_.LastWriteTime -lt (Get-Date).AddDays(-1) } | Remove-Item -Force -ErrorAction SilentlyContinue
            # Per-launch file keyed by PID: two Rider windows resolve to two ports and
            # must not overwrite each other's config.
            $tmp = Join-Path $env:TEMP "claude-mcp-rider-$PID.json"
            (@{ mcpServers = @{ rider = @{ type = 'http'; url = $riderUrl } } } | ConvertTo-Json -Depth 5) | Set-Content $tmp -Encoding utf8
            $mcpConfigs += $tmp
            Write-Host "  rider MCP: $riderUrl" -ForegroundColor DarkGray
        } else {
            Write-Host "  rider MCP: not found (is the IDE MCP server enabled?)" -ForegroundColor DarkYellow
        }
    } catch { Write-Host "  MCP config skipped: $($_.Exception.Message)" -ForegroundColor DarkYellow }
    return $mcpConfigs
}
function Get-FriendlyModelName {
    # Same family -> friendly-name mapping the model row's own Labels table uses (Screens.ps1),
    # applied to a raw settings.json 'model' string instead of an internal row key. Kept here
    # rather than shared with Screens.ps1 because that file must stay free of file I/O and this
    # one is already the file-reading layer - duplicating four short mappings costs less than
    # threading a lookup table across files for something this small.
    #
    # Matches case-insensitively on the family name appearing ANYWHERE in the raw value, so both
    # the alias form ('fable') and the full id form ('claude-fable-5-1[1m]') resolve the same way.
    # A raw value with no recognised family renders as itself, truncated - an unknown model id
    # must never blow up the row width the way the untranslated raw id used to (30 chars for
    # 'claude-fable-5-1[1m]' alone, before the surrounding 'default (...)').
    #
    # The friendly name is the CURRENT release of each family - `fable` is what `/model` writes
    # and what the CLI resolves to the latest Fable, so the label follows the CLI, not the id.
    # Bump the label here (and the Labels table in Screens.ps1) when a new release lands.
    param([string]$Raw)
    if (-not $Raw) { return $Raw }
    $families = [ordered]@{
        fable  = 'Fable 5.1'
        opus   = 'Opus 5'
        sonnet = 'Sonnet 5'
        haiku  = 'Haiku 4.5'
    }
    $friendly = $null
    foreach ($key in $families.Keys) {
        if ($Raw -imatch [regex]::Escape($key)) { $friendly = $families[$key]; break }
    }
    if (-not $friendly) {
        if ($Raw.Length -gt 24) { return $Raw.Substring(0, 23) + [string][char]0x2026 }
        return $Raw
    }
    if ($Raw -imatch '\[1m\]') { $friendly += '[1M]' }
    return $friendly
}

function Get-DefaultModelLabel {
    # What "default" on the model row actually means right now. Resolved at render time from
    # settings.json rather than baked into the row definition, because it changes whenever the
    # owner edits that file directly and a stale label would be a menu that lies. This is a menu,
    # not a validator: any failure - missing file, unparseable JSON, no 'model' key - degrades to
    # a plain label rather than throwing and taking the launch screen down with it.
    #
    # Rendered through Get-FriendlyModelName so the raw --model id (e.g. 'claude-fable-5-1[1m]',
    # 30 chars) never sits in the row unshortened - at width 100 the box is capped at 100 columns
    # (Screens.ps1 boxWidth) with 96 usable, and the raw id alone left no room for the other four
    # options. 'Fable 5.1[1M]' (13 chars) does.
    param([string]$Path = (Join-Path $WorkRoot 'settings.json'))
    try {
        if (-not (Test-Path -LiteralPath $Path)) { return 'default' }
        $json = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
        if ($json.model) { return "default ($(Get-FriendlyModelName -Raw $json.model))" }
        return 'default (account default)'
    } catch { return 'default' }
}

function Get-DefaultAdvisorLabel {
    # What "default" on the advisor row means right now - the same job Get-DefaultModelLabel does for
    # the model row, and the same failure policy: this is a menu, not a validator, so a missing file,
    # unparseable JSON or an absent key degrades to a plain label instead of throwing.
    #
    # The raw value, not a friendly name: the advisor row's other options are the bare CLI words
    # ('fable', 'opus'), so 'default (fable)' reads as "the same thing the row below offers", which
    # is the question the label answers. 'default (none)' is deliberately different from 'default':
    # the first says the file was read and holds no advisorModel, the second says it could not be
    # read at all, and those must not look alike on a screen that decides what a launch spends.
    param([string]$Path = (Join-Path $WorkRoot 'settings.json'))
    try {
        if (-not (Test-Path -LiteralPath $Path)) { return 'default' }
        $json = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
        if ($json.advisorModel) { return "default ($($json.advisorModel))" }
        return 'default (none)'
    } catch { return 'default' }
}

function Get-FreshestRateLimitRecord {
    # Two writers, one profile: statusline.js writes `<profile>.json` while a session runs, and
    # claude-usage-widget writes `<profile>.widget.json` every five minutes whether or not one does.
    # Neither is always the fresher, so pick by `atMs` rather than by which file we like - a running
    # session beats the widget's five-minute-old poll, and the widget beats a profile nobody has
    # opened for three days. Separate files on purpose: one file with two writers is last-writer-wins,
    # and the loser there would be the fresher number.
    param([string]$Directory, [string]$ProfileName)

    $best = $null
    foreach ($suffix in @('.json', '.widget.json')) {
        $path = Join-Path $Directory "$ProfileName$suffix"
        if (-not (Test-Path -LiteralPath $path)) { continue }
        try {
            $candidate = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
            if ($null -eq $candidate.atMs) { continue }
            if ($null -eq $best -or [long]$candidate.atMs -gt [long]$best.atMs) { $best = $candidate }
        } catch { }   # unreadable or malformed: the other file may still be good
    }
    return $best
}

function Get-RateLimitSummary {
    # Last-known limits from whichever source saw them last - see Get-FreshestRateLimitRecord.
    # `claude usage` is not a command, transcripts carry no rate-limit records, and stats-cache.json
    # holds token counters rather than limits (and is computed from the shared transcript store, so
    # it cannot tell the accounts apart anyway).
    #
    # The age is therefore always rendered. A profile whose newest record is three days old shows a
    # three-day-old number, and it must say so rather than read as current.
    #
    # -Directory is a parameter so the suite can run against a throwaway store: the default reads the
    # live one, and a check that cannot be run casually stops being run.
    param([string]$Directory = (Join-Path $HOME '.claude\rate-limits'))
    $out = @{}
    foreach ($profileName in @($ProfileRoots.Keys)) {
        $j = Get-FreshestRateLimitRecord -Directory $Directory -ProfileName $profileName
        if ($null -eq $j) { continue }
        try {
            # Epoch milliseconds, not an ISO string: ConvertFrom-Json silently turns an ISO string
            # into a [datetime] and drops the Z, after which re-parsing reads UTC as local and the
            # age comes out wrong by exactly the timezone offset. Measured that once, not twice.
            $age = [datetimeoffset]::Now - [datetimeoffset]::FromUnixTimeMilliseconds([long]$j.atMs)
            $ageText =
                if ($age.TotalSeconds -lt 90) { 'just now' }
                elseif ($age.TotalMinutes -lt 60) { '{0:N0} min ago' -f $age.TotalMinutes }
                elseif ($age.TotalHours -lt 24) { '{0:N0} h ago' -f $age.TotalHours }
                else { '{0:N0} d ago' -f $age.TotalDays }

            # Numbers, not a formatted string: the launch screen draws bars from the percentages and
            # cannot recover them from '5h 19% 7d 98%'. The age still travels with them - a stale
            # number that reads as current is the failure this field exists to prevent.
            #
            # Model/ModelLabel are the per-model weekly bucket (claude-usage-widget's payload v2:
            # modelSevenDay + modelLabel). They come from the SAME record as the two percentages,
            # never from whichever file happens to carry them: a bucket read out of the older of the
            # two files would put a third number of a third age on one line, which is the exact
            # staleness this whole function exists to make visible. statusline.js writes no model
            # fields at all, so when its record is the fresher one both are $null and the screen
            # draws no third bar - correct, not a gap.
            if ($null -ne $j.fiveHour -or $null -ne $j.sevenDay) {
                $out[$profileName] = [pscustomobject]@{
                    FiveHour   = if ($null -ne $j.fiveHour) { [int]$j.fiveHour } else { $null }
                    SevenDay   = if ($null -ne $j.sevenDay) { [int]$j.sevenDay } else { $null }
                    AgeText    = $ageText
                    Model      = if ($null -ne $j.modelSevenDay) { [int]$j.modelSevenDay } else { $null }
                    ModelLabel = if ($j.modelLabel) { [string]$j.modelLabel } else { $null }
                }
            }
        } catch { }   # unreadable or malformed: show nothing rather than a wrong number
    }
    return $out
}

# `claude --help` Commands (2.1.239, read 2026-08-22) plus the background-session CLI that
# remote-control-claude-code adds (`attach`, `stop`) and the gate's own list. A subcommand is an
# administrative call, not a session: it rejects session flags, `--mcp-config` first among them.
$ClaudeSubcommands = @(
    'agents', 'auth', 'auto-mode', 'doctor', 'gateway', 'import', 'install', 'mcp', 'plugin', 'plugins',
    'project', 'setup-token', 'ultrareview', 'update', 'upgrade',
    'attach', 'stop', 'config', 'login', 'logout', 'migrate-installer'
)

function Test-ClaudeSubcommand {
    param([AllowNull()][AllowEmptyString()][string]$Token)
    if (-not $Token) { return $false }
    return ($Token -in $ClaudeSubcommands)
}

function Get-ClaudeInvocation {
    # The single source of truth for what gets handed to `claude`. Both the preview print and the
    # real exec go through this, because those two diverged once and every automated check then
    # verified a string that nothing executed.
    #
    # -McpConfigs is forced into array context here as well as at the call site: a scalar string
    # splatted with @ enumerates character by character, and `.Count` on a string is 1 so a
    # `-gt 0` guard cannot catch it. --mcp-config goes LAST because it is variadic - anything
    # after it is swallowed as another config path.
    #
    # A SUBCOMMAND gets no MCP flags at all (2026-08-22): `claude-auto auto-mode
    # defaults` ran the whole launch preamble and then died with `error: unknown option
    # '--mcp-config'`, and the gate's block message recommends exactly that form. Only the first
    # token decides — a positional prompt (`claude-auto "fix the test"`) is a session and keeps them.
    param(
        [object[]]$LaunchArgs = @(),
        [object[]]$McpConfigs = @()
    )
    $out = @($LaunchArgs)
    if ($out.Count -gt 0 -and (Test-ClaudeSubcommand ([string]$out[0]))) { return ,$out }
    $configs = @($McpConfigs | Where-Object { $_ })
    if ($configs.Count -gt 0) { $out += '--mcp-config'; $out += $configs }
    return ,$out
}
