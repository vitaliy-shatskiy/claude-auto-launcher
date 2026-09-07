# Assertions for Prefs.ps1. Run: pwsh -NoProfile -File Test-Prefs.ps1
$env:CLAUDE_AUTO_CONFIG = "$PSScriptRoot\fixtures\config-four.json"   # BEFORE the dot-sources
try {
    . "$PSScriptRoot\..\claude-auto\Theme.ps1"
    . "$PSScriptRoot\..\claude-auto\Layout.ps1"
    . "$PSScriptRoot\..\claude-auto\Sessions.ps1"
    . "$PSScriptRoot\..\claude-auto\Screens.ps1"
    . "$PSScriptRoot\..\claude-auto\Prefs.ps1"
    . "$PSScriptRoot\..\claude-auto\Config.ps1"   # Test-Prefs does not load Env.ps1; Config must be explicit there
    Set-LaunchRoster -Accounts (Read-LauncherConfig).Accounts -Remote
} catch { Write-Host "COULD NOT RUN: $($_.Exception.Message)"; exit 2 }

$script:Failed = 0
$script:Ran = 0
function Assert-Equal {
    param($Expected, $Actual, [string]$Because)
    $script:Ran++
    if ("$Expected" -ne "$Actual") {
        Write-Host "FAIL  $Because"; Write-Host "      expected: $Expected"; Write-Host "      actual:   $Actual"
        $script:Failed++
    } else { Write-Host "ok    $Because" }
}

function New-PrefsPath { Join-Path $env:TEMP ('claude-auto-prefs-' + [guid]::NewGuid().ToString('N').Substring(0, 8) + '.json') }
# Key order in a hashtable is not part of the contract, so a profile is compared as a sorted
# key=value list rather than as JSON text: a comparison that can fail on ordering alone would be a
# flake, and a flake stops being read.
function Get-ProfileFingerprint { param($Entry) if (-not $Entry) { return '' }; (($Entry.Keys | Sort-Object | ForEach-Object { "$_=$($Entry[$_])" }) -join ';') }

$rows = Get-LaunchRows
$paths = @()

# --- v2 file shape --------------------------------------------------------------------------
# Prefs became per-account on 2026-09-04: switching accounts is the habit, and one shared record
# made every switch re-pick model, effort and permission. Only the tab that LAUNCHES is written;
# edits made on a tab the owner left are discarded, so there is one rule and no surprise writes.
$tmp = New-PrefsPath; $paths += $tmp
$s = New-LaunchState
$s.Model = 'sonnet1m'; $s.Effort = 'high'; $s.Advisor = 'opus'; $s.Permission = 'plan'; $s.Remote = 'off'
$s.Account = 'personal'; $s.Action = 'resume'; $s.Mode = 'safe'
$null = Save-LaunchPrefs -State $s -Path $tmp -NowMs 1000
$p = Read-LaunchPrefs -Path $tmp
Assert-Equal 2 $p.Version 'the file states its version'
Assert-Equal 'personal' $p.Account 'the launched account is remembered at the top level'
$e = $p.Profiles['personal']
Assert-Equal 'sonnet1m' $e.Model      'the model is remembered under that account (as its internal key)'
Assert-Equal 'high'   $e.Effort       'the effort is remembered'
Assert-Equal 'opus'   $e.Advisor      'the advisor choice is remembered'
Assert-Equal 'plan'   $e.Permission   'the permission mode is remembered'
Assert-Equal 'off'    $e.Remote       'the remote choice is remembered'
Assert-Equal 1000     $e.SavedAtMs    'the profile carries its own timestamp'
Assert-Equal $false   ($e.ContainsKey('Account')) 'the account is not repeated inside its own profile'
Assert-Equal $false   ($e.ContainsKey('Action'))  'the action is never remembered - it describes one launch'
Assert-Equal $false   ($e.ContainsKey('Mode'))    'safe mode is never remembered - it would cripple later sessions invisibly'
Assert-Equal $false   ($p.Profiles.ContainsKey('work')) 'an account that never launched has no profile at all'

# 'stop server' is an action, not a state - persisting it literally would kill the server on every
# launch. But dropping it entirely made the next launch revert to 'on', which the owner read as
# "remote is not remembered" (2026-08-11). It is remembered as 'off': the server stays down.
$s2 = New-LaunchState; $s2.Remote = 'stop server'
$null = Save-LaunchPrefs -State $s2 -Path $tmp -NowMs 2000
$p = Read-LaunchPrefs -Path $tmp
Assert-Equal 'off' $p.Profiles['work'].Remote 'stop server is remembered as off, never as stop server'
Assert-Equal 'sonnet1m' $p.Profiles['personal'].Model 'and saving one account leaves the other account''s profile standing'

# --- v1 -> v2 migration ---------------------------------------------------------------------
# The flat file every launcher wrote before 2026-09-04 belongs to whichever account it remembers.
# Attributing it to 'work' instead would hand the owner's personal habits to the work account on
# the first launch after the upgrade - silently, and on the row that decides what a session spends.
$v1 = New-PrefsPath; $paths += $v1
Set-Content -LiteralPath $v1 -Value '{ "Account": "personal", "Model": "opus1m", "Effort": "max", "SavedAtMs": 1 }' -Encoding utf8
$p = Read-LaunchPrefs -Path $v1
Assert-Equal 2 $p.Version 'a v1 file reads as v2 - migration happens on the way in, not on the way out'
Assert-Equal 'opus1m' $p.Profiles['personal'].Model 'the v1 fields become that account''s profile'
Assert-Equal 'max'    $p.Profiles['personal'].Effort 'every remembered field moves with it'
Assert-Equal 1        $p.Profiles['personal'].SavedAtMs 'and keeps the timestamp it was written with'
Assert-Equal $false   ($p.Profiles.ContainsKey('work')) 'no other account is invented by the migration'
# A v1 file with no Account at all is the pre-2026-08-11 shape: work was the only account it could
# have described, because the row was not remembered then.
$v1b = New-PrefsPath; $paths += $v1b
Set-Content -LiteralPath $v1b -Value '{ "Model": "haiku", "SavedAtMs": 5 }' -Encoding utf8
$p = Read-LaunchPrefs -Path $v1b
Assert-Equal 'haiku' $p.Profiles['work'].Model 'a v1 file with no account belongs to work'

# Saving after a migration must copy every other profile forward VERBATIM. Up to four launchers
# write this file; last writer wins per PROFILE, and a save that rebuilt the whole file from the
# launching state would silently drop the three accounts that were not on screen.
$before = Get-ProfileFingerprint (Read-LaunchPrefs -Path $v1).Profiles['personal']
$w = New-LaunchState; $w.Account = 'work'; $w.Model = 'fable'
$null = Save-LaunchPrefs -State $w -Path $v1 -NowMs 9000
$after = Read-LaunchPrefs -Path $v1
Assert-Equal $before (Get-ProfileFingerprint $after.Profiles['personal']) 'saving work leaves the personal profile byte-for-byte as it was'
Assert-Equal 'fable' $after.Profiles['work'].Model 'and writes the launched account''s own profile'
Assert-Equal 'work'  $after.Account 'the top-level account follows the launch'

# --- 'default' keeps the previous answer, per profile ----------------------------------------
# 'default' is what the screen shows when a row was never chosen, and what ctrl+r puts back on the
# active tab. Written verbatim it ERASED the remembered model, effort and permission, and the launch
# after that came up bare - which is how "the settings reset themselves" looked from the owner's
# chair on 2026-08-23 (launcher-logs 12:57:02, argv carried no state flags). Now per profile: a
# 'default' on the work tab may only fall back to WORK's previous answer, never to personal's.
$keep = New-PrefsPath; $paths += $keep
$k = New-LaunchState; $k.Account = 'work'; $k.Model = 'opus1m'; $k.Effort = 'high'; $k.Advisor = 'fable'; $k.Permission = 'auto'
$null = Save-LaunchPrefs -State $k -Path $keep -NowMs 1000
$k2 = New-LaunchState; $k2.Account = 'personal'; $k2.Model = 'haiku'
$null = Save-LaunchPrefs -State $k2 -Path $keep -NowMs 1500
$null = Save-LaunchPrefs -State (New-LaunchState) -Path $keep -NowMs 2000   # work again, everything default
$p = Read-LaunchPrefs -Path $keep
Assert-Equal 'opus1m' $p.Profiles['work'].Model      'a row left at default keeps THIS profile''s remembered model'
Assert-Equal 'high'   $p.Profiles['work'].Effort     'a row left at default keeps the remembered effort'
Assert-Equal 'fable'  $p.Profiles['work'].Advisor    'a row left at default keeps the remembered advisor'
Assert-Equal 'auto'   $p.Profiles['work'].Permission 'a row left at default keeps the remembered permission'
Assert-Equal 'haiku'  $p.Profiles['personal'].Model  'and never reaches across to another profile''s answer'
Assert-Equal 2000     $p.Profiles['work'].SavedAtMs  'the timestamp still advances - the file was written, not skipped'

# With nothing remembered yet, 'default' must not be written either: Merge-LaunchPrefs would restore
# it and mark the row `*`, which claims a habit that was never expressed.
$virgin = New-PrefsPath; $paths += $virgin
$null = Save-LaunchPrefs -State (New-LaunchState) -Path $virgin -NowMs 1000
$p = Read-LaunchPrefs -Path $virgin
Assert-Equal $false ($p.Profiles['work'].ContainsKey('Model'))      'default is not written when nothing was remembered'
Assert-Equal $false ($p.Profiles['work'].ContainsKey('Effort'))     'default effort is not written when nothing was remembered'
Assert-Equal $false ($p.Profiles['work'].ContainsKey('Advisor'))    'default advisor is not written when nothing was remembered'
Assert-Equal $false ($p.Profiles['work'].ContainsKey('Permission')) 'default permission is not written when nothing was remembered'
Assert-Equal 'work' $p.Account 'account has no default value, so it is always written'

# A file written before this rule existed can already hold the literal 'default'. Carrying it
# forward would let Merge-LaunchPrefs restore it and mark the row `*`, claiming a habit nobody ever
# expressed - so a legacy 'default' is dropped on the first save rather than inherited forever.
$legacy = New-PrefsPath; $paths += $legacy
Set-Content -LiteralPath $legacy -Value '{ "Model": "default", "Effort": "high", "SavedAtMs": 1 }' -Encoding utf8
$null = Save-LaunchPrefs -State (New-LaunchState) -Path $legacy -NowMs 2000
$p = Read-LaunchPrefs -Path $legacy
Assert-Equal $false ($p.Profiles['work'].ContainsKey('Model')) 'a legacy default in the file is dropped, not carried forward'
Assert-Equal 'high' $p.Profiles['work'].Effort 'a real remembered value beside it still survives'

# What was actually persisted is returned, so the launch log can record it. Without this the prefs
# file is the only record of what a launch remembered, it is untracked by git, and the next launch
# overwrites it - which is exactly why the 2026-08-23 investigation could not read the pre-incident
# state.
$written = Save-LaunchPrefs -State $k -Path $keep -NowMs 3000
Assert-Equal 'opus1m' $written.Profiles['work'].Model 'the saved values are returned for the launch log'
Assert-Equal 3000 $written.SavedAtMs 'the returned record carries the timestamp that was written'
Assert-Equal 'haiku' $written.Profiles['personal'].Model 'and the whole file, not just the launched profile'

# --- files that must never be trusted ---------------------------------------------------------
# A version from the future is a file this code cannot understand. Reading it optimistically would
# hand unknown values to the command line; it is ignored and rewritten on the next save instead.
$v3 = New-PrefsPath; $paths += $v3
Set-Content -LiteralPath $v3 -Value '{ "Version": 3, "Profiles": { "work": { "Model": "opus1m" } } }' -Encoding utf8
$p = Read-LaunchPrefs -Path $v3
Assert-Equal 0 $p.Profiles.Count 'a file from a newer version is ignored, not half-read'
Assert-Equal 2 $p.Version 'and reads as an empty v2 table'
Assert-Equal 0 (Read-LaunchPrefs -Path (Join-Path $env:TEMP 'claude-auto-prefs-does-not-exist.json')).Profiles.Count 'a missing prefs file reads as empty'
$bad = New-PrefsPath; $paths += $bad
Set-Content -LiteralPath $bad -Value '{ this is not json' -Encoding utf8
Assert-Equal 0 (Read-LaunchPrefs -Path $bad).Profiles.Count 'a corrupt prefs file reads as empty rather than throwing'

# Four launchers, one file. Each re-reads before writing, so two saves of two different accounts
# both survive - last writer wins per PROFILE, not per file.
$race = New-PrefsPath; $paths += $race
$a = New-LaunchState; $a.Account = 'work';     $a.Model = 'fable'
$b = New-LaunchState; $b.Account = 'personal'; $b.Model = 'haiku'
$null = Save-LaunchPrefs -State $a -Path $race -NowMs 1000
$null = Save-LaunchPrefs -State $b -Path $race -NowMs 1001
$p = Read-LaunchPrefs -Path $race
Assert-Equal 'fable' $p.Profiles['work'].Model     'a second launcher''s save keeps the first launcher''s profile'
Assert-Equal 'haiku' $p.Profiles['personal'].Model 'and adds its own'
Assert-Equal 'personal' $p.Account 'the top-level account is the one that launched last'

# --- restoring ---------------------------------------------------------------------------------
$file = @{ Version = 2; Account = 'work'; SavedAtMs = 1000
           Profiles = @{ work     = @{ Model = 'opus1m'; Effort = 'low'; SavedAtMs = 1000 }
                         personal = @{ Model = 'haiku'; SavedAtMs = 1000 } } }
$r = Merge-LaunchPrefs -State (New-LaunchState) -Prefs $file -Rows $rows
Assert-Equal 'opus1m' $r.State.Model 'a remembered model is restored'
Assert-Equal 'low'  $r.State.Effort 'a remembered effort is restored'
Assert-Equal 'work' $r.State.Account 'the remembered account is the tab that opens'
Assert-Equal 'default' $r.State.Advisor 'another profile''s rows are NOT applied to this one'
Assert-Equal 3 (@($r.Restored)).Count 'the restored fields are reported so the screen can mark them'
Assert-Equal $true ('Account' -in @($r.Restored)) 'the account counts as restored - a stale one must be visible, not silent'
Assert-Equal 3 (@($r.State.Restored)).Count 'and are on the state too, where the frame now reads them'
Assert-Equal $true ($r.Profiles.ContainsKey('personal')) 'the whole profile table travels back for the tab strip'

$acc = Merge-LaunchPrefs -State (New-LaunchState) -Prefs @{ Version = 2; Account = 'personal'; Profiles = @{} } -Rows $rows
Assert-Equal 'personal' $acc.State.Account 'a remembered account with no profile still opens its tab'
# 'shared' is hidden from the row since 2026-09-02 (inactive account): a prefs file that still
# remembers it must fall back to work, not resurrect a value the screen no longer offers.
$shared = Merge-LaunchPrefs -State (New-LaunchState) -Prefs @{ Version = 2; Account = 'shared'; Profiles = @{ shared = @{ Model = 'haiku' } } } -Rows $rows
Assert-Equal 'work' $shared.State.Account 'a remembered but hidden account (shared) falls back to work'
Assert-Equal 'default' $shared.State.Model 'and the hidden account''s profile is not applied to work'
$low = Merge-LaunchPrefs -State (New-LaunchState) -Prefs @{ Version = 2; Account = 'low'; Profiles = @{} } -Rows $rows
Assert-Equal 'low' $low.State.Account 'the fourth account (low) is restored like the rest'
$badAcc = Merge-LaunchPrefs -State (New-LaunchState) -Prefs @{ Version = 2; Account = 'root'; Profiles = @{} } -Rows $rows
Assert-Equal 'work' $badAcc.State.Account 'an account outside the row options is ignored, not trusted'
# The same two fallbacks on a v1-shaped record (no Version): a machine whose config hides or drops
# an account must still open on the first visible one, whatever the file remembers.
$m = Merge-LaunchPrefs -State (New-LaunchState) -Prefs @{ Account = 'shared'; Profiles = @{} } -Rows (Get-LaunchRows)
Assert-Equal 'work' $m.State.Account 'a remembered hidden account falls back to the first visible one'
$m = Merge-LaunchPrefs -State (New-LaunchState) -Prefs @{ Account = 'ghost'; Profiles = @{} } -Rows (Get-LaunchRows)
Assert-Equal 'work' $m.State.Account 'a remembered unknown account falls back too'

# --- a field whose row is absent is never written. Remote off in the config = no Remote row; the
# state still carries Remote = 'on' from New-LaunchState, and writing it would (a) put a key in the
# file the screen cannot show and (b) overwrite a remembered 'off' from a config where the row was
# on. Save must mirror the guard Merge already has. ---
$tmpNoRow = New-PrefsPath; $paths += $tmpNoRow
$sNoRow = New-LaunchState; $sNoRow.Account = 'work'; $sNoRow.Remote = 'off'; $sNoRow.Model = 'haiku'
$null = Save-LaunchPrefs -State $sNoRow -Path $tmpNoRow -NowMs 1000
Set-LaunchRoster -Accounts (Read-LauncherConfig).Accounts        # NO -Remote: the row is gone
try {
    $sNoRow2 = New-LaunchState; $sNoRow2.Account = 'work'; $sNoRow2.Effort = 'high'
    $null = Save-LaunchPrefs -State $sNoRow2 -Path $tmpNoRow -NowMs 2000
    $back = Read-LaunchPrefs -Path $tmpNoRow
    Assert-Equal 'high' $back.Profiles['work'].Effort 'the save without a Remote row still writes the rows it has'
    Assert-Equal 'off'  $back.Profiles['work'].Remote 'a remembered Remote survives a save made with no Remote row'
    Assert-Equal 'haiku' $back.Profiles['work'].Model 'and the other remembered rows carry forward as before'
    # With nothing remembered, the absent row leaves NO key: the state's seed value never reaches the file.
    $tmpFresh = New-PrefsPath; $paths += $tmpFresh
    $null = Save-LaunchPrefs -State $sNoRow2 -Path $tmpFresh -NowMs 3000
    $fresh = Read-LaunchPrefs -Path $tmpFresh
    Assert-Equal $false $fresh.Profiles['work'].ContainsKey('Remote') 'no Remote row and nothing remembered: no Remote key is written'
} finally { Set-LaunchRoster -Accounts (Read-LauncherConfig).Accounts -Remote }
$rows = Get-LaunchRows
Assert-Equal 'new'  $r.State.Action  'the action still starts at its default'

# A value that is not one of the row's own options must be ignored, not passed to claude. A prefs
# file is an ordinary text file and an invalid model would otherwise reach the command line.
$junk = Merge-LaunchPrefs -State (New-LaunchState) -Prefs @{ Version = 2; Account = 'work'; Profiles = @{ work = @{ Model = 'gpt-4'; SavedAtMs = 1000 } } } -Rows $rows
Assert-Equal 'default' $junk.State.Model 'an unknown value is ignored rather than trusted'
Assert-Equal 1 (@($junk.Restored)).Count 'an ignored value is not reported as restored (only the account is)'

# The age is rendered from epoch milliseconds. ConvertFrom-Json turns an ISO string into a
# [datetime] and drops the Z, after which the age is wrong by exactly the timezone offset - so the
# timestamp is written as a number and this asserts it survives the round trip.
$null = Save-LaunchPrefs -State $s -Path $tmp -NowMs 5000
$p = Read-LaunchPrefs -Path $tmp
Assert-Equal 5000 $p.Profiles['personal'].SavedAtMs 'the timestamp round-trips as epoch milliseconds'
$aged = Merge-LaunchPrefs -State (New-LaunchState) -Prefs $file -Rows $rows -NowMs 3601000
Assert-Equal '1 h' $aged.AgeText 'the age of the remembered choices is available to the screen'
Assert-Equal '1 h' $aged.State.RestoredAge 'and on the state, beside the rows it explains'

# --- switching tabs ---------------------------------------------------------------------------
# The screen keeps its own stash of the five rows per account. Switching away parks the current
# answers under the account being left, switching back brings them home: without that, changing tabs
# to read another account's limits would silently discard the choices already made on this one.
$st = Merge-LaunchPrefs -State (New-LaunchState) -Prefs $file -Rows $rows -NowMs 2000
$st = $st.State
$st.Effort = 'max'                                     # an edit on the work tab
$st = Switch-LaunchAccount -State $st -To 'personal' -Prefs $file -Rows $rows -NowMs 2000
Assert-Equal 'personal' $st.Account 'the switch lands on the new account'
Assert-Equal 'haiku'   $st.Model    'and loads that account''s remembered model from the file'
Assert-Equal 'default' $st.Effort   'a row the new account never chose comes up at its default'
Assert-Equal $true ('Model' -in @($st.Restored)) 'rows restored from the file are marked on the new tab'
$st = Switch-LaunchAccount -State $st -To 'work' -Prefs $file -Rows $rows -NowMs 2000
Assert-Equal 'max'    $st.Effort 'coming back restores the edit made before leaving, not the file value'
Assert-Equal 'opus1m' $st.Model  'and the rest of that tab with it'
$st = Switch-LaunchAccount -State $st -To 'low' -Prefs $file -Rows $rows -NowMs 2000
Assert-Equal 'default' $st.Model 'an account with neither a stash nor a file entry starts clean'
Assert-Equal 0 (@($st.Restored)).Count 'and nothing is marked as restored on it'
Assert-Equal '' "$($st.RestoredAge)" 'and it carries no age'

# ctrl+r resets the ACTIVE tab only, for this launch only (the file is untouched, as it always was).
# A stray reset must not erase another account's habit - that is the whole reason it is per tab.
$st = Switch-LaunchAccount -State $st -To 'personal' -Prefs $file -Rows $rows -NowMs 2000
$st.Effort = 'high'
$st = Reset-LaunchTab -State $st
Assert-Equal 'personal' $st.Account 'reset leaves the account alone'
Assert-Equal 'default'  $st.Effort  'reset puts the active tab''s rows back to their defaults'
Assert-Equal 'default'  $st.Model   'including the ones restored from the file'
Assert-Equal 0 (@($st.Restored)).Count 'and clears the restored marks with them'
Assert-Equal $false ($st.Profiles.ContainsKey('personal')) 'the active tab''s stash is dropped, so nothing resurrects it'
Assert-Equal $true  ($st.Profiles.ContainsKey('work')) 'another tab''s stash survives a reset'
Assert-Equal 'max' $st.Profiles['work'].Effort 'with the edit it was holding'

# --- CLAUDE_AUTO_PREFS override (deferred review finding) -------------------------------------
# The test harness must never read or write the owner's real ~/.claude/claude-auto-prefs.json:
# Get-LaunchPrefsPath honours this variable, same shape as Get-LauncherConfigPath's
# CLAUDE_AUTO_CONFIG, so the preview driver can point the whole read/save path at a throwaway file.
$savedAutoPrefs = $env:CLAUDE_AUTO_PREFS
try {
    $env:CLAUDE_AUTO_PREFS = Join-Path $env:TEMP 'claude-auto-prefs-envtest.json'
    Assert-Equal $env:CLAUDE_AUTO_PREFS (Get-LaunchPrefsPath) 'CLAUDE_AUTO_PREFS overrides the default path'
    $env:CLAUDE_AUTO_PREFS = '~\prefs-tilde-test.json'
    Assert-Equal (Join-Path $HOME 'prefs-tilde-test.json') (Get-LaunchPrefsPath) 'a ~-rooted CLAUDE_AUTO_PREFS expands against $HOME'
} finally {
    if ($null -eq $savedAutoPrefs) { Remove-Item Env:CLAUDE_AUTO_PREFS -ErrorAction SilentlyContinue }
    else { $env:CLAUDE_AUTO_PREFS = $savedAutoPrefs }
}
Assert-Equal (Join-Path $HOME '.claude\claude-auto-prefs.json') (Get-LaunchPrefsPath) 'with no override, the default path is unchanged'

# A full preview run must never touch the real prefs path - proven against a FAKE home (never the
# owner's own $HOME) so this assertion cannot itself do the damage it is checking for. $HOME is
# fixed at process startup, so overriding $env:USERPROFILE here (this process already has its
# $HOME) has no effect on THIS process, only on the fresh child processes preview.ps1 spawns -
# which is exactly what is needed: the "default path" the launcher would use if CLAUDE_AUTO_PREFS
# were not set resolves under the fake home, never under the real one.
$fakeHome = Join-Path $env:TEMP ('claude-auto-fakehome-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force -Path $fakeHome | Out-Null
$savedUserProfile = $env:USERPROFILE
try {
    $fakeRealPrefs = Join-Path $fakeHome '.claude\claude-auto-prefs.json'
    Assert-Equal $false (Test-Path -LiteralPath $fakeRealPrefs) 'before the probe: the fake-home default prefs path has no file'
    $env:USERPROFILE = $fakeHome
    $null = & pwsh -NoProfile -File "$PSScriptRoot\preview.ps1" -Keys 'Enter' -Launcher "$PSScriptRoot\..\claude-auto.ps1" 2>&1
    Assert-Equal $false (Test-Path -LiteralPath $fakeRealPrefs) 'after a preview run: the default-path prefs file is still not there - the run wrote its throwaway file instead'
} finally {
    $env:USERPROFILE = $savedUserProfile
    Remove-Item -LiteralPath $fakeHome -Recurse -Force -ErrorAction SilentlyContinue
}

foreach ($f in $paths) { Remove-Item -LiteralPath $f -Force -ErrorAction SilentlyContinue }
Remove-Item Env:CLAUDE_AUTO_CONFIG -ErrorAction SilentlyContinue
if ($script:Ran -ne 92) { Write-Host "COULD NOT RUN: expected 92 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
