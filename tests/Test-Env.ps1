# Import-ProjectSecrets: the tier cascade shared -> org -> project.
#
# Why this suite exists: the loader is the single consumer of the credential store, so every future
# session inherits whatever it does, and until 2026-08-13 none of the six claude-auto suites touched
# it. That gap hid a real defect for weeks - the `Shared` tier was never read at all, so a secret
# placed there was silently dead while looking correctly installed.
#
# Runs entirely against a throwaway store under $env:TEMP. It must never read, write or even stat
# the real secrets store: a test that touches it cannot be run casually, and a check that cannot be
# run casually stops being run.
#
# Run bare and read $LASTEXITCODE. A pipeline reports the LAST command's status.

# The roster is derived at load, so the fixture must be named BEFORE the dot-source.
$env:CLAUDE_AUTO_CONFIG = "$PSScriptRoot\fixtures\config-four.json"
try { . (Join-Path $PSScriptRoot '..\claude-auto\Env.ps1') } catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }

# The body runs inside try/finally so a terminating error mid-suite cannot leak the fixture path.
try {

$script:fail = 0
$script:Ran = 0
function Assert([string]$Name, [bool]$Condition) {
    $script:Ran++
    if ($Condition) { Write-Host "  ok   $Name" }
    else { Write-Host "  FAIL $Name" -ForegroundColor Red; $script:fail++ }
}

$root = Join-Path $env:TEMP ("cct-secrets-test-" + [guid]::NewGuid().ToString('N'))
$slug = 'C--test--proj'
New-Item -ItemType Directory -Force "$root\shared", "$root\org\acme", "$root\$slug" | Out-Null

Set-Content -LiteralPath "$root\shared\SHARED_ONLY"    -Value 'from-shared' -NoNewline
Set-Content -LiteralPath "$root\shared\OVERRIDDEN"     -Value 'from-shared' -NoNewline
Set-Content -LiteralPath "$root\shared\ORG_BEATS_THIS" -Value 'from-shared' -NoNewline
Set-Content -LiteralPath "$root\org\acme\ORG_ONLY"       -Value 'from-org' -NoNewline
Set-Content -LiteralPath "$root\org\acme\OVERRIDDEN"     -Value 'from-org' -NoNewline
Set-Content -LiteralPath "$root\org\acme\ORG_BEATS_THIS" -Value 'from-org' -NoNewline
Set-Content -LiteralPath "$root\$slug\PROJ_ONLY"  -Value "from-proj`r`n"
Set-Content -LiteralPath "$root\$slug\OVERRIDDEN" -Value 'from-proj' -NoNewline
Set-Content -LiteralPath "$root\$slug\EMPTY"      -Value '' -NoNewline
# An index file inside a tier would otherwise become $env:NOTES.md - the loader must skip it.
Set-Content -LiteralPath "$root\$slug\NOTES.md"   -Value 'not a secret' -NoNewline
New-Item -ItemType Junction -Path "$root\$slug\org" -Target "$root\org\acme" | Out-Null

$vars = 'SHARED_ONLY', 'ORG_ONLY', 'PROJ_ONLY', 'OVERRIDDEN', 'ORG_BEATS_THIS', 'EMPTY', 'NOTES.md', 'SONAR_TOKEN_FILE'
foreach ($v in $vars) { Remove-Item -LiteralPath "Env:$v" -ErrorAction SilentlyContinue }

$loaded = @(Import-ProjectSecrets -WorkingDirectory 'C:\test\.proj' -Root $root)

Assert 'shared tier is loaded'                ($env:SHARED_ONLY -eq 'from-shared')
Assert 'org tier is loaded through the junction' ($env:ORG_ONLY -eq 'from-org')
Assert 'project tier is loaded'               ($env:PROJ_ONLY -eq 'from-proj')
Assert 'project beats org and shared'         ($env:OVERRIDDEN -eq 'from-proj')
Assert 'org beats shared'                     ($env:ORG_BEATS_THIS -eq 'from-org')
Assert 'trailing newline is trimmed'          ($env:PROJ_ONLY -notmatch '\s$')
Assert 'markdown is not loaded as a variable' (-not (Test-Path 'Env:NOTES.md'))
Assert 'an empty file sets nothing'           (-not (Test-Path 'Env:EMPTY'))
Assert 'the return value names each tier'     (($loaded -join ',') -match 'shared' -and ($loaded -join ',') -match 'org' -and ($loaded -join ',') -match 'project')
Assert 'no SONARQUBE_TOKEN means no SONAR_TOKEN_FILE' (-not $env:SONAR_TOKEN_FILE)

# -Root's default must track the LIVE config, not Get-LauncherDefaults - a machine that set a
# non-default secretsRoot must have it picked up without every call site passing -Root by hand.
$origLauncherConfig = $LauncherConfig
$defaultRootTest = Join-Path $env:TEMP ("cct-defaultroot-test-" + [guid]::NewGuid().ToString('N'))
try {
    New-Item -ItemType Directory -Force (Join-Path $defaultRootTest 'shared') | Out-Null
    Set-Content -LiteralPath (Join-Path $defaultRootTest 'shared\NAME_X') -Value 'v1' -NoNewline
    $LauncherConfig = [pscustomobject]@{ SecretsRoot = $defaultRootTest }
    Remove-Item Env:NAME_X -ErrorAction SilentlyContinue
    $null = Import-ProjectSecrets -WorkingDirectory 'C:\x'
    Assert '-Root default reads the live config, not Get-LauncherDefaults' ($env:NAME_X -eq 'v1')
} finally {
    Remove-Item Env:NAME_X -ErrorAction SilentlyContinue
    $LauncherConfig = $origLauncherConfig
    Remove-Item -LiteralPath $defaultRootTest -Recurse -Force -ErrorAction SilentlyContinue
}

# SONAR_TOKEN_FILE resolves to the winning tier's file, not the first one found.
Set-Content -LiteralPath "$root\shared\SONARQUBE_TOKEN" -Value 'shared-token' -NoNewline
Set-Content -LiteralPath "$root\$slug\SONARQUBE_TOKEN"  -Value 'proj-token' -NoNewline
$null = Import-ProjectSecrets -WorkingDirectory 'C:\test\.proj' -Root $root
Assert 'SONAR_TOKEN_FILE points at the winning tier' ($env:SONAR_TOKEN_FILE -eq (Join-Path "$root\$slug" 'SONARQUBE_TOKEN'))

# A project with no directory of its own still gets the shared tier, and does not throw.
foreach ($v in $vars) { Remove-Item -LiteralPath "Env:$v" -ErrorAction SilentlyContinue }
$none = @(Import-ProjectSecrets -WorkingDirectory 'C:\test\unknown' -Root $root)
Assert 'an unknown project still gets shared' ($env:SHARED_ONLY -eq 'from-shared')
Assert 'an unknown project gets no project tier' (-not (Test-Path 'Env:PROJ_ONLY'))
Assert 'an unknown project returns only shared names' ((($none -join ',') -notmatch 'project') -and $none.Count -gt 0)

# A missing root is not an error - a machine with no store must still launch.
$missing = @(Import-ProjectSecrets -WorkingDirectory 'C:\test\.proj' -Root (Join-Path $root 'does-not-exist'))
Assert 'a missing root returns empty and does not throw' ($missing.Count -eq 0)

foreach ($v in $vars) { Remove-Item -LiteralPath "Env:$v" -ErrorAction SilentlyContinue }
Remove-Item -LiteralPath $root -Recurse -Force

# --- launch log -------------------------------------------------------------------------------
# Also against a throwaway root: the real log under ~/.claude/launcher-logs is evidence about how
# this machine actually launches Claude, and a suite that appends test records to it would poison
# exactly the file someone reads while debugging a launch.

$logRoot = Join-Path $env:TEMP ("cct-launchlog-test-" + [guid]::NewGuid().ToString('N'))

$wrote = Write-LauncherLog -Stage 'start' -RunId 'testrun0001' -Root $logRoot -Data @{ cwd = 'C:\x'; args = @('-p', 'hi') }
Assert 'a log write reports success'      ($wrote -eq $true)
Assert 'the log directory is created'     (Test-Path -LiteralPath $logRoot)

$logFile = Join-Path $logRoot ('claude-auto-{0:yyyy-MM-dd}.jsonl' -f (Get-Date))
Assert 'the file is named for today'      (Test-Path -LiteralPath $logFile)

$null = Write-LauncherLog -Stage 'exit' -RunId 'testrun0001' -Root $logRoot -Data @{ code = 0 }
$lines = @(Get-Content -LiteralPath $logFile)
Assert 'a second record appends'          ($lines.Count -eq 2)

$first = $lines[0] | ConvertFrom-Json
Assert 'the record carries stage and run' ($first.stage -eq 'start' -and $first.run -eq 'testrun0001')
Assert 'the record carries this pid'      ($first.pid -eq $PID)
Assert 'the timestamp round-trips'        ([datetimeoffset]::Parse($first.ts).Year -eq (Get-Date).Year)
Assert 'caller data is merged in'         ($first.cwd -eq 'C:\x' -and ($first.args -join ' ') -eq '-p hi')

# One record per LINE is what makes the file greppable, and ConvertTo-Json without -Compress would
# silently pretty-print a multi-line object into it.
Assert 'each record is a single line'     (@($lines | Where-Object { $_ -match '^\{.*\}$' }).Count -eq 2)

# Silence is load-bearing: check-launcher-regression.ps1 diffs the launcher's console output, so a
# single Write-Host in the logger would redden a check that guards real behaviour.
$noise = @(Write-LauncherLog -Stage 'quiet' -RunId 'testrun0002' -Root $logRoot 6>&1 | Where-Object { $_ -isnot [bool] })
Assert 'the logger writes nothing to the console' ($noise.Count -eq 0)

# An unwritable root must cost a log line, never a launch - and must not shout about it either.
# The NUL is built in code on purpose: a path this broken cannot survive being typed through a
# shell, and Test-Path/New-Item report it as a NON-terminating error that a bare try/catch misses.
$blockedAll = @(Write-LauncherLog -Stage 'start' -RunId 'testrun0003' -Root ("$logRoot" + [string][char]0) 2>&1 6>&1)
$blocked = @($blockedAll | Where-Object { $_ -is [bool] })
$blockedNoise = @($blockedAll | Where-Object { $_ -isnot [bool] })
Assert 'an unusable root fails open'      ($blocked.Count -eq 1 -and $blocked[0] -eq $false)
Assert 'an unusable root stays silent'    ($blockedNoise.Count -eq 0)

$chain = @(Get-LauncherParentChain)
Assert 'the parent chain starts at this process' ($chain.Count -ge 1 -and $chain[0].pid -eq $PID)
Assert 'the chain records a parent too'   ($chain.Count -ge 2 -and $chain[1].pid -ne $PID)

# Retention: old files go, today's stays.
$old = Join-Path $logRoot 'claude-auto-2000-01-01.jsonl'
Set-Content -LiteralPath $old -Value '{}' -NoNewline
(Get-Item -LiteralPath $old).LastWriteTime = (Get-Date).AddDays(-30)
Remove-OldLauncherLogs -Root $logRoot -Days 14
Assert 'an old log is pruned'             (-not (Test-Path -LiteralPath $old))
Assert "today's log survives the prune"   (Test-Path -LiteralPath $logFile)

Remove-Item -LiteralPath $logRoot -Recurse -Force

# --- profile roots and the shared-file links ---------------------------------------------------
# Three accounts since 2026-08-20. This section exists because of a defect the two-account version
# could not see: Repair-SharedLink used to accept `fsutil hardlink list <work> | count >= 2` as
# proof of sharing, which stays true while a THIRD copy drifts away unnoticed.
#
# Fake roots under $env:TEMP, injected by reassigning the module's own variables - Env.ps1 is
# dot-sourced into this scope, so its functions resolve $WorkRoot/$SecondaryRoots/$SharedFiles from
# here at call time. The real ~/.claude is never touched.
$origWorkRoot = $WorkRoot; $origSecondary = $SecondaryRoots; $origSharedFiles = $SharedFiles
$fake = Join-Path $env:TEMP ("cct-profiles-test-" + [guid]::NewGuid().ToString('N'))
try {
    $WorkRoot = Join-Path $fake 'work'
    $rootA = Join-Path $fake 'personal'
    $rootB = Join-Path $fake 'shared'
    $SecondaryRoots = @($rootA, $rootB)
    $SharedFiles = @('settings.json')
    New-Item -ItemType Directory -Force $WorkRoot | Out-Null
    $workFile = Join-Path $WorkRoot 'settings.json'
    Set-Content -LiteralPath $workFile -Value '{"model":"work"}' -NoNewline

    Assert 'the roster names four accounts'         (@($ProfileRoots.Keys).Count -eq 4)
    # $origWorkRoot, not $WorkRoot: this block has already swapped $WorkRoot for the fake root above.
    Assert 'the canonical account is work'          ($CanonicalAccount -eq 'work' -and $origWorkRoot -eq (Join-Path $HOME '.claude'))
    Assert 'shared maps to its configured root'     ($ProfileRoots['shared'] -eq (Join-Path $HOME '.claude-shared'))
    Assert 'the config object is exposed'           ($LauncherConfig.Sharing -eq $true)
    # Every account in the roster except 'work' must also be a root the launcher SHARES into -
    # $ProfileRoots and $SecondaryRoots are two lists that have to agree, and the failure of them
    # disagreeing is silent: the account launches and then drifts on its own settings.json copy.
    Assert 'every non-work account is a shared root' (@($ProfileRoots.Keys | Where-Object { $_ -ne 'work' } | Where-Object { $origSecondary -notcontains $ProfileRoots[$_] }).Count -eq 0)
    # The launch line's label and tint come from tables, and a key missing from either prints an
    # empty label with a $null colour - which reads as a successful launch of an unnamed account.
    $nonWork = @($ProfileRoots.Keys | Where-Object { $_ -ne 'work' })
    Assert 'every account has a launch label'       (@($nonWork | Where-Object { -not $ProfileLabels[$_] }).Count -eq 0)
    Assert 'every account has its own tint'         ((@($nonWork | ForEach-Object { $ProfileTints[$_] } | Where-Object { $_ } | Sort-Object -Unique).Count) -eq $nonWork.Count)

    $createdA = New-ClaudeProfileRoot -Root $rootA 6>$null
    $createdB = New-ClaudeProfileRoot -Root $rootB 6>$null
    Assert 'a new profile root is created'          ($createdA -and $createdB -and (Test-Path $rootB))
    Assert 'creating it again is a no-op'           ((New-ClaudeProfileRoot -Root $rootB 6>$null) -eq $false)
    $ids = @(@($WorkRoot, $rootA, $rootB) | ForEach-Object { Get-SharedFileId -Path (Join-Path $_ 'settings.json') } | Sort-Object -Unique)
    Assert 'the shared file is one inode in all three' ($ids.Count -eq 1 -and $ids[0])

    # Set-ClaudeProfile under preview. Setting CLAUDE_CONFIG_DIR is this process's own environment
    # and may happen; running the MCP mirror may not - it rewrites the target account's .claude.json
    # wholesale, and a preview run did exactly that to a real profile. The account root is pointed
    # at the fake tree first, so that even a build with the guard removed cannot reach a real one.
    $savedPersonalRoot = $ProfileRoots['personal']
    $savedConfigDir = $env:CLAUDE_CONFIG_DIR
    try {
        $ProfileRoots['personal'] = $rootA
        # Counted through the injected seam, not read off the output: a mirror run that finds nothing
        # to copy prints NOTHING, so "no line mentioning mirror" is satisfied by a mirror that really
        # ran. Written that way first, and the mutation that removes the guard survived it.
        $script:MirrorRuns = 0
        $recordingMirror = { param($Root) $script:MirrorRuns++; '' }
        $previewOut = @(Set-ClaudeProfile -Account 'personal' -Preview -Mirror $recordingMirror 6>&1 | ForEach-Object { "$_" })
        Assert 'preview never runs the project MCP mirror'   ($script:MirrorRuns -eq 0)
        Assert 'preview still announces the account'         (@($previewOut | Where-Object { $_ -match '->' }).Count -eq 1)
        $null = Set-ClaudeProfile -Account 'personal' -Mirror $recordingMirror 6>&1
        Assert 'and without preview it does run, once'       ($script:MirrorRuns -eq 1)
    } finally {
        $ProfileRoots['personal'] = $savedPersonalRoot
        if ($null -eq $savedConfigDir) { Remove-Item Env:CLAUDE_CONFIG_DIR -ErrorAction SilentlyContinue }
        else { $env:CLAUDE_CONFIG_DIR = $savedConfigDir }
    }

    # THE REGRESSION: work+personal stay linked while the third copy is replaced by an atomic
    # write. The old hardlink-count check saw 2 names and returned "shared".
    $sharedFile = Join-Path $rootB 'settings.json'
    Remove-Item -LiteralPath $sharedFile -Force
    Set-Content -LiteralPath $sharedFile -Value '{"model":"drifted"}' -NoNewline
    (Get-Item -LiteralPath $sharedFile).LastWriteTimeUtc = (Get-Date).ToUniversalTime().AddHours(-2)
    Assert 'the pairwise count still reads as shared' (@(fsutil hardlink list $workFile 2>$null).Count -ge 2)

    Repair-SharedLink -Name 'settings.json' 6>$null
    $ids = @(@($WorkRoot, $rootA, $rootB) | ForEach-Object { Get-SharedFileId -Path (Join-Path $_ 'settings.json') } | Sort-Object -Unique)
    Assert 'a drifted third copy is re-linked'      ($ids.Count -eq 1)
    Assert 'the newest copy won'                    ((Get-Content -LiteralPath $sharedFile -Raw) -eq '{"model":"work"}')
    Assert 'the discarded copy is kept beside it'   (Test-Path -LiteralPath "$sharedFile.pre-relink")

    # And the other direction: when the divergent copy is the NEWEST, its content wins everywhere -
    # one winner for the whole set, not a pairwise repair that undoes itself.
    Remove-Item -LiteralPath $sharedFile -Force
    Set-Content -LiteralPath $sharedFile -Value '{"model":"newest"}' -NoNewline
    (Get-Item -LiteralPath $sharedFile).LastWriteTimeUtc = (Get-Date).ToUniversalTime().AddHours(2)
    Repair-SharedLink -Name 'settings.json' 6>$null
    $ids = @(@($WorkRoot, $rootA, $rootB) | ForEach-Object { Get-SharedFileId -Path (Join-Path $_ 'settings.json') } | Sort-Object -Unique)
    Assert 'the newest copy wins across every root' ($ids.Count -eq 1 -and (Get-Content -LiteralPath $workFile -Raw) -eq '{"model":"newest"}')

    # A root that does not exist yet is not a broken share: it must be skipped silently, or every
    # launch would warn about an account nobody has created.
    $SecondaryRoots = @($rootA, (Join-Path $fake 'never-created'))
    $noise = @(Repair-SharedLink -Name 'settings.json' 6>&1)
    Assert 'a missing root is skipped silently'     ($noise.Count -eq 0)
    $junctionNoise = @(Repair-SharedJunction -Name 'projects' -Root (Join-Path $fake 'never-created') 6>&1)
    Assert 'a junction under a missing root is skipped' ($junctionNoise.Count -eq 0)
} finally {
    $WorkRoot = $origWorkRoot; $SecondaryRoots = $origSecondary; $SharedFiles = $origSharedFiles
    if (Test-Path -LiteralPath $fake) { Remove-Item -LiteralPath $fake -Recurse -Force }
}

# --- Repair-SharedProfiles: Preview must be side-effect-free -----------------------------------
# deferred review finding: only the profile-root-creation loop was ever guarded by
# `if (-not $Preview)` in claude-auto.ps1 - the link and junction repairs ran unconditionally, so
# every preview run on a sharing machine mutated real files (settings.json re-hardlinked) and could
# create a junction with its icacls deny ACEs. Fake roots again; the real ~/.claude is never touched.
$origWorkRoot2 = $WorkRoot; $origSecondary2 = $SecondaryRoots; $origSharedFiles2 = $SharedFiles; $origSharedDirs2 = $SharedDirs
$fake2 = Join-Path $env:TEMP ("cct-preview-guard-test-" + [guid]::NewGuid().ToString('N'))
try {
    $WorkRoot = Join-Path $fake2 'work'
    $rootP = Join-Path $fake2 'personal'
    $SecondaryRoots = @($rootP)
    $SharedFiles = @('settings.json')
    $SharedDirs = @('projects')
    New-Item -ItemType Directory -Force $WorkRoot, $rootP | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $WorkRoot 'projects') | Out-Null
    Set-Content -LiteralPath (Join-Path $WorkRoot 'settings.json') -Value '{"model":"work"}' -NoNewline
    # A drifted, unlinked copy in the secondary root - a real repair would relink it to $WorkRoot's.
    Set-Content -LiteralPath (Join-Path $rootP 'settings.json') -Value '{"model":"drifted"}' -NoNewline
    (Get-Item -LiteralPath (Join-Path $rootP 'settings.json')).LastWriteTimeUtc = (Get-Date).ToUniversalTime().AddHours(-2)

    $beforeContent = Get-Content -LiteralPath (Join-Path $rootP 'settings.json') -Raw
    $beforeTime = (Get-Item -LiteralPath (Join-Path $rootP 'settings.json')).LastWriteTimeUtc

    Repair-SharedProfiles -Preview 6>$null

    Assert 'preview: the divergent copy is left untouched (content)' ((Get-Content -LiteralPath (Join-Path $rootP 'settings.json') -Raw) -eq $beforeContent)
    Assert 'preview: the divergent copy is left untouched (write time)' ((Get-Item -LiteralPath (Join-Path $rootP 'settings.json')).LastWriteTimeUtc -eq $beforeTime)
    Assert 'preview: no junction is created' (-not (Test-Path -LiteralPath (Join-Path $rootP 'projects')))
    Assert 'preview: no .pre-relink is left behind either' (-not (Test-Path -LiteralPath (Join-Path $rootP 'settings.json.pre-relink')))

    # Off preview, the same setup DOES get repaired - proves the guard is the ONLY thing that changed,
    # not that Repair-SharedProfiles silently does nothing.
    Repair-SharedProfiles 6>$null
    Assert 'not preview: the drifted copy is relinked to the newest'     ((Get-Content -LiteralPath (Join-Path $rootP 'settings.json') -Raw) -eq '{"model":"work"}')
    Assert 'not preview: the missing junction is created'                (Test-Path -LiteralPath (Join-Path $rootP 'projects'))
} finally {
    $WorkRoot = $origWorkRoot2; $SecondaryRoots = $origSecondary2; $SharedFiles = $origSharedFiles2; $SharedDirs = $origSharedDirs2
    if (Test-Path -LiteralPath $fake2) { Remove-Item -LiteralPath $fake2 -Recurse -Force -ErrorAction SilentlyContinue }
}

# Get-ClaudeInvocation: a SUBCOMMAND must not get --mcp-config (2026-08-22).
# `claude auto-mode defaults` died with `error: unknown option '--mcp-config'` after the full launch
# preamble, because the flags were appended to every pass-through. A positional PROMPT is not a
# subcommand and keeps them; an empty launch keeps them; --version keeps them (it ignores extras).
$cfg = @('C:\x\mcp-shared.json')
Assert 'a subcommand gets no --mcp-config'         ((Get-ClaudeInvocation -LaunchArgs @('auto-mode', 'defaults') -McpConfigs $cfg) -notcontains '--mcp-config')
Assert 'mcp list gets no --mcp-config'             ((Get-ClaudeInvocation -LaunchArgs @('mcp', 'list') -McpConfigs $cfg) -notcontains '--mcp-config')
Assert 'plugin (alias plugins) gets no --mcp-config' ((Get-ClaudeInvocation -LaunchArgs @('plugins', 'list') -McpConfigs $cfg) -notcontains '--mcp-config')
Assert 'the subcommand args survive untouched'     (((Get-ClaudeInvocation -LaunchArgs @('auto-mode', 'defaults') -McpConfigs $cfg) -join ' ') -eq 'auto-mode defaults')
Assert 'an empty launch keeps --mcp-config'        ((Get-ClaudeInvocation -LaunchArgs @() -McpConfigs $cfg) -contains '--mcp-config')
Assert 'a positional prompt keeps --mcp-config'    ((Get-ClaudeInvocation -LaunchArgs @('fix the failing test') -McpConfigs $cfg) -contains '--mcp-config')
Assert 'a flag launch keeps --mcp-config'          ((Get-ClaudeInvocation -LaunchArgs @('--resume', 'abc') -McpConfigs $cfg) -contains '--mcp-config')
Assert 'Test-ClaudeSubcommand knows the help list' ((Test-ClaudeSubcommand 'doctor') -and (Test-ClaudeSubcommand 'upgrade') -and -not (Test-ClaudeSubcommand 'doctor-notes') -and -not (Test-ClaudeSubcommand '--version'))

# Get-FreshestRateLimitRecord: two writers per profile, newest atMs wins.
#
# The helper, not Get-RateLimitSummary itself: that one reads $HOME\.claude\rate-limits and the real
# $ProfileRoots, and this suite must never touch the live store.
$rl = Join-Path $env:TEMP ("cct-ratelimit-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $rl | Out-Null
try {
    Set-Content -LiteralPath "$rl\a.json"        -Value '{"fiveHour":10,"sevenDay":20,"atMs":1000}'
    Set-Content -LiteralPath "$rl\a.widget.json" -Value '{"fiveHour":11,"sevenDay":21,"atMs":2000}'
    Assert 'the widget file wins when it is fresher'        ((Get-FreshestRateLimitRecord -Directory $rl -ProfileName 'a').fiveHour -eq 11)

    Set-Content -LiteralPath "$rl\b.json"        -Value '{"fiveHour":30,"atMs":9000}'
    Set-Content -LiteralPath "$rl\b.widget.json" -Value '{"fiveHour":31,"atMs":8000}'
    Assert 'a live session wins over an older widget poll'  ((Get-FreshestRateLimitRecord -Directory $rl -ProfileName 'b').fiveHour -eq 30)

    Set-Content -LiteralPath "$rl\c.json" -Value '{"fiveHour":40,"atMs":5000}'
    Assert 'one file alone is used'                         ((Get-FreshestRateLimitRecord -Directory $rl -ProfileName 'c').fiveHour -eq 40)

    Assert 'no file at all is null, not an error'           ($null -eq (Get-FreshestRateLimitRecord -Directory $rl -ProfileName 'nothing'))

    # A half-written widget file must not hide a perfectly good session file.
    Set-Content -LiteralPath "$rl\d.json"        -Value '{"fiveHour":50,"atMs":5000}'
    Set-Content -LiteralPath "$rl\d.widget.json" -Value '{"fiveHour":51,'
    Assert 'a malformed file does not shadow a good one'    ((Get-FreshestRateLimitRecord -Directory $rl -ProfileName 'd').fiveHour -eq 50)

    # No atMs means nothing can be compared - the record is not a candidate at all.
    Set-Content -LiteralPath "$rl\e.json"        -Value '{"fiveHour":60}'
    Set-Content -LiteralPath "$rl\e.widget.json" -Value '{"fiveHour":61,"atMs":1}'
    Assert 'a record without atMs is ignored'               ((Get-FreshestRateLimitRecord -Directory $rl -ProfileName 'e').fiveHour -eq 61)
} finally {
    if (Test-Path -LiteralPath $rl) { Remove-Item -LiteralPath $rl -Recurse -Force -ErrorAction SilentlyContinue }
}

# Get-RateLimitSummary: the model bucket travels with the two account limits, or does not exist.
#
# -Directory is a parameter so this can run against a throwaway store; without it the function
# reads $HOME\.claude\rate-limits and a suite that touches the live store stops being run casually.
# The model fields come from the SAME record the percentages come from (Get-FreshestRateLimitRecord):
# a bucket read from the older of the two files would be a third number of a third age on one line.
$ml = Join-Path $env:TEMP ("cct-modelbucket-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $ml | Out-Null
try {
    $nowMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    Set-Content -LiteralPath "$ml\work.json"        -Value ('{"fiveHour":39,"sevenDay":47,"atMs":' + ($nowMs - 600000) + '}')
    Set-Content -LiteralPath "$ml\work.widget.json" -Value ('{"fiveHour":40,"sevenDay":48,"atMs":' + $nowMs + ',"modelSevenDay":15,"modelLabel":"FABLE"}')
    $sum = Get-RateLimitSummary -Directory $ml
    Assert 'the widget record supplies the five-hour percentage' ($sum.work.FiveHour -eq 40)
    Assert 'the model bucket is read from the widget record'     ($sum.work.Model -eq 15)
    Assert 'and its label travels with it'                       ($sum.work.ModelLabel -eq 'FABLE')

    # statusline.js knows nothing about the model bucket. When ITS record is the fresher one the
    # third bar must disappear rather than show a stale number beside two current ones.
    Set-Content -LiteralPath "$ml\personal.widget.json" -Value ('{"fiveHour":10,"sevenDay":20,"atMs":' + ($nowMs - 600000) + ',"modelSevenDay":15,"modelLabel":"FABLE"}')
    Set-Content -LiteralPath "$ml\personal.json"        -Value ('{"fiveHour":11,"sevenDay":21,"atMs":' + $nowMs + '}')
    $sum = Get-RateLimitSummary -Directory $ml
    Assert 'the newer statusline record still supplies the limits' ($sum.personal.FiveHour -eq 11)
    Assert 'a record with no model bucket reports none'            ($null -eq $sum.personal.Model)
    Assert 'and no label either'                                   ($null -eq $sum.personal.ModelLabel)
} finally {
    if (Test-Path -LiteralPath $ml) { Remove-Item -LiteralPath $ml -Recurse -Force -ErrorAction SilentlyContinue }
}

# Get-DefaultAdvisorLabel: what 'default' on the advisor row means right now. Same shape and same
# failure policy as Get-DefaultModelLabel - this is a menu, not a validator, so every failure
# degrades to a plain label instead of taking the launch screen down.
$adv = Join-Path $env:TEMP ("cct-advisor-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $adv | Out-Null
try {
    Set-Content -LiteralPath "$adv\set.json"   -Value '{ "model": "fable", "advisorModel": "fable" }'
    Set-Content -LiteralPath "$adv\unset.json" -Value '{ "model": "fable" }'
    Set-Content -LiteralPath "$adv\bad.json"   -Value '{ "advisorModel": '
    Assert 'a configured advisor model is named on the row' ((Get-DefaultAdvisorLabel -Path "$adv\set.json") -eq 'default (fable)')
    Assert 'no advisorModel key says so explicitly'         ((Get-DefaultAdvisorLabel -Path "$adv\unset.json") -eq 'default (none)')
    Assert 'a missing settings file degrades to default'    ((Get-DefaultAdvisorLabel -Path "$adv\nope.json") -eq 'default')
    Assert 'unparseable settings degrade to default'        ((Get-DefaultAdvisorLabel -Path "$adv\bad.json") -eq 'default')
} finally {
    if (Test-Path -LiteralPath $adv) { Remove-Item -LiteralPath $adv -Recurse -Force -ErrorAction SilentlyContinue }
}

# --- launch hooks: best-effort, in order, a failure is one line ------------------------------------
$hookDir = Join-Path $env:TEMP ("cal-hooks-" + [guid]::NewGuid().ToString('N')); New-Item -ItemType Directory $hookDir | Out-Null
try {
    Set-Content (Join-Path $hookDir 'one.ps1') 'Write-Output "one ran"'
    Set-Content (Join-Path $hookDir 'two.ps1') 'Write-Output "two broke"; exit 3'
    Set-Content (Join-Path $hookDir 'three.ps1') 'Write-Output "three ran"'
    Set-Content (Join-Path $hookDir 'four.hook') 'not a script'
    $lines = @(Invoke-LaunchHooks -Hooks @((Join-Path $hookDir 'one.ps1'), (Join-Path $hookDir 'two.ps1'), (Join-Path $hookDir 'missing.ps1'), (Join-Path $hookDir 'four.hook'), (Join-Path $hookDir 'three.ps1')) 6>&1 | ForEach-Object { "$_" })
    Assert 'hook stdout is printed verbatim'       ($lines -contains 'one ran')
    Assert 'a failing hook reports its exit code'  (@($lines | Where-Object { $_ -match '^\s*hook two\.ps1 exit 3' }).Count -eq 1)
    Assert 'a missing hook is one line'            (@($lines | Where-Object { $_ -match 'missing\.ps1.*not found' }).Count -eq 1)
    # An unknown extension is refused, never executed: `& file.hook` would ShellExecute the association or throw,
    # and a hook that runs nothing must not inherit the previous hook's $LASTEXITCODE. Counting
    # lines that mention 'four.hook' AT ALL (not the narrower 'unsupported extension' phrase) is
    # what catches the reset being removed: without it, $LASTEXITCODE still carries two.ps1's exit
    # 3, and the default branch's own message is followed by a second, spurious "hook four.hook
    # exit 3: " line - the narrower match would still see exactly one hit and stay green.
    Assert 'an unknown extension is refused, and nothing else is said about it' (@($lines | Where-Object { $_ -match 'four\.hook' }).Count -eq 1)
    Assert 'the one line about it names the reason'                            (@($lines | Where-Object { $_ -match 'four\.hook.*unsupported extension' }).Count -eq 1)
    Assert 'later hooks still run'                 ($lines -contains 'three ran')
    Assert 'an empty list prints nothing'          (@(Invoke-LaunchHooks -Hooks @() 6>&1).Count -eq 0)
} finally { Remove-Item $hookDir -Recurse -Force }

# --- Get-McpConfigPaths: extras only when present, rider by mode ------------------------------------
$origCfg = $LauncherConfig
try {
    $extra = Join-Path $env:TEMP ("cal-extra-" + [guid]::NewGuid().ToString('N') + '.json'); Set-Content $extra '{}'
    $LauncherConfig = [pscustomobject]@{ ExtraMcpConfigs = @($extra, 'C:\nope\absent.json'); RiderMcp = 'off' }
    $paths = @(Get-McpConfigPaths 6>$null)
    Assert 'an existing extra is passed'           ($paths -contains $extra)
    Assert 'a missing extra is skipped silently'   ($paths.Count -eq 1)
    $LauncherConfig = [pscustomobject]@{ ExtraMcpConfigs = @(); RiderMcp = 'off' }
    $noise = @(Get-McpConfigPaths 6>&1)
    Assert 'rider off: no rider line at all'       (@($noise | Where-Object { "$_" -match 'rider' }).Count -eq 0)
} finally { $LauncherConfig = $origCfg; Remove-Item $extra -Force -ErrorAction SilentlyContinue }

# --- Get-AccountPrompt: the fallback prompt is generated from the roster ------------------------------
$p = Get-AccountPrompt -Accounts $Accounts -Default $CanonicalAccount
Assert 'prompt lists visible accounts by first letter' ($p.Text -eq 'Claude account: [w]ork / [p]ersonal / [l]ow, Enter = work : ')
Assert 'hidden account is typeable, not advertised' ($p.Map['s'] -eq 'shared')
$one = Get-AccountPrompt -Accounts (Get-LauncherDefaults).Accounts -Default 'work'
Assert 'one account: no prompt text'             ($one.Text -eq '')

# deferred review finding: Text advertises the raw-case first letter while Map is lower-cased, so
# a mixed-case key ("Work") advertised "[W]ork" for a letter the map only accepts as lower-case 'w'.
$mixed = @(
    [pscustomobject]@{ Key = 'Work'; Hidden = $false }
    [pscustomobject]@{ Key = 'Personal'; Hidden = $false }
)
$mp = Get-AccountPrompt -Accounts $mixed -Default 'Work'
Assert 'mixed-case key: the advertised letter matches the map''s case' ($mp.Text -ceq 'Claude account: [w]ork / [p]ersonal, Enter = Work : ')
Assert 'mixed-case key: the map still keys on the lower-case letter'   ($mp.Map['w'] -eq 'Work')

# --- Resolve-AccountAnswer: what the no-UI fallback prompt does with a typed answer -------------------
# Extracted out of claude-auto.ps1's fallback branch so the mapping itself is testable without a
# console: every case below is a scenario the bare-Enter/no-UI prompt has to get right.
Assert 'empty answer keeps the default'                ((Resolve-AccountAnswer -Answer '' -Prompt $p.Map -Default 'work') -eq 'work')
Assert 'whitespace-only answer keeps the default'       ((Resolve-AccountAnswer -Answer '   ' -Prompt $p.Map -Default 'work') -eq 'work')
Assert 'an unmapped letter keeps the default'           ((Resolve-AccountAnswer -Answer 'z' -Prompt $p.Map -Default 'work') -eq 'work')
Assert 'an exact key typed in full resolves like its shorthand' ((Resolve-AccountAnswer -Answer 'personal' -Prompt $p.Map -Default 'work') -eq 'personal')
Assert 'a hidden account''s letter still resolves'      ((Resolve-AccountAnswer -Answer 's' -Prompt $p.Map -Default 'work') -eq 'shared')
Assert 'mixed-case input resolves the same as lower-case' ((Resolve-AccountAnswer -Answer 'P' -Prompt $p.Map -Default 'work') -eq 'personal')

# --- Resolve-ClaudeExecutable: PATH resolution must never throw and never silently degrade to
# exit 0 -------------------------------------------------------------------------------------------
# deferred review finding: `.Source` on a $null match (no -ErrorAction on the old inline
# Get-Command) is $null, `& $null @args` throws, and `exit $claudeExit` with that variable never set
# exited 0 - a launch that found nothing to run reported success. -Resolver is injected so both
# branches are assertable without touching the real PATH.
$found = Resolve-ClaudeExecutable -Resolver { [pscustomobject]@{ Source = 'C:\fake\claude.cmd' } }
Assert 'a resolver that finds claude reports Ok'          $found.Ok
Assert 'and carries its resolved path'                    ($found.Path -eq 'C:\fake\claude.cmd')
Assert 'and no message'                                   ($null -eq $found.Message)

$missing = Resolve-ClaudeExecutable -Resolver { $null }
Assert 'a resolver that finds nothing reports not Ok, never throws' (-not $missing.Ok)
Assert 'and carries no path'                                        ($null -eq $missing.Path)
Assert 'and names the problem'                                      ($missing.Message -match 'not on PATH')

# --- Get-RateLimitSummary: a machine with no records (a fresh install) gets an empty table ---------
$emptyLimits = Join-Path $env:TEMP ("cal-limits-" + [guid]::NewGuid().ToString('N')); New-Item -ItemType Directory $emptyLimits | Out-Null
try { Assert 'no rate-limit records: empty table, no error' ((Get-RateLimitSummary -Directory $emptyLimits).Count -eq 0) } finally { Remove-Item $emptyLimits -Recurse -Force }

} finally { Remove-Item Env:CLAUDE_AUTO_CONFIG -ErrorAction SilentlyContinue }

if ($script:Ran -ne 110) {
    Write-Host "COULD NOT RUN: expected 110 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)" -ForegroundColor Red
    exit 2
}
if ($script:fail -gt 0) {
    Write-Host "$script:fail assertion(s) failed" -ForegroundColor Red
    exit 1
}
# Counted, not guessed: HEAD claimed 72 while running 75 (measured 2026-09-04 by counting the
# ok/FAIL lines of a bare run). A banner nobody re-counts is a number that drifts silently.
Write-Host '110 assertions, all pass' -ForegroundColor Green
exit 0
