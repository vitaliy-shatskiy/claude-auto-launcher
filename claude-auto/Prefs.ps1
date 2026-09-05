# Remembered launch choices, one profile per account (file v2, 2026-09-04).
#
# Rows that describe a HABIT are remembered. Until 2026-09-04 there was ONE record for all four
# accounts, so every account switch re-picked model, effort and permission - the switch is itself
# the habit, and the shared record made it expensive. Now the account lives at the top level and
# the five habit rows live under it.
#
# Action is excluded: it describes one launch, not a habit. Safe mode is excluded because it
# disables CLAUDE.md, skills, plugins, hooks and MCP - a value that persisted silently would cripple
# later sessions in a way nobody would think to look for. 'stop server' is excluded because it is an
# action, not a state (remembered as 'off' - see Save-LaunchPrefs).
#
# Account is remembered too (owner reversed the exclusion 2026-08-11). It is still shown with a `*`
# and its age like any other restored row, so a stale choice is visible rather than silent - that
# visibility is what makes remembering it safe.
$script:ProfileFields = @('Model', 'Effort', 'Advisor', 'Permission', 'Remote')

function Get-LaunchPrefsPath {
    return (Join-Path $HOME '.claude\claude-auto-prefs.json')
}

function Get-PrefsAgeText {
    # Epoch milliseconds, not an ISO string: ConvertFrom-Json silently turns an ISO string into a
    # [datetime] and drops the Z, after which the age reads wrong by exactly the timezone offset.
    param($SavedAtMs, [long]$NowMs)
    if ($null -eq $SavedAtMs) { return '' }
    try { $span = [TimeSpan]::FromMilliseconds($NowMs - [long]$SavedAtMs) } catch { return '' }
    if ($span.TotalMinutes -lt 1) { return 'just now' }
    if ($span.TotalMinutes -lt 60) { return ('{0:N0} min' -f $span.TotalMinutes) }
    if ($span.TotalHours -lt 24) { return ('{0:N0} h' -f $span.TotalHours) }
    return ('{0:N0} d' -f $span.TotalDays)
}

function ConvertTo-PrefsTable {
    # A PSCustomObject from ConvertFrom-Json becomes a plain hashtable, so every reader below can use
    # ContainsKey and indexing without caring where the value came from.
    param($Object)
    $out = @{}
    if ($null -eq $Object) { return $out }
    foreach ($p in $Object.PSObject.Properties) { $out[$p.Name] = $p.Value }
    return $out
}

function Read-LaunchPrefs {
    # Always returns a v2 table: @{ Version; Account; SavedAtMs; Profiles = @{ <account> = @{...} } }.
    # Migration happens on the way IN, so nothing downstream ever sees the v1 shape.
    param([string]$Path = (Get-LaunchPrefsPath))
    $empty = @{ Version = 2; Profiles = @{} }
    if (-not (Test-Path -LiteralPath $Path)) { return $empty }
    try {
        $flat = ConvertTo-PrefsTable (Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json)
        if ($flat.Count -eq 0) { return $empty }
        # No Version key at all is the v1 (flat) file every launcher wrote before 2026-09-04.
        $version = if ($flat.ContainsKey('Version')) { [int]$flat['Version'] } else { 1 }
        # A version from the future is a file this code cannot understand. Reading it optimistically
        # would hand unknown values to the command line; it is ignored and rewritten on the next save.
        if ($version -gt 2) { return $empty }

        $out = @{ Version = 2; Profiles = @{} }
        if ($flat.ContainsKey('SavedAtMs')) { $out['SavedAtMs'] = [long]$flat['SavedAtMs'] }
        $account = if ($flat['Account']) { "$($flat['Account'])" } else { '' }

        # -ge, not -eq: a Version key at all means the nested shape, and the only thing that may
        # reject a future version is the gate above. With `-eq 2` a v3 file fell through to the v1
        # migration instead, which reads flat fields, finds none, and returns empty for the wrong
        # reason - so lifting the gate changed nothing and the gate was untestable (caught by
        # mutation, 2026-09-04).
        if ($version -ge 2) {
            if ($account) { $out['Account'] = $account }
            foreach ($p in (ConvertTo-PrefsTable $flat['Profiles']).GetEnumerator()) {
                $out['Profiles'][$p.Key] = ConvertTo-PrefsTable $p.Value
            }
            return $out
        }

        # v1: the flat fields belong to whichever account the file remembers. Attributing them to
        # 'work' instead would hand the owner's personal habits to the work account on the first
        # launch after the upgrade - silently, and on the row that decides what a session spends.
        # A file with no Account is older than 2026-08-11, when work was the only account there was.
        if (-not $account) { $account = 'work' }
        $out['Account'] = $account
        $entry = @{}
        foreach ($f in $script:ProfileFields) {
            if ($flat.ContainsKey($f) -and $null -ne $flat[$f]) { $entry[$f] = $flat[$f] }
        }
        if ($entry.Count -gt 0) {
            if ($flat.ContainsKey('SavedAtMs')) { $entry['SavedAtMs'] = [long]$flat['SavedAtMs'] }
            $out['Profiles'][$account] = $entry
        }
        return $out
    } catch { return $empty }   # a corrupt file is rewritten on the next save, never fatal
}

function Save-LaunchPrefs {
    # -NowMs is a parameter so the test does not depend on the clock.
    param(
        [Parameter(Mandatory)]$State,
        [string]$Path = (Get-LaunchPrefsPath),
        [long]$NowMs = ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()),
        $Rows = (Get-LaunchRows)
    )
    # The file is rebuilt from the previous one rather than from scratch, for two reasons. Up to four
    # launchers write it, so every profile that is not the launching one has to be copied forward
    # verbatim - last writer wins per PROFILE, not per file. And a row left at 'default' has to be
    # able to fall back to what THIS profile already remembered.
    $prev = Read-LaunchPrefs -Path $Path
    $account = if ($State.Account) { "$($State.Account)" } else { 'work' }
    $prevEntry = if ($prev.Profiles.ContainsKey($account)) { $prev.Profiles[$account] } else { @{} }

    $entry = @{ SavedAtMs = $NowMs }
    foreach ($f in $script:ProfileFields) {
        # A field with no row (Remote when the feature is off) is never written from the state: the
        # state still carries the seed value, and writing it would overwrite what a config WITH the
        # row remembered. Carry the previous answer forward instead - the same guard Merge has.
        if (-not ($Rows | Where-Object { $_.Name -eq $f })) {
            if ($prevEntry.ContainsKey($f)) { $entry[$f] = $prevEntry[$f] }
            continue
        }
        $v = $State.$f
        # 'stop server' is an action, not a state - but skipping it entirely meant the NEXT launch
        # silently reverted to remote 'on', which read as "my remote choice is not remembered"
        # (owner, 2026-08-11). Someone who just killed the server wants it to stay down: remember it
        # as 'off'.
        if ($f -eq 'Remote' -and $v -eq 'stop server') { $entry[$f] = 'off'; continue }
        # 'default' is the screen's "never chosen", not a choice - and ctrl+r puts it back on every
        # row of the active tab at once. Written verbatim it ERASED the remembered model, effort and
        # permission, and the launch after that came up bare: the owner read it as the settings
        # resetting themselves (2026-08-23, launcher-logs 12:57:02 - argv carried no state flags at
        # all). Saying nothing leaves the previous answer standing, and the previous answer is THIS
        # profile's - never another account's.
        if ($v -eq 'default') {
            # `-ne 'default'` drops a literal 'default' left in a file written before this rule:
            # inherited forever it would be restored and marked `*`, claiming a habit nobody had.
            if ($prevEntry.ContainsKey($f) -and $prevEntry[$f] -ne 'default') { $entry[$f] = $prevEntry[$f] }
            continue
        }
        if ($v) { $entry[$f] = $v }
    }

    $data = @{ Version = 2; Account = $account; SavedAtMs = $NowMs; Profiles = $prev.Profiles }
    $data.Profiles[$account] = $entry
    try {
        $dir = Split-Path -LiteralPath $Path
        if (-not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
        # Temp then Move-Item -Force: four launchers share this file, and a reader that catches a
        # half-written one would fall back to "nothing remembered" and come up bare. The temp name
        # carries the pid so two launchers writing at the same instant cannot share it.
        $tmp = "$Path.$PID-$([guid]::NewGuid().ToString('N').Substring(0, 6)).tmp"
        $data | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $tmp -Encoding utf8
        Move-Item -LiteralPath $tmp -Destination $Path -Force
    } catch { }   # never let a preferences write stop a session from starting
    # Returned so the launch log can record it. The prefs file is untracked by git and every launch
    # overwrites it, so without this line the state a launch remembered is unrecoverable the moment
    # the next one starts - which is what made the 2026-08-23 investigation guess.
    return $data
}

function Merge-LaunchPrefs {
    # Applies the remembered account's values onto a fresh state, and reports which ones were applied
    # so the screen can mark them. A value that is not one of the row's own options is IGNORED: the
    # prefs file is an ordinary text file, and an unvalidated model would otherwise reach the claude
    # command line. Only the remembered account's profile is applied - the others travel back
    # untouched in .Profiles, for the tab strip to load when the owner switches.
    param(
        [Parameter(Mandatory)]$State,
        [hashtable]$Prefs = @{},
        [Parameter(Mandatory)]$Rows,
        [long]$NowMs = ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
    )
    $profiles = if ($Prefs['Profiles']) { $Prefs['Profiles'] } else { @{} }
    $restored = @()
    # The account is validated like every other row, so a hand-edited profile name can never reach
    # Set-ClaudeProfile - and an account the config hides is simply not in the row's options, so it
    # falls back to the default rather than being resurrected.
    $accountRow = $Rows | Where-Object { $_.Name -eq 'Account' } | Select-Object -First 1
    if ($Prefs['Account'] -and $accountRow -and $Prefs['Account'] -in $accountRow.Values) {
        $State.Account = $Prefs['Account']
        $restored += 'Account'
    }
    $age = ''
    if ($profiles.ContainsKey($State.Account)) {
        $entry = $profiles[$State.Account]
        foreach ($f in $script:ProfileFields) {
            if ($null -eq $entry[$f]) { continue }
            $row = $Rows | Where-Object { $_.Name -eq $f } | Select-Object -First 1
            if (-not $row) { continue }
            if ($entry[$f] -notin $row.Values) { continue }
            $State.$f = $entry[$f]
            $restored += $f
        }
        $age = Get-PrefsAgeText -SavedAtMs $entry['SavedAtMs'] -NowMs $NowMs
    }
    if (-not $age) { $age = Get-PrefsAgeText -SavedAtMs $Prefs['SavedAtMs'] -NowMs $NowMs }
    # On the state as well as in the return value: the frame reads them from the state now, because
    # switching tabs recomputes both and a copy captured before the screen opened would be stale.
    $State.Restored = @($restored)
    $State.RestoredAge = $age
    return [pscustomobject]@{ State = $State; Restored = @($restored); AgeText = $age; Profiles = $profiles }
}

function Switch-LaunchAccount {
    # Moves the screen to another tab. The five habit rows are per account, so leaving a tab parks
    # its current answers in $State.Profiles and arriving at one loads them back: without the stash,
    # switching tabs merely to read another account's limits would silently discard the choices
    # already made on this one. $State.Account is the tab being LEFT - the caller restores it before
    # calling, precisely so the stash lands under the right account.
    #
    # Order of preference on arrival: this session's stash, then the file, then the defaults. The
    # stash wins because it is newer by definition, and it carries its own Restored marks so a tab
    # restored from the file still shows its `*` after a round trip.
    param(
        [Parameter(Mandatory)]$State,
        [Parameter(Mandatory)][string]$To,
        $Prefs = @{},
        [Parameter(Mandatory)]$Rows,
        [long]$NowMs = ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
    )
    $stash = @{ Restored = @($State.Restored); RestoredAge = "$($State.RestoredAge)" }
    foreach ($f in $script:ProfileFields) { $stash[$f] = $State.$f }
    $State.Profiles[$State.Account] = $stash

    $State.Account = $To
    $fresh = New-LaunchState
    foreach ($f in $script:ProfileFields) { $State.$f = $fresh.$f }
    $State.Restored = @()
    $State.RestoredAge = ''

    if ($State.Profiles.ContainsKey($To)) {
        $entry = $State.Profiles[$To]
        foreach ($f in $script:ProfileFields) { if ($null -ne $entry[$f]) { $State.$f = $entry[$f] } }
        $State.Restored = @($entry['Restored'])
        $State.RestoredAge = "$($entry['RestoredAge'])"
        return $State
    }

    $profiles = if ($Prefs -and $Prefs['Profiles']) { $Prefs['Profiles'] } else { @{} }
    if ($profiles.ContainsKey($To)) {
        $entry = $profiles[$To]
        $restored = @()
        foreach ($f in $script:ProfileFields) {
            if ($null -eq $entry[$f]) { continue }
            $row = $Rows | Where-Object { $_.Name -eq $f } | Select-Object -First 1
            if (-not $row) { continue }
            if ($entry[$f] -notin $row.Values) { continue }
            $State.$f = $entry[$f]
            $restored += $f
        }
        $State.Restored = @($restored)
        $State.RestoredAge = Get-PrefsAgeText -SavedAtMs $entry['SavedAtMs'] -NowMs $NowMs
    }
    return $State
}

function Reset-LaunchTab {
    # ctrl+r resets the ACTIVE tab only, and only for this launch - the file is untouched, as it
    # always was. Per tab because a stray reset must not erase another account's habit; the account
    # and the cursor row stay put because losing your place is not part of "reset the values".
    # Action and Mode are deliberately outside this: they are not habits, they already start at
    # their defaults, and resetting them would be undoing a choice made for THIS launch.
    param([Parameter(Mandatory)]$State)
    $fresh = New-LaunchState
    foreach ($f in $script:ProfileFields) { $State.$f = $fresh.$f }
    $State.Restored = @()
    $State.RestoredAge = ''
    if ($State.Profiles.ContainsKey($State.Account)) { $State.Profiles.Remove($State.Account) }
    return $State
}
