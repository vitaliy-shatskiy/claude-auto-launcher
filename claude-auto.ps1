# Launcher for Claude Code: picks the account, repairs shared profile files, discovers MCP
# configs, then starts claude with the caller's arguments passed through untouched (the bodies
# live in claude-auto\*.ps1; this file is the order they run in). ~/.claude/claude-auto.json
# (or $env:CLAUDE_AUTO_CONFIG) decides the account roster and every optional step - sharing
# repairs, the remote/companion path, launch hooks. Everything after the account choice is
# best-effort: a failure there must never stop Claude from starting, so each step is wrapped
# and only warns. There is deliberately no param() block - adding one changes how PowerShell
# binds --resume, --continue and -p, and passing those through untouched is the hard invariant.

# The launcher's own version. Deliberately NOT on --version: that argument is passed through to
# claude, and the headless regression check depends on it staying that way.
$script:LauncherVersion = '0.1.0'
if ($args.Count -eq 1 -and $args[0] -eq '--launcher-version') {
    Write-Host "claude-auto $script:LauncherVersion"
    exit 0
}

# PowerShell 7 or nothing. PowerShell resolves this .ps1 ahead of claude-auto.cmd, so the shim that
# exists to force pwsh is bypassed whenever `claude-auto` is typed in a 5.1 window - hence the
# re-exec. Without pwsh at all this used to carry on and start a session with no MCP configuration,
# described as "the full UI but no MCP". That was false: several modules do not even PARSE under
# 5.1, so every helper was missing and the launcher fell through to a bare `claude` with no account
# choice, no sharing repair and no config. install.ps1 already refuses to install without pwsh 7;
# refusing here says the same thing at the same volume instead of degrading into something else.
if ($PSVersionTable.PSVersion.Major -lt 6) {
    $pwshExe = Get-Command pwsh -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($pwshExe) {
        & $pwshExe.Source -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath @args
        exit $LASTEXITCODE
    }
    Write-Host "  claude-auto needs PowerShell 7 and this is Windows PowerShell $($PSVersionTable.PSVersion)." -ForegroundColor Red
    Write-Host "  Install it - winget install Microsoft.PowerShell - and run claude-auto again." -ForegroundColor Red
    exit 1
}

$ModuleDir = Join-Path $PSScriptRoot 'claude-auto'
$ModulesOk = $true
foreach ($m in @('Config.ps1', 'Env.ps1', 'Remote.ps1', 'Sessions.ps1', 'Projects.ps1', 'Theme.ps1', 'Layout.ps1', 'Screens.ps1', 'Prefs.ps1', 'Input.ps1', 'Ui.ps1', 'Maintenance.ps1')) {
    try { . (Join-Path $ModuleDir $m) }
    catch {
        Write-Host "  module $m failed to load: $($_.Exception.Message)" -ForegroundColor DarkYellow
        $ModulesOk = $false
    }
}
$UiOk = $ModulesOk
if ($ModulesOk) {
    Set-LaunchRoster -Accounts $Accounts -Remote:([bool]$LauncherConfig.Remote) -Default $CanonicalAccount
    foreach ($w in @($LauncherConfig.Warnings)) { Write-Host "  config: $w" -ForegroundColor DarkYellow }
}

# Before modularisation this file was self-contained and its helpers could not go missing. Now a
# broken or absent module would make the very first helper call throw and kill the launcher BEFORE
# Claude starts - strictly worse than the prompts it replaced. So a load failure degrades to a bare
# session rather than to no session. Loud on purpose: MCP servers and the account choice are gone
# in this mode, and silently starting a crippled session would be worse than saying so.
if (-not $ModulesOk) {
    Write-Host "  starting WITHOUT profile choice, secrets or MCP config - fix the error above" -ForegroundColor Red
    # Deliberately NOT Resolve-ClaudeExecutable (Env.ps1): a module failed to load and Env.ps1 may be
    # the one that failed, so this fallback must resolve claude itself rather than lean on a function
    # that might not exist. Same guard inline: -ErrorAction so a missing claude does not throw on
    # `.Source`, and an explicit exit 1 so a launch that found nothing to run never reports success.
    $bareCmd = Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $bareCmd) {
        Write-Host "  claude is not on PATH - install Claude Code first" -ForegroundColor Red
        exit 1
    }
    & $bareCmd.Source @args
    exit $LASTEXITCODE
}

# Preview is an environment variable, never a parameter: a param() block would change how
# --resume/--continue/-p bind, and passing those through untouched is the hard invariant.
$Preview = ($env:CLAUDE_AUTO_PREVIEW -eq '1')

# Every real launch leaves a trace in ~/.claude/launcher-logs/claude-auto-<date>.jsonl: a `start`
# record here, `ui` when the menu closes, `decision` with the exact argv, `exit` with the code and
# the wall time. Added 2026-08-16 after a burst of Claude sessions in a Rider project restarted
# roughly every twelve seconds and nothing on disk could say who spawned them - the launcher was
# the only suspect that could be ruled out, and only because a side effect (the per-PID rider MCP
# temp file) happened to be datable. Guessing from side effects is what this replaces.
#
# The `start` record is written BEFORE any work, so a launcher that hangs in the menu or is killed
# still leaves its parent chain behind; a run with `start` and no `exit` is itself the finding.
# Preview stays out: the suites and check-launcher-regression.ps1 drive that path, and preview must
# remain side-effect-free.
$RunId = [guid]::NewGuid().ToString('N').Substring(0, 12)
$LaunchStartedAt = Get-Date
if (-not $Preview) {
    Remove-OldLauncherLogs
    $null = Write-LauncherLog -Stage 'start' -RunId $RunId -Data @{
        cwd              = $PWD.Path
        args             = @($args)
        inputRedirected  = [Console]::IsInputRedirected
        outputRedirected = [Console]::IsOutputRedirected
        psVersion        = $PSVersionTable.PSVersion.ToString()
        configDir        = $env:CLAUDE_CONFIG_DIR
        # Which terminal this came out of. Rider's own terminal sets TERMINAL_EMULATOR, Windows
        # Terminal sets WT_SESSION, and CLAUDE_CODE_ENTRYPOINT is set when Claude Code itself is
        # the caller - the three cases that looked identical while debugging the restart burst.
        env              = @{
            TERMINAL_EMULATOR      = $env:TERMINAL_EMULATOR
            TERM_PROGRAM           = $env:TERM_PROGRAM
            WT_SESSION             = $env:WT_SESSION
            CLAUDE_CODE_ENTRYPOINT = $env:CLAUDE_CODE_ENTRYPOINT
            CLAUDE_NO_ROAM         = $env:CLAUDE_NO_ROAM
            # Decides whether the `ide` MCP server attaches. Whether `ide` was in a session's MCP
            # log is what separated launcher starts from everything else while reconstructing the
            # 2026-08-16 burst - recording the cause beats inferring it from the effect next time.
            CLAUDE_CODE_SSE_PORT   = $env:CLAUDE_CODE_SSE_PORT
        }
        parents          = @(Get-LauncherParentChain)
    }
}

# Preview alone is not enough to verify the flow from a non-interactive session, because
# [Console]::ReadKey needs a real console. CLAUDE_AUTO_PREVIEW_KEYS supplies the keystrokes
# instead - comma separated, e.g. "RightArrow,Enter" - so the screens can be driven and their
# outcome asserted without a terminal. Ignored unless preview is on.
$KeySource = { [Console]::ReadKey($true) }
if ($Preview -and $env:CLAUDE_AUTO_PREVIEW_KEYS) {
    $KeySource = New-ScriptedKeyReader -Keys ($env:CLAUDE_AUTO_PREVIEW_KEYS -split ',')
}

# The default account is the default: it is also what a launcher fed `< NUL` runs under. The
# working directory never decides the account - both kinds of project sit in the same folders.
# Same resolution the UI uses (Get-LaunchDefaultAccount in Screens.ps1, set by Set-LaunchRoster
# above) - a hidden canonical account must not disagree with what the launch screen defaults to.
$choice = Get-LaunchDefaultAccount
$remote = [bool]$LauncherConfig.Remote      # when on: the session is reachable from the phone
$showQr = $false
$stopServer = $false
$resumeId = $null
$forkSession = $false
$previewPickerCancelled = $false

# The screen draws only for a bare interactive launch. Redirected stdin means something is driving
# this unattended - a scheduled task feeding it `< NUL` to take the defaults; arguments mean the
# caller already knows what it wants. Either way the UI must stay out of the way.
$UseUi = $UiOk -and ($Preview -or (-not [Console]::IsInputRedirected -and $args.Count -eq 0))
if ($UseUi -and -not $Preview) {
    try { $null = $Host.UI.RawUI.WindowSize } catch { $UseUi = $false }
}

if ($UseUi) {
    $limits = Get-RateLimitSummary
    $useColor = Test-ColorSupported
    $ascii = Test-AsciiRequired
    $defaultModelLabel = Get-DefaultModelLabel
    $defaultAdvisorLabel = Get-DefaultAdvisorLabel
    $version = @{}
    try {
        $vi = Get-ClaudeInstallInfo
        $version = @{ Installed = (& claude --version 2>$null | Select-Object -First 1); Newest = $vi.NewestVersion }
    } catch { }

    $alt = Test-AltBufferSupported
    $mouse = $null
    try {
        if ($alt) { Enter-AltBuffer }
        # Arm the console for mouse input. $null when there is no console, when the interop will not
        # load, or in preview - preview must stay side-effect-free, and it has no console anyway.
        # Everything downstream treats $null as "keyboard only", so a failure here costs a feature
        # and never a launch. The restore is in the finally AND on the early exit below, because
        # leaving QuickEdit disabled changes how the owner's terminal behaves afterwards.
        if (-not $Preview) { $mouse = Open-ClaudeConsoleInput }
        # Preview has no real console (the harness that drives it has none either - verified: even
        # [Console]::WindowWidth throws "the handle is invalid" there), same reason
        # Test-AltBufferSupported already special-cases it above. A fixed 78x24 and an always-ready
        # key queue keep the scripted-key flow off [Console] entirely, matching how Wait-KeyOrResize
        # is exercised in Test-Ui.ps1.
        $size = { if ($Preview) { @(78, 24) } else { @([Console]::WindowWidth, [Console]::WindowHeight) } }
        $keyAvailable = { if ($Preview) { $true } else { [Console]::KeyAvailable } }
        $paint = { param($lines) if ($alt) { Write-Frame -Lines $lines } else { $lines | ForEach-Object { Write-Host $_ } } }

        $draw = {
            param($s)
            $w, $h = & $size
            # Returns the row map and nothing else; $paint writes through the console and emits no
            # pipeline output. Invoke-LaunchScreen needs it to turn a click into a row and a value.
            $map = $null
            # No -Restored/-RestoredAge here any more: they now live ON the state, and switching
            # tabs recomputes both. Passing the pre-screen copy would override the frame's default
            # and freeze the marks of whichever tab opened first - every suite green, the live
            # screen wrong from the first account switch.
            & $paint (Get-LaunchFrame -State $s -Width $w -Height $h -Limits $limits -Version $version -DefaultModelLabel $defaultModelLabel -DefaultAdvisorLabel $defaultAdvisorLabel -Color:$useColor -Ascii:$ascii -RowMap ([ref]$map))
            $map
        }
        $wait = { $w, $h = & $size; Wait-KeyOrResize -ReadKey $KeySource -Width $w -Height $h -GetSize $size -KeyAvailable $keyAvailable -MouseState $mouse }

        # Esc in the picker lands back on this screen rather than silently starting a new session:
        # cancelling a resume is a change of mind about which session, not about launching at all.
        # Preferences load ONLY here, inside the interactive branch. An unattended caller feeds the
        # launcher `< NUL` and takes the defaults; a scheduled run that silently inherited
        # yesterday's --effort max would spend the weekly limit with nobody watching.
        # Read once and keep it: the screen needs the OTHER accounts' profiles too, so that arriving
        # at a tab with no stash yet loads what that account last launched with.
        $prefs = Read-LaunchPrefs
        $merged = Merge-LaunchPrefs -State (New-LaunchState) -Prefs $prefs -Rows (Get-LaunchRows)
        $state = $merged.State
        # Kept for the `ui` log below only - it is the state the FILE produced, before the screen.
        # The frame reads the live marks off $state itself now (they change with every tab switch).
        $restored = $merged.Restored

        # Built once, before the loop: Get-ProjectRegistry scans the filesystem, so re-running it on
        # every pass through the loop (a rejected free path, a trip back from the picker) would cost
        # a rescan for nothing the loop itself changes. Preview must stay side-effect-free: the
        # registry's cache write is a WRITE like any other guarded on this path, so preview gets its
        # own per-PID scratch cache under %TEMP% instead of the real one under ~/.claude - reading
        # the real ~/.claude/projects directory itself is unavoidable (the screen has nothing to
        # list otherwise) and mirrors Get-RateLimitSummary's own unguarded read a few lines up.
        $LaunchCwd = $PWD.Path
        $projectsCacheArgs = if ($Preview) { @{ CachePath = (Join-Path $env:TEMP "claude-auto-projects-preview-$PID.json") } } else { @{} }
        $projects = @(Get-ProjectRegistry @projectsCacheArgs)
        # cwd when it is a known project or holds a .git; otherwise what this account launched last;
        # otherwise nothing, and the project screen opens with the cursor at the top.
        $projectSource = Set-LaunchStartProject -State $state -Cwd $LaunchCwd -Projects $projects

        # The free-path prompt itself is Read-ClaudeFreePath (Input.ps1) - see its own comment for
        # why the re-arm MUST come back through a return value, never a plain `$mouse = ...` inside
        # this scriptblock (fix round 1, CRITICAL 1: that assignment ran in the child scope `&`
        # creates for the call and never reached the script-scope $mouse the rest of this file
        # reads, so the caller kept polling a CLOSED handle - a tight 100%-CPU loop with no sleep,
        # reachable from the very first rejected free path). `$script:mouse` here is not optional.
        # NEEDS THE OWNER'S HAND IN A REAL TERMINAL: no suite can drive a real [Console]::ReadLine,
        # so the actual keystroke handling is unverified past what Read-ClaudeFreePath's own
        # injected-dependency unit tests (Test-Input.ps1) prove.
        $readClaudePath = {
            # -RestoreCursorHidden:$alt - only Exit-AltBuffer ever shows the cursor again, and it
            # only runs `if ($alt)`; a non-alt-buffer session (no Enter-AltBuffer ?25l) must not have
            # this prompt hide the cursor on its way out, or nothing ever shows it again (review
            # item B, Task 10).
            $result = Read-ClaudeFreePath -MouseState $mouse -Rearm:(-not $Preview) -GetSize $size -RestoreCursorHidden:$alt
            $script:mouse = $result.MouseState
            return $result.Line
        }

        while ($true) {
            $state = Invoke-LaunchScreen -State $state -ReadKey $KeySource -Wait $wait -Draw $draw -Prefs $prefs -OnKey {
                param($k)
                # 'u' opens maintenance from anywhere on the launch screen and returns here.
                # Test-ClaudeHotkey: any layout (virtual key, or the Cyrillic letter on that key),
                # never uppercase - an uppercase U from a terminal that reports the mouse as text
                # would open maintenance on a hover (caught live 2026-08-25).
                if (Test-ClaudeHotkey -Key $k -Char 'u') {
                    $mdraw = {
                        param($info, $status)
                        $w, $h = & $size
                        $mmap = $null
                        & $paint (Get-MaintenanceFrame -Info $info -Width $w -Height $h -Status $status -Color:$useColor -Ascii:$ascii -RowMap ([ref]$mmap) -Actions $LauncherConfig.MaintenanceActions)
                        $mmap
                    }
                    Invoke-MaintenanceScreen -ReadKey $KeySource -Wait $wait -Draw $mdraw -Actions $LauncherConfig.MaintenanceActions -Drain { $null = Clear-ClaudeInputQueue -State $mouse }
                    return $true
                }
                return $false
            }
            # `exit` here leaves the script, and a console left armed keeps QuickEdit disabled for
            # the rest of the terminal's life - so the restore is explicit rather than trusted to
            # the finally. Close-ClaudeConsoleInput is idempotent, so running it twice is free.
            if ($null -eq $state) {
                $null = Close-ClaudeConsoleInput -State $mouse; if ($alt) { Exit-AltBuffer }
                if (-not $Preview) {
                    $null = Write-LauncherLog -Stage 'cancelled' -RunId $RunId -Data @{
                        ms = [int]((Get-Date) - $LaunchStartedAt).TotalMilliseconds
                    }
                }
                exit 0
            }
            # EIGHT parameters, and both of the last two forwarded. A renderer one short does not
            # fail: the extra arguments land in $args and the action field draws 'new' on every
            # frame while the loop is on 'resume' - which is exactly what a preview run showed
            # before these lines existed. Test-Maintenance pins the shape as source, because this
            # scriptblock has a console and no suite can drive it.
            $projDraw = {
                param($p, $i, $f, $t, $h, $n, $a, $oa)
                $w, $hh = & $size
                $pmap = $null
                & $paint (Get-ProjectFrame -Projects $p -Index $i -Filter $f -Typing:$t -Hover $h -Notice $n -Cwd $LaunchCwd `
                          -Action $a -OnAction:$oa `
                          -Width $w -Height $hh -Color:$useColor -Ascii:$ascii -RowMap ([ref]$pmap))
                $pmap
            }
            $chosen = Invoke-ProjectScreen -Projects $projects -Cwd $LaunchCwd -Initial "$($state.Project)" `
                      -InitialAction "$($state.ProjectAction)" `
                      -ReadKey $KeySource -Wait $wait -Draw $projDraw -ReadPath $readClaudePath
            # Escape at the project screen goes back to the launch screen, exactly as Escape at the
            # session picker already does. Preview cannot loop - its key list is finite - so it breaks.
            if (-not $chosen) { if ($Preview) { $previewPickerCancelled = $true; break }; continue }
            $state.Project = $chosen.Path
            $state.ProjectSlug = $chosen.Slug
            $state.ProjectSlugs = @($chosen.Slugs)
            $state.Action = $chosen.Action
            # And onto the field's own state, which Prefs.ps1 remembers per account. Two properties
            # rather than one: $Action describes THIS launch (Reset-LaunchTab and Save-LaunchPrefs
            # both refuse to remember it), $ProjectAction describes the habit the screen opens on.
            $state.ProjectAction = $chosen.Action
            if ($state.Action -ne 'resume') { break }

            $projectName = Split-Path -Path $state.Project -Leaf
            $pdraw = {
                param($s, $i, $f, $scope, $name)
                $w, $h = & $size
                # The row map is the ONLY thing this returns: $paint writes through
                # [Console]::Write / Write-Host and emits nothing to the pipeline. The picker needs
                # it to turn a click into a session, and it comes from the renderer so the two can
                # never disagree about which line holds which row.
                $map = $null
                & $paint (Get-PickerFrame -Sessions $s -Index $i -Filter $f -Scope $scope -ProjectName $name -Width $w -Height $h -Color:$useColor -Ascii:$ascii -RowMap ([ref]$map))
                $map
            }
            # deferred review finding: this used to call Get-ClaudeSessions with no root at all,
            # which lists the CANONICAL account's sessions regardless of which account is selected -
            # with sharing off, resuming one then attaches under a CLAUDE_CONFIG_DIR that never held it.
            $sessionsRoot = Get-SessionsRootForAccount -Account $state.Account -ProfileRoots $ProfileRoots
            # @() is load-bearing (same trap as $mcpConfigs below): Get-ClaudeSessions returning ZERO
            # items unrolls to $null on the pipeline, and Invoke-SessionPicker's -Sessions is
            # Mandatory - an account with no sessions yet (a fresh secondary root, or any account
            # once the root fix above actually scopes to it) crashed here with a raw PowerShell
            # binding error instead of showing an empty picker. Found via tests\check-preview.ps1.
            # SCOPED, both halves. The picker shows the project the owner just chose, so the page it
            # is handed - and every page it later asks for - must be that project's. Handing a
            # scoped picker an unscoped page of ten opened it EMPTY whenever the chosen project's
            # newest session was not among the ten newest of the whole account, and each Down then
            # paged other projects' transcripts that the scope threw away (adversarial review
            # 2026-09-16, G1/G2/D5). Tab asks the fetcher again with an empty scope, so widening to
            # the whole account is one page of disk, not a second read of everything.
            #
            # SNAPSHOT: the file list is enumerated once per scope and every page is -Skip over THAT
            # list, so a transcript appended while the picker is open cannot shift the window and
            # hide a session behind the gap (C4).
            #
            # PAGED: the first frame costs one page of summarising, not forty. Measured on this
            # machine over the live projects root, cold: 40 sessions 1 333 ms, 10 sessions ~240 ms.
            # The picker asks for the next page when the cursor reaches the last row, so the rest is
            # paid for by whoever actually scrolls that far.
            $sessionPageSize = 10
            $sessionSnapshots = @{}
            # The snapshot is stored in a [string[]] VARIABLE and handed to -Files as that variable,
            # never as an argument expression: a function returning an empty array unrolls to $null
            # on the pipeline, -Files would come back unbound, and Get-ClaudeSessions would silently
            # fall back to the whole unscoped account - which is exactly how a two-slug project
            # opened its picker on ten foreign rows (re-review 2026-09-16, C1).
            $fetchNextPage = { param($have, [string[]]$ProjectSlug)
                $k = (@($ProjectSlug | Where-Object { $_ }) -join '|')
                if (-not $sessionSnapshots.ContainsKey($k)) {
                    $sessionSnapshots[$k] = [string[]](Get-ClaudeSessionFile -ProjectsRoot $sessionsRoot -ProjectSlug $ProjectSlug)
                }
                $snapshot = [string[]]$sessionSnapshots[$k]
                @(Get-ClaudeSessions -ProjectsRoot $sessionsRoot -Limit $sessionPageSize -Skip $have -Files $snapshot)
            }.GetNewClosure()
            $pickerSlugs = @($state.ProjectSlugs | Where-Object { $_ })
            if ($pickerSlugs.Count -eq 0 -and $state.ProjectSlug) { $pickerSlugs = @("$($state.ProjectSlug)") }
            $picked = Invoke-SessionPicker -Sessions @(& $fetchNextPage 0 $pickerSlugs) `
                      -FetchMore $fetchNextPage `
                      -ProjectSlug $pickerSlugs -ProjectName $projectName -ReadKey $KeySource -Wait $wait -Draw $pdraw
            if ($picked) { $resumeId = $picked.Session.SessionId; $forkSession = [bool]$picked.Fork; break }
            # Escape at the picker returns $null (cancel) and, in a real session, this loop goes back
            # to the launch screen. Preview cannot loop - the scripted key list is finite - so it must
            # break here too, but breaking silently would fall through to the ordinary preview summary
            # below and print a default launch, which reads exactly like "cancelling starts a session".
            # The flag lets the Preview block downstream report the cancellation instead.
            if ($Preview) { $previewPickerCancelled = $true; break }
        }
    } finally {
        $null = Close-ClaudeConsoleInput -State $mouse
        if ($alt) { Exit-AltBuffer }
    }

    # Captured, not discarded: the prefs file is untracked by git and every launch overwrites it, so
    # what a launch remembered is unreadable the moment the next one starts. On 2026-08-23 that made
    # a "my settings reset themselves" report unanswerable from the file - the launch log had to be
    # reconstructed from argv instead. The record goes into the `ui` line below.
    # Guarded like every other write on this path: a preview run created the prefs file from
    # nothing, and on a machine that already had one it would have rewritten the remembered choices
    # of a launch that never happened. The suites hid it - they point CLAUDE_AUTO_PREFS at a
    # throwaway file, so the write landed somewhere harmless and nothing ever noticed.
    $savedPrefs = if ($Preview) { $null } else { Save-LaunchPrefs -State $state }

    # 'off' on the advisor row is an ENVIRONMENT variable, not a flag - the CLI has no --advisor off
    # (code.claude.com/docs/en/advisor.md). Set here, before either exec path, so the child inherits
    # it whether the session is born through crc or directly. Never unset in the other direction: an
    # owner who exported it themselves means it, and a menu default must not quietly override that.
    if ($state.Advisor -eq 'off') { $env:CLAUDE_CODE_DISABLE_ADVISOR_TOOL = '1' }

    $choice = $state.Account
    if ($LauncherConfig.Remote) {
        if ($state.Remote -eq 'off') { $remote = $false }
        elseif ($state.Remote -eq 'on+QR') { $showQr = $true }
        elseif ($state.Remote -eq 'stop server') { $remote = $false; $stopServer = $true }
    }

    # How long the menu was on screen, and what it returned. A launch that never gets past here
    # leaves `start` with no `ui`, which separates "stuck in the menu" from "started and died".
    if (-not $Preview) {
        $null = Write-LauncherLog -Stage 'ui' -RunId $RunId -Data @{
            ms       = [int]((Get-Date) - $LaunchStartedAt).TotalMilliseconds
            account  = $choice
            action   = $state.Action
            remote   = if ($LauncherConfig.Remote) { $state.Remote } else { $null }   # no row, no value
            # The two rows that decide what a session costs. Without them the log says which account
            # paid but not what it was spending on, which is half the question every time.
            effort   = $state.Effort
            advisor  = $state.Advisor
            resumeId = $resumeId
            fork     = $forkSession
            restored = $restored
            # `restored` is the state the prefs file produced BEFORE the screen; `saved` is what the
            # screen left behind. The two differing is the whole signature of a launch that changed
            # the habit, whether that was deliberate or a stray `r`.
            saved    = $savedPrefs
            # Without BOTH of the next two, "it started in the wrong folder" is unanswerable from the
            # log - the same gap the 2026-08-23 prefs investigation hit, one level up (there it was
            # the account's habits; here it is which directory the session actually runs in).
            launchedFrom   = $LaunchCwd
            project        = $state.Project
            # How the STARTING pick (before the project screen ran) was resolved - cwd/remembered/
            # none, from Resolve-StartProject. Not re-derived if the owner picked a different project
            # on the screen: this answers "why did the screen open where it did", a separate question
            # from "what did it end on" (already `project`, above).
            projectSource  = $projectSource
        }
    }
}
elseif (-not [Console]::IsInputRedirected -and $args.Count -eq 0) {
    # Fallback when the UI cannot run: plain prompts generated from the roster. Hidden accounts are
    # not advertised but stay typeable; only the FIRST letter decides, so a key typed in full cannot
    # fall into another account's branch.
    $prompt = Get-AccountPrompt -Accounts $Accounts -Default (Get-LaunchDefaultAccount)
    if ($prompt.Text) {
        Write-Host $prompt.Text -NoNewline -ForegroundColor Cyan
        $choice = Resolve-AccountAnswer -Answer (Read-Host) -Prompt $prompt.Map -Default $choice
    }
    if ($LauncherConfig.Remote) {
        # One keypress covers the whole remote story. Enter = reachable from the phone, server
        # started on demand. n = plain local session. q = remote plus the pairing QR (must be on
        # screen before Claude takes the terminal over). s = stop the companion server and run local.
        Write-Host "Remote: [Enter] on / [n] off / [q] on + QR / [s] stop server + off : " -NoNewline -ForegroundColor Cyan
        $remoteAnswer = Read-Host
        if ($remoteAnswer -match '^n') { $remote = $false }
        elseif ($remoteAnswer -match '^q') { $showQr = $true }
        elseif ($remoteAnswer -match '^s') { $remote = $false; $stopServer = $true }
    }
}

Set-ClaudeProfile -Account $choice -Preview:$Preview
# BEFORE Import-ProjectSecrets: it derives its slug from $PWD (Env.ps1), so switching after this
# line would load the launch directory's secrets into another project's session. Verified
# 2026-09-15: a child process DOES inherit the directory after Set-Location, so no explicit
# WorkingDirectory start is needed - but [Environment]::CurrentDirectory does NOT follow, so every
# .NET path call past this point must be absolute. Audited Import-ProjectSecrets and
# Get-McpConfigPaths (Env.ps1): neither makes a relative .NET path call - both build every path by
# Join-Path against an already-absolute base ($Root, $env:TEMP), so this switch cannot break either.
# A rejected path (not there, or not a directory) is logged and falls back to the launch cwd rather
# than throwing - Set-Location -LiteralPath on a bad path would otherwise take the whole launcher
# down after everything else already succeeded. Set-ClaudeProjectDirectory (Env.ps1) is the guard
# itself, pulled out so it has a unit seam - Test-Env.ps1 drives it directly.
#
# BOTH halves are guarded on -Preview, not just the cd. Import-ProjectSecrets was unguarded, so a
# preview run imported the LAUNCH directory's secrets into its own process without ever performing
# the cd - process-local and harmless, but a side effect on a path documented as side-effect-free,
# and it meant no preview-driven check could ever exercise this ordering at all (adversarial review
# 2026-09-16, B13). The ORDER itself is pinned as source by Test-Maintenance.
if ($UseUi -and $state -and -not $Preview) {
    $null = Set-ClaudeProjectDirectory -Project $state.Project
}
if (-not $Preview) {
    $null = Import-ProjectSecrets -WorkingDirectory $PWD.Path -Root $LauncherConfig.SecretsRoot
}

# Profile sharing only when the config asks for it: a single-account machine has nothing to link.
# Preview must be side-effect-free - Repair-SharedProfiles (Env.ps1) guards every step on it.
Repair-SharedProfiles -Preview:$Preview
# Config-listed launch hooks (best-effort, each its own line). Preview stays side-effect-free.
if (-not $Preview) { Invoke-LaunchHooks -Hooks $LauncherConfig.LaunchHooks }

# @() is load-bearing: a PowerShell function returning ONE item returns a scalar String, and
# splatting a string with @ enumerates it CHARACTER BY CHARACTER - `--mcp-config C : \ U s e r s`.
# `.Count` on a string is 1, so a `-gt 0` guard passes and the damage is invisible until claude
# rejects 39 config paths. Fires whenever Rider is closed, because then only one config path exists.
$mcpConfigs = @(Get-McpConfigPaths -Preview:$Preview)

# deferred review finding: `.Source` on $null threw, and `exit $claudeExit` with the variable
# never set exited 0 - a launch that could not even find claude reported success.
$resolvedClaude = Resolve-ClaudeExecutable
if (-not $resolvedClaude.Ok) {
    Write-Host "  $($resolvedClaude.Message)" -ForegroundColor Red
    exit 1
}
$claude = $resolvedClaude.Path

# A session picked in the UI is resumed by id. That works from any directory: passing an id
# explicitly makes Claude Code search the current project, its worktrees, then every other project
# on the machine - which is exactly what the built-in cwd-scoped picker cannot do.
$launchArgs = @($args)
if ($UseUi -and $state) {
    $launchArgs = @(Get-LaunchArgs -State $state -ResumeId $resumeId -Fork:$forkSession)
}

# crc forwards everything after `--` to `claude --bg` verbatim (crc.mjs:37), so session-shaping
# flags can ride along. What it cannot do is attach: it refuses a session id together with
# passthrough flags (crc.mjs:114), and birthing is its whole job. So --resume/-c/-w leave the
# roaming path, and the old `$launchArgs.Count -eq 0` gate would have dropped remote silently the
# moment anyone picked a non-default model. Decided here, before Preview's exit, so preview shows
# the same $remote a real launch would use rather than the pre-crc-gate value.
$selectsSession = @($launchArgs | Where-Object { $_ -in @('--resume', '-c', '-w') }).Count -gt 0
if ($selectsSession -and $remote) {
    # Same gate, different reason: -w CREATES a session (the "cannot resume one" wording is simply
    # wrong for it), while --resume/-c attach to one that already exists. Whether `claude --bg -w`
    # would birth correctly through crc is NOT verified, so -w still leaves the crc path here - this
    # only fixes what gets printed, not which flags trigger the override.
    if ($launchArgs -contains '-w') {
        Write-Host "  remote off for this session: a worktree launch is not routed through crc (unverified interaction)" -ForegroundColor DarkYellow
    } else {
        Write-Host "  remote off for this session: crc births a new session, it cannot attach to an existing one" -ForegroundColor DarkYellow
    }
    $remote = $false
}

if ($Preview) {
    if ($previewPickerCancelled) {
        Write-Host ""
        Write-Host "--- preview: picker cancelled ---" -ForegroundColor Cyan
        Write-Host "in an interactive session this returns to the launch screen; preview cannot loop because the scripted key list is finite"
        exit 0
    }
    Write-Host ""
    Write-Host "--- preview, nothing launched ---" -ForegroundColor Cyan
    Write-Host "account            : $choice"
    Write-Host "CLAUDE_CONFIG_DIR  : $($env:CLAUDE_CONFIG_DIR)"
    Write-Host "remote             : $remote   qr=$showQr   stopServer=$stopServer"
    # Its own line because it is the one choice that leaves NO trace in argv: 'off' is an
    # environment variable, so a preview that printed only the arguments would show a launch
    # indistinguishable from the default one.
    $advisorState = if ($env:CLAUDE_CODE_DISABLE_ADVISOR_TOOL -eq '1') { 'disabled (CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1)' } else { 'enabled' }
    Write-Host "advisor tool       : $advisorState"
    Write-Host "launch args        : $($launchArgs -join ' ')"
    # Print the SAME array that would be splatted, not a re-joined reconstruction of it. The two
    # diverged once: the preview printed one correct `--mcp-config <path>` while the exec path
    # splatted that path character by character, so every automated check passed against a lie.
    Write-Host "command            : $claude $((Get-ClaudeInvocation -LaunchArgs $launchArgs -McpConfigs $mcpConfigs) -join ' ')"
    Write-Host "argv count         : $((Get-ClaudeInvocation -LaunchArgs $launchArgs -McpConfigs $mcpConfigs).Count)"
    exit 0
}

# Births the session daemon-held, so it can be taken over from the phone and handed back.
# `crc` (remote-control-claude-code, installed with `npm link`) runs `claude --bg` and then
# `claude attach`; the terminal here is just the first window on a session the daemon owns.
#
# Only for a plain interactive launch: with arguments the plugin means something specific
# (--resume, --continue, -p) and crc has no place in that. CLAUDE_NO_ROAM=1 opts out entirely.
# A missing crc must never block a session, so it falls through to the plain path and says so.
#
# The .cmd shim, not crc.ps1: PowerShell treats `--` as end-of-parameters and drops it before a
# script shim ever sees it, so `crc -- --mcp-config ...` would arrive as `--mcp-config ...` and
# crc would read the first flag as a session id. Through the .cmd shim `--` survives. Verified.
if ($stopServer) {
    try { Write-Host "  companion server: $(Stop-CompanionServer)" -ForegroundColor Yellow }
    catch { Write-Host "  companion server stop failed: $($_.Exception.Message)" -ForegroundColor DarkYellow }
}
if ($LauncherConfig.Remote -and -not $remote -and -not $selectsSession) {
    # The selectsSession branch above already gave its own, more specific reason - this generic
    # line is only for a user-chosen "off" (UI dropdown or the fallback prompt's [n]). With the
    # feature off in the config there is nothing to announce.
    Write-Host "  remote off for this session - plain claude, not reachable from the phone" -ForegroundColor DarkGray
}

$crc = if (-not $LauncherConfig.Remote -or $env:CLAUDE_NO_ROAM -eq '1' -or -not $remote) { $null } else { Get-Command crc.cmd -ErrorAction SilentlyContinue }

# The Step-1 ruling classifies flags Get-LaunchArgs can produce; it says nothing about raw
# passthrough $args (a caller invoking this script directly with -p/--continue/--version/etc).
# Those never went through Get-LaunchArgs, so they are not provably session-shaping - the original
# "must be empty" gate stays in force for them. Confirmed by check-launcher-regression.ps1: without
# this, `claude-auto.ps1 --version` (args.Count -eq 1, not session-selecting) started entering the
# crc branch, which the pre-v2 launcher never did for any non-empty $args.
if ($LauncherConfig.Remote -and -not $selectsSession -and $env:CLAUDE_NO_ROAM -ne '1' -and $remote -and ($UseUi -or $launchArgs.Count -eq 0)) {
    if ($crc) {
        Write-Host "  roaming window: session will be reachable from the phone (CLAUDE_NO_ROAM=1 to disable)" -ForegroundColor DarkGray
        try {
            $root = if ($env:CLAUDE_REMOTE_ROOT) { $env:CLAUDE_REMOTE_ROOT } else { Join-Path $HOME 'Desktop/Projects/remote-control-claude-code' }
            # Named apart from the launch-screen $state above: this is latent today (nothing on this
            # path reads $state afterwards, it exits via $crcExit below), but a variable named the
            # same as the screen's state object one scope up is a clobber waiting for the next read.
            $companionState = Start-CompanionServer -Root $root
            if ($companionState -match '\(dist\)') {
                # The built server discovers and serves every profile sharing the transcripts
                # directory (issue #48 closed) - it prints the list in its own log.
                Write-Host "  companion server: $companionState (serves all profiles)" -ForegroundColor DarkGray
            } else {
                # tsx fallback = pre-multi-profile code: it covers only the profile it inherits.
                # By VALUE, not by presence: with three accounts, "CLAUDE_CONFIG_DIR is set"
                # no longer means personal.
                $profileName = $choice
                Write-Host "  companion server: $companionState (serving the $profileName profile)" -ForegroundColor DarkGray
            }
            # The phone needs an address it can actually reach. The server binds its tailnet address
            # itself (the first 100.x it finds) plus loopback; printing it here saves guessing which
            # one the app should hold, since the app stores exactly one (issue #14).
            $tailnet = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
                Where-Object { $_.IPAddress -like '100.*' } | Select-Object -First 1 -ExpandProperty IPAddress
            if ($tailnet) { Write-Host "  phone over tailscale: http://${tailnet}:8791" -ForegroundColor DarkGray }
            else { Write-Host "  no tailnet address - the phone can reach this only over USB (adb reverse tcp:8791 tcp:8791)" -ForegroundColor DarkYellow }
        } catch {
            Write-Host "  companion server not started: $($_.Exception.Message)" -ForegroundColor DarkYellow
        }

        if ($showQr) {
            $root = if ($env:CLAUDE_REMOTE_ROOT) { $env:CLAUDE_REMOTE_ROOT } else { Join-Path $HOME 'Desktop/Projects/remote-control-claude-code' }
            Show-PairingQr -Root $root
        }

        $crcArgs = Get-ClaudeInvocation -LaunchArgs $launchArgs -McpConfigs $mcpConfigs
        $null = Write-LauncherLog -Stage 'decision' -RunId $RunId -Data @{
            via        = 'crc'
            exe        = $crc.Source
            account    = $choice
            useUi      = $UseUi
            remote     = $remote
            mcpConfigs = @($mcpConfigs)
            argv       = @($crcArgs)
            ms         = [int]((Get-Date) - $LaunchStartedAt).TotalMilliseconds
        }
        & $crc.Source -- @crcArgs
        # Read $LASTEXITCODE into a variable FIRST: any command in between - the log write included
        # - overwrites it, and `exit $LASTEXITCODE` would then report the logger's status.
        $crcExit = $LASTEXITCODE
        $null = Write-LauncherLog -Stage 'exit' -RunId $RunId -Data @{
            via  = 'crc'
            code = $crcExit
            ms   = [int]((Get-Date) - $LaunchStartedAt).TotalMilliseconds
        }
        exit $crcExit
    }
    Write-Host "  crc not on PATH - plain session, not reachable from the phone (npm link in the server/ directory)" -ForegroundColor DarkYellow
}

# Ordering (--mcp-config last, because it is variadic) lives in Get-ClaudeInvocation, which the
# preview prints from too - so what is shown and what runs cannot drift apart again.
$claudeArgs = Get-ClaudeInvocation -LaunchArgs $launchArgs -McpConfigs $mcpConfigs
$null = Write-LauncherLog -Stage 'decision' -RunId $RunId -Data @{
    via        = 'claude'
    exe        = $claude
    account    = $choice
    useUi      = $UseUi
    remote     = $remote
    mcpConfigs = @($mcpConfigs)
    argv       = @($claudeArgs)
    ms         = [int]((Get-Date) - $LaunchStartedAt).TotalMilliseconds
}
& $claude @claudeArgs
# Same trap as the crc branch: capture the exit code before the logger runs.
$claudeExit = $LASTEXITCODE
$null = Write-LauncherLog -Stage 'exit' -RunId $RunId -Data @{
    via  = 'claude'
    code = $claudeExit
    ms   = [int]((Get-Date) - $LaunchStartedAt).TotalMilliseconds
}
exit $claudeExit
