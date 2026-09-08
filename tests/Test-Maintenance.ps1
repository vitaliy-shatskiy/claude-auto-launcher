# Assertions for Maintenance.ps1. Run: pwsh -NoProfile -File Test-Maintenance.ps1
# The filesystem is injected, so nothing here touches the real installation.
try {
    . "$PSScriptRoot\..\claude-auto\Theme.ps1"
    . "$PSScriptRoot\..\claude-auto\Layout.ps1"
    . "$PSScriptRoot\..\claude-auto\Screens.ps1"
    . "$PSScriptRoot\..\claude-auto\Input.ps1"   # Invoke-MaintenanceScreen maps a footer click through Get-ClaudeFooterHit
    . "$PSScriptRoot\..\claude-auto\Ui.ps1"      # ConvertTo-StatusText, used by Invoke-MaintenanceScript to reduce output
    . "$PSScriptRoot\..\claude-auto\Maintenance.ps1"
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

# Fixed-length synthetic path, not (Join-Path $HOME '.local\bin\claude.exe'): several assertions
# below render at the MINIMUM supported width and check that content fits without truncation - a
# fixture built from the real $HOME would make that margin depend on how long this machine's user
# name happens to be, which has nothing to do with the code under test.
$script:FixtureBinPath = 'C:\Users\sample-user\.local\bin\claude.exe'

$tmp = Join-Path $env:TEMP ('claude-auto-maint-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
$versions = Join-Path $tmp 'versions'
New-Item -ItemType Directory -Force -Path $versions | Out-Null
Set-Content -Path (Join-Path $versions '2.1.226') -Value 'old build' -NoNewline
Set-Content -Path (Join-Path $versions '2.1.230') -Value 'new build' -NoNewline
$bin = Join-Path $tmp 'claude.exe'
Set-Content -Path $bin -Value 'old build' -NoNewline

# The whole reason this module exists: the answer comes from a hash, never from an updater message.
$info = Get-ClaudeInstallInfo -BinPath $bin -VersionsDir $versions
Assert-Equal '2.1.230' $info.NewestVersion 'the newest version directory is found'
Assert-Equal $false    $info.Matches       'an installed binary that is the old build does not match'
Assert-Equal 2         $info.VersionCount  'every downloaded build is counted'

Set-Content -Path $bin -Value 'new build' -NoNewline
$info = Get-ClaudeInstallInfo -BinPath $bin -VersionsDir $versions
Assert-Equal $true $info.Matches 'an installed binary equal to the newest build matches'

# --- Ordering: Get-ClaudeInstallInfo and Repair-ClaudeBinaryByRename must agree with
# Remove-OldClaudeVersions on what "newest" means - all three are driven by the one shared helper,
# Get-OrderedClaudeBuilds, so these prove the functions share a rule rather than each keeping their
# own copy that can drift apart (which is how the LastWriteTime bug happened in the first place). ---
$ordDir = Join-Path $tmp 'order-versions'
New-Item -ItemType Directory -Force -Path $ordDir | Out-Null

# The oldest-by-version build is written LAST on purpose - giving it the newest LastWriteTime -
# to prove NewestVersion is decided by version, not by write time. Do not "fix" this write order;
# it is the point of the test.
Set-Content -Path (Join-Path $ordDir '2.1.230') -Value 'build 230' -NoNewline
Set-Content -Path (Join-Path $ordDir '2.1.226') -Value 'build 226' -NoNewline
$ordInfo = Get-ClaudeInstallInfo -BinPath (Join-Path $tmp 'no-such-binary-2.exe') -VersionsDir $ordDir
Assert-Equal '2.1.230' $ordInfo.NewestVersion 'NewestVersion is the highest version, even though the older build was written last'

# The rename swap must install that same highest version - assert on the resulting file CONTENT,
# not the returned message: a wrong message reporting a successful swap is exactly what was wrong
# before this fix (a silent downgrade whose hash check still passed).
$ordBin = Join-Path $tmp 'order-bin.exe'
Set-Content -Path $ordBin -Value 'installed old' -NoNewline
$null = Repair-ClaudeBinaryByRename -BinPath $ordBin -VersionsDir $ordDir
Assert-Equal 'build 230' (Get-Content -LiteralPath $ordBin -Raw) 'the rename swap installs the highest version, not the most recently written one'

Remove-Item -Recurse -Force $ordDir

# A tie in LastWriteTime must no longer decide anything: two builds forced onto the identical
# timestamp must still order by version.
$tieDir = Join-Path $tmp 'tie-versions'
New-Item -ItemType Directory -Force -Path $tieDir | Out-Null
Set-Content -Path (Join-Path $tieDir '2.1.100') -Value 'build 100' -NoNewline
Set-Content -Path (Join-Path $tieDir '2.1.900') -Value 'build 900' -NoNewline
$tieStamp = Get-Date
(Get-Item (Join-Path $tieDir '2.1.100')).LastWriteTime = $tieStamp
(Get-Item (Join-Path $tieDir '2.1.900')).LastWriteTime = $tieStamp
$tieInfo = Get-ClaudeInstallInfo -BinPath (Join-Path $tmp 'no-such-binary-3.exe') -VersionsDir $tieDir
Assert-Equal '2.1.900' $tieInfo.NewestVersion 'a LastWriteTime tie is broken by version, not left to enumeration order'
Remove-Item -Recurse -Force $tieDir

# --- Pruning: an isolated fixture tree, so its own churn cannot shift the LastWriteTime-based
# "newest build" that Get-ClaudeInstallInfo and Repair-ClaudeBinaryByRename rely on further down. ---
$pruneDir = Join-Path $tmp 'prune-versions'
New-Item -ItemType Directory -Force -Path $pruneDir | Out-Null
# The installed binary is INSIDE the keep window here, so the hash exemption is a no-op and the
# keep count alone decides. It still has to exist: prune fails closed when it cannot identify the
# running build, because "no binary" is also the state during a swap, and deleting then is how the
# running build disappeared.
$keptBin = Join-Path $tmp 'installed-newest.exe'

# Pruning orders by parsed VERSION, not write time. This fixture deliberately writes '2.1.200'
# LAST - giving it the newest LastWriteTime even though it is the oldest version - to prove the
# sort is by version, not by timestamp. Do not "fix" this write order; it is the point of the test.
Set-Content -Path (Join-Path $pruneDir '2.1.226') -Value 'build 226' -NoNewline
Set-Content -Path (Join-Path $pruneDir '2.1.230') -Value 'build 230' -NoNewline
Set-Content -Path (Join-Path $pruneDir '2.1.200') -Value 'ancient'   -NoNewline
Copy-Item -LiteralPath (Join-Path $pruneDir '2.1.230') -Destination $keptBin -Force
$r = Remove-OldClaudeVersions -VersionsDir $pruneDir -Keep 2 -BinPath $keptBin
Assert-Equal 1 $r.Deleted.Count 'pruning deletes only what is beyond the keep count'
Assert-Equal $false (Test-Path (Join-Path $pruneDir '2.1.200')) 'the oldest-by-version build is gone, despite having the newest write time'
Assert-Equal $true  (Test-Path (Join-Path $pruneDir '2.1.230')) 'the newest-by-version build survives'
Assert-Equal $true  (Test-Path (Join-Path $pruneDir '2.1.226')) 'the second-newest build survives, within the keep window'

# The installed build is the only one that cannot be re-obtained by rolling back, so pruning must
# never delete it, even when it falls outside the keep window.
Remove-Item -Recurse -Force $pruneDir
New-Item -ItemType Directory -Force -Path $pruneDir | Out-Null
Set-Content -Path (Join-Path $pruneDir '2.1.226') -Value 'build 226' -NoNewline
Set-Content -Path (Join-Path $pruneDir '2.1.230') -Value 'build 230' -NoNewline
Set-Content -Path (Join-Path $pruneDir '2.1.200') -Value 'ancient'   -NoNewline
$installedBin = Join-Path $tmp 'installed-old.exe'
Copy-Item -LiteralPath (Join-Path $pruneDir '2.1.200') -Destination $installedBin
$r = Remove-OldClaudeVersions -VersionsDir $pruneDir -Keep 1 -BinPath $installedBin
Assert-Equal $true  (Test-Path (Join-Path $pruneDir '2.1.200')) 'an installed build outside the keep window survives pruning'
Assert-Equal $false ($r.Deleted -contains '2.1.200') 'the installed build is not reported as deleted'

# A directory written by the updater, not by us, may contain a name that does not parse as a
# version - it must not throw, and must not be mistaken for the newest build.
Remove-Item -Recurse -Force $pruneDir
New-Item -ItemType Directory -Force -Path $pruneDir | Out-Null
Set-Content -Path (Join-Path $pruneDir '2.1.226') -Value 'build 226' -NoNewline
Set-Content -Path (Join-Path $pruneDir '2.1.230') -Value 'build 230' -NoNewline
Set-Content -Path (Join-Path $pruneDir 'nightly')  -Value 'unparseable' -NoNewline
Copy-Item -LiteralPath (Join-Path $pruneDir '2.1.230') -Destination $keptBin -Force
$threw = $false
try { $r = Remove-OldClaudeVersions -VersionsDir $pruneDir -Keep 2 -BinPath $keptBin }
catch { $threw = $true }
Assert-Equal $false $threw 'an unparseable build name does not throw'
Assert-Equal $true  (Test-Path (Join-Path $pruneDir '2.1.230')) 'the real newest build still survives alongside an unparseable name'
Assert-Equal $true  (Test-Path (Join-Path $pruneDir '2.1.226')) 'the real second-newest build also survives - the unparseable name was not treated as newest'
Assert-Equal $true  (Test-Path (Join-Path $pruneDir 'nightly')) 'and the unparseable name itself is left alone - it is not a build to prune'

Remove-Item -Recurse -Force $pruneDir

# The rename swap is the riskiest function here - it renames a binary and copies over it - and it
# is entirely testable against the fake tree, so it gets asserted rather than trusted.
Set-Content -Path $bin -Value 'old build' -NoNewline
$r = Repair-ClaudeBinaryByRename -BinPath $bin -VersionsDir $versions
Assert-Equal $true  $r.Ok 'the rename swap reports success'
Assert-Equal 'new build' (Get-Content -LiteralPath $bin -Raw) 'the installed binary is now the new build'
Assert-Equal $true  (Test-Path "$bin.old") 'the previous binary is kept as .old, not deleted'
Assert-Equal 'old build' (Get-Content -LiteralPath "$bin.old" -Raw) 'the .old copy is the build that was replaced'
$r = Repair-ClaudeBinaryByRename -BinPath $bin -VersionsDir $versions
Assert-Equal $true $r.Ok 'a second swap on an already-current binary is a no-op that still reports success'
Assert-Equal 'new build' (Get-Content -LiteralPath $bin -Raw) 'the no-op swap did not damage the binary'

# A missing versions directory must not throw - it is the state of a fresh install.
$r = Get-ClaudeInstallInfo -BinPath $bin -VersionsDir (Join-Path $tmp 'does-not-exist')
Assert-Equal '' "$($r.NewestVersion)" 'no downloaded builds yields no newest version'
Assert-Equal $false $r.Matches 'nothing to compare against is not a match'

# The frame renders the mismatch as a sentence, not as a boolean.
$info = Get-ClaudeInstallInfo -BinPath $bin -VersionsDir $versions
$f = Get-MaintenanceFrame -Info $info -Width 80 -Height 24
Assert-Equal 0 (@($f | Where-Object { $_.Length -gt 80 }).Count) 'no maintenance line exceeds the width'
Assert-Equal 1 (@($f | Where-Object { $_ -match '2\.1\.230' }).Count) 'the newest version is shown'

# Fix 2: the width check above only proves no line is OVER width - Get-MaintenanceFrame pipes every
# line through Limit-Line before returning, so a genuinely too-long value would come back truncated
# with an ellipsis and that assertion would still pass. Real SHA-256 hashes (64 hex chars, the frame
# shows a 16-char prefix) at the minimum supported width, 60, prove the content actually fits rather
# than being silently cut. Rendered and inspected by hand first (60 columns, real hashes): the
# 16-char hash prefix, the version string and the "N builds, X.Y GB" figure all come back complete -
# the label columns (14 chars) plus this content stay well inside the 58-char inner box width, so no
# narrower fallback exists or is needed here.
$hash60a = 'a1' * 32
$hash60b = 'b2' * 32
$wideInfo = [pscustomobject]@{
    BinPath = $script:FixtureBinPath; InstalledHash = $hash60a
    NewestVersion = '2.1.230'; NewestHash = $hash60b; Matches = $false
    VersionCount = 12; VersionsBytes = 3650722201
}
$expectedPrefix = $hash60a.Substring(0, 16)
$expectedGb = '{0:N1}' -f ($wideInfo.VersionsBytes / 1GB)
# $script:MinHeight, not a literal 20: the minimum is measured from the LAUNCH frame (Screens.ps1)
# and moved to 21 on 2026-09-04, at which point a hardcoded 20 stopped rendering this screen at all
# and started asserting against the too-small notice.
$f60 = Get-MaintenanceFrame -Info $wideInfo -Width 60 -Height $script:MinHeight
Assert-Equal 0 (@($f60 | Where-Object { $_.Length -gt 60 }).Count) 'no maintenance line exceeds width 60'
Assert-Equal 1 (@($f60 | Where-Object { $_ -match [regex]::Escape($expectedPrefix) }).Count) 'width 60: the 16-char hash prefix renders complete, not truncated'
Assert-Equal 1 (@($f60 | Where-Object { $_ -match [regex]::Escape($wideInfo.NewestVersion) }).Count) 'width 60: the newest version string renders complete, not truncated'
Assert-Equal 1 (@($f60 | Where-Object { $_ -match [regex]::Escape("$($wideInfo.VersionCount) builds, $expectedGb GB") }).Count) 'width 60: the "versions ... GB" figure renders complete, not truncated'

# Fix 2: Get-MaintenanceFrame must not throw on a hash shorter than the 16-char prefix it slices,
# or on no hash at all. Real SHA-256 hashes are 64 chars so this never fires in practice - but the
# maintenance screen is exactly the one someone opens when something is already wrong with their
# install, so a throw here is the worst possible failure mode.
function Assert-MaintenanceFrameRenders {
    param($Info, [string]$Because)
    $threw = $false
    $frame = @()
    try { $frame = @(Get-MaintenanceFrame -Info $Info -Width 78 -Height 24) }
    catch { $threw = $true }
    Assert-Equal $false $threw "$Because : does not throw"
    $hasException = @($frame | Where-Object { $_ -match 'Exception|error' }).Count -gt 0
    Assert-Equal $false $hasException "$Because : no exception text in the rendered frame"
}

$fullHashInfo = [pscustomobject]@{
    BinPath = $script:FixtureBinPath; InstalledHash = ('a1' * 32)
    NewestVersion = '2.1.230'; NewestHash = ('b2' * 32); Matches = $false
    VersionCount = 3; VersionsBytes = 900000000
}
Assert-MaintenanceFrameRenders -Info $fullHashInfo -Because 'a full 64-char hash'

$shortHashInfo = [pscustomobject]@{
    BinPath = $script:FixtureBinPath; InstalledHash = 'ab12c'
    NewestVersion = '2.1.230'; NewestHash = 'de34f'; Matches = $false
    VersionCount = 3; VersionsBytes = 900000000
}
Assert-MaintenanceFrameRenders -Info $shortHashInfo -Because 'a 5-char hash'

$nullHashInfo = [pscustomobject]@{
    BinPath = $script:FixtureBinPath; InstalledHash = $null
    NewestVersion = $null; NewestHash = $null; Matches = $false
    VersionCount = 0; VersionsBytes = 0
}
Assert-MaintenanceFrameRenders -Info $nullHashInfo -Because 'a null hash'

# --- the status region ---------------------------------------------------------------------
# The defect: a multi-line status went into the box as ONE row. Its newlines then split that row in
# the middle of the frame, and Limit-Line measured the whole blob as a single 78-column line and
# threw the rest away. Assert on CONTENT - "no line exceeds the width" is satisfied by construction
# here, because every row is piped through Limit-Line before the frame is returned.
$statusInfo = [pscustomobject]@{
    BinPath = $script:FixtureBinPath; InstalledHash = ('a1' * 32)
    NewestVersion = '2.1.230'; NewestHash = ('b2' * 32); Matches = $false
    VersionCount = 3; VersionsBytes = 900000000
}
$doctorText = "Running: native (2.1.229)`nPlatform: win32-x64`n`nNo installation issues found."
$fs = @(Get-MaintenanceFrame -Info $statusInfo -Width 78 -Height 24 -Status $doctorText)
Assert-Equal 0 (@($fs | Where-Object { $_ -match "`n" }).Count) 'no rendered row contains a newline'
Assert-Equal 1 (@($fs | Where-Object { $_ -match 'Running: native' }).Count) 'the first status line reaches the screen'
Assert-Equal 1 (@($fs | Where-Object { $_ -match 'Platform: win32-x64' }).Count) 'the middle status line is not swallowed by the first'
Assert-Equal 1 (@($fs | Where-Object { $_ -match 'No installation issues found' }).Count) 'the last status line reaches the screen'

# A one-line status longer than the box must WRAP, not lose its second half: losing it is exactly
# how the update message's own instruction became invisible.
$oneLiner = 'the updater could not overwrite the running claude.exe, so it was installed by rename: now 2.1.230, verified by hash'
$fw = @(Get-MaintenanceFrame -Info $statusInfo -Width 78 -Height 24 -Status $oneLiner)
Assert-Equal 1 (@($fw | Where-Object { $_ -match 'verified by hash' }).Count) 'the tail of a long status wraps onto another row instead of being truncated away'

# A report taller than the terminal must leave one row spare, not merely fit. Write-Frame appends a
# newline after EVERY line including the last, so a frame that fills the screen exactly scrolls the
# alternate buffer by one row: the top border leaves the screen and the next repaint's cursor-home
# lands a row off. `-le $Height` would pass on the version that did exactly that.
$longText = (1..40 | ForEach-Object { "status line $_" }) -join "`n"
foreach ($h in 16, 24, 50) {
    $fh = @(Get-MaintenanceFrame -Info $statusInfo -Width 78 -Height $h -Status $longText)
    Assert-Equal $true ($fh.Count -le ($h - 1)) "a status taller than the screen leaves a spare row at height $h"
}
$fl = @(Get-MaintenanceFrame -Info $statusInfo -Width 78 -Height 24 -Status $longText)
Assert-Equal 1 (@($fl | Where-Object { $_ -match 'status line 1\b' }).Count) 'the cap keeps the head of the report'
Assert-Equal 0 (@($fl | Where-Object { $_ -match 'status line 40\b' }).Count) 'the tail beyond the cap is not shown'
Assert-Equal 1 (@($fl | Where-Object { $_ -match 'more lines' }).Count) 'the reader is told the report was cut'

# Both verdict wordings must survive the MINIMUM width - the old ones did not, and the advice was
# the half that got cut off.
$verdictInfo = [pscustomobject]@{
    BinPath = $script:FixtureBinPath; InstalledHash = ('a1' * 32)
    NewestVersion = '2.1.230'; NewestHash = ('b2' * 32); Matches = $false
    VersionCount = 3; VersionsBytes = 900000000
}
# 50 is the minimum since 2026-09-02 (RDP from a phone); 60 stays as the old floor.
foreach ($vw in @(50, 60)) {
    $verdictInfo.Matches = $false
    $fv = @(Get-MaintenanceFrame -Info $verdictInfo -Width $vw -Height $script:MinHeight)
    Assert-Equal 1 (@($fv | Where-Object { $_ -match 'press u to install it' }).Count) "width ${vw}: the mismatch verdict keeps its advice"
    $verdictInfo.Matches = $true
    $fv = @(Get-MaintenanceFrame -Info $verdictInfo -Width $vw -Height $script:MinHeight)
    Assert-Equal 1 (@($fv | Where-Object { $_ -match 'newest downloaded build' }).Count) "width ${vw}: the match verdict renders complete"
}

# --- Invoke-ClaudeUpdate ---------------------------------------------------------------------
# Asserted now that -Updater is injectable. The real `claude update` downloads ~300 MB and mutates
# the live installation; the behaviour under test is what happens AFTER it returns, which is where
# the bug was.
$upDir = Join-Path $tmp 'update-versions'
New-Item -ItemType Directory -Force -Path $upDir | Out-Null
Set-Content -Path (Join-Path $upDir '2.1.240') -Value 'build 240' -NoNewline
$upBin = Join-Path $tmp 'update-bin.exe'

# The case measured on this machine 2026-08-13: the updater exits 0 saying it succeeded and the
# binary is untouched, because Windows will not overwrite a loaded image. Assert on the resulting
# file CONTENT - a message claiming success is precisely what could not be trusted here.
Set-Content -Path $upBin -Value 'build 200' -NoNewline
$r = Invoke-ClaudeUpdate -BinPath $upBin -VersionsDir $upDir -Updater { 'Successfully updated from 2.1.200 to version 2.1.240' }
Assert-Equal 'build 240' (Get-Content -LiteralPath $upBin -Raw) 'a lying updater still ends with the newest build installed'
Assert-Equal $true $r.Matches 'the reported result is the re-hash, not the updater message'
Assert-Equal $true $r.Swapped 'the rename swap is reported as having happened'
Assert-Equal $true ($r.Message -match '2\.1\.240') 'the status names the build that is now installed'

# An updater that really did install must not trigger a swap - a pointless rename would leave a
# 300 MB .old file behind on every press of u.
Set-Content -Path $upBin -Value 'build 200' -NoNewline
Remove-Item -LiteralPath "$upBin.old" -Force -ErrorAction SilentlyContinue
$r = Invoke-ClaudeUpdate -BinPath $upBin -VersionsDir $upDir -Updater { Set-Content -Path $upBin -Value 'build 240' -NoNewline; 'Successfully updated' }
Assert-Equal $true  $r.Matches 'an updater that really installed is recognised'
Assert-Equal $false $r.Swapped 'no rename swap happens when the updater already landed'
Assert-Equal $false (Test-Path "$upBin.old") 'no .old file is left behind when no swap was needed'

# Nothing downloaded is a different problem from a failed swap, and must say so rather than send
# the reader off to close sessions that are not the cause.
$emptyDir = Join-Path $tmp 'empty-versions'
New-Item -ItemType Directory -Force -Path $emptyDir | Out-Null
$r = Invoke-ClaudeUpdate -BinPath $upBin -VersionsDir $emptyDir -Updater { 'nothing to do' }
Assert-Equal $false $r.Matches 'nothing downloaded is not a match'
Assert-Equal $true ($r.Message -match 'no build has been downloaded') 'the empty-versions case names its own cause'
Assert-Equal $true (Get-ClaudeInstallInfo -BinPath $upBin -VersionsDir $emptyDir).Native 'a versions directory that EXISTS (even empty) is a native install'

# deferred review finding: an npm/global install has no versions directory AT ALL - Maintenance.ps1
# hardcoded ~/.local/bin/claude.exe and ~/.local/share/claude/versions, so this case showed no hash
# and reported the download message even right after a real, successful update. The versions
# directory NOT EXISTING (never created, not merely empty) is what must trigger the different message.
$neverCreated = Join-Path $tmp 'never-created-versions'
Assert-Equal $false (Test-Path -LiteralPath $neverCreated) 'fixture sanity: the non-native versions dir does not exist'
Assert-Equal $false (Get-ClaudeInstallInfo -BinPath $upBin -VersionsDir $neverCreated).Native 'a versions directory that does not exist at all is reported as not native'
$rNonNative = Invoke-ClaudeUpdate -BinPath $upBin -VersionsDir $neverCreated -Updater { 'Successfully updated from 2.1.200 to version 2.1.240' }
Assert-Equal $false $rNonNative.Matches 'a non-native install cannot match a build that was never downloaded here'
Assert-Equal 'not a native install; updates are handled by your installer' $rNonNative.Message 'the message names the real situation instead of the download message'

# --- Get-DefaultClaudeBinPath: derives from wherever `claude` actually resolves --------------------
# deferred review finding: the maintenance functions' own BinPath defaults were hardcoded to the
# native installer's path, so an npm/global install always showed "no hash" regardless of what was
# really running. Asserted against the real PATH (this machine has claude on it, like any dev box
# these functions run on) rather than an injected resolver, because the fix IS the Get-Command call.
$resolvedClaude = Get-Command claude -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
if ($resolvedClaude) {
    Assert-Equal $resolvedClaude.Source (Get-DefaultClaudeBinPath) 'the default bin path is wherever claude actually resolves on PATH'
} else {
    Assert-Equal (Join-Path $HOME '.local\bin\claude.exe') (Get-DefaultClaudeBinPath) 'with no claude on PATH at all, the native path is the fallback'
}

# The maintenance screen is what someone opens when the install is already broken, so an updater
# that cannot start at all must become a status line, never a throw.
$threw = $false
try { $r = Invoke-ClaudeUpdate -BinPath $upBin -VersionsDir $upDir -Updater { throw 'claude: command not found' } }
catch { $threw = $true }
Assert-Equal $false $threw 'an updater that cannot start does not throw out of the screen'
Assert-Equal $false $r.Ran 'it is reported as not run'
Assert-Equal $true ($r.Message -match 'command not found') 'the reason reaches the screen'

# --- Hash cache. Added 2026-08-15 when Get-ClaudeInstallInfo turned out to cost 569 ms of the
# launcher's 1 560 ms, hashing a 305 MB binary twice at every start. A cache on that answer is only
# acceptable if it CANNOT go stale, and the whole module exists because the updater lies about
# exactly this - so the assertion that matters is the one where the bytes change and the cache has
# to notice. Same LENGTH on purpose: if the key were length-only, this case would silently pass. ---
$hashDir = Join-Path $tmp 'hashcache'
New-Item -ItemType Directory -Force -Path $hashDir | Out-Null
$script:HashCachePath = Join-Path $hashDir 'cache.json'
$big = Join-Path $hashDir 'big.bin'

[IO.File]::WriteAllBytes($big, [byte[]]::new(2MB))
$h1 = Get-CachedFileHash -Path $big
Assert-Equal $h1 (Get-CachedFileHash -Path $big) 'a second call returns the same hash'
Assert-Equal $true (Test-Path -LiteralPath $script:HashCachePath) 'a large file populates the cache file'

$changed = [byte[]]::new(2MB); $changed[0] = 7
[IO.File]::WriteAllBytes($big, $changed)
Assert-Equal $false ($h1 -eq (Get-CachedFileHash -Path $big)) 'changed bytes at the SAME length are not served from the cache'
Assert-Equal (Get-FileHash -LiteralPath $big).Hash (Get-CachedFileHash -Path $big) 'the cached answer equals the real hash'

# A small file must never be cached: the suites rewrite tiny fixtures many times a second, and a
# cache keyed on a timestamp would eventually hand one test the previous test's answer.
$small = Join-Path $hashDir 'small.txt'
Set-Content -LiteralPath $small -Value 'one' -NoNewline
$s1 = Get-CachedFileHash -Path $small
Set-Content -LiteralPath $small -Value 'two' -NoNewline
Assert-Equal $false ($s1 -eq (Get-CachedFileHash -Path $small)) 'a small file is hashed fresh every time, never cached'
$cacheAfter = Get-Content -LiteralPath $script:HashCachePath -Raw | ConvertFrom-Json
Assert-Equal $false ([bool]($cacheAfter.PSObject.Properties.Name -match 'small\.txt')) 'a small file never enters the cache file at all'

# Entries for files that no longer exist must not accumulate. Pruning happens when the cache is
# WRITTEN, i.e. on a miss - a hit returns early and touches no disk, which is the whole point of it.
# That bound is sufficient in practice because a new build always misses. Written this way after the
# first version of this assertion re-read a file that was already cached, got a hit, and reported
# the pruning as broken when it had simply never been asked to run.
$gone = Join-Path $hashDir 'gone.bin'
[IO.File]::WriteAllBytes($gone, [byte[]]::new(2MB))
$null = Get-CachedFileHash -Path $gone
Remove-Item -LiteralPath $gone -Force
$forceMiss = [byte[]]::new(2MB); $forceMiss[1] = 9
[IO.File]::WriteAllBytes($big, $forceMiss)
$null = Get-CachedFileHash -Path $big
$pruned = Get-Content -LiteralPath $script:HashCachePath -Raw | ConvertFrom-Json
Assert-Equal $false ([bool]($pruned.PSObject.Properties.Name -match 'gone\.bin')) 'an entry whose file is gone is pruned at the next write'
Assert-Equal $true ([bool]($pruned.PSObject.Properties.Name -match 'big\.bin')) 'the entry still in use survives the prune'

Assert-Equal $null (Get-CachedFileHash -Path (Join-Path $hashDir 'never-existed.bin')) 'a missing file yields null rather than throwing'

# A file that EXISTS but cannot be read is the same answer. It used to throw a raw PowerShell error
# into the maintenance screen - reachable whenever an antivirus or indexer holds a just-downloaded
# build, which is exactly when someone is looking at that screen. Both size branches, because only
# the large one goes through the cache.
$lockedSmall = Join-Path $hashDir 'locked-small.txt'
Set-Content -LiteralPath $lockedSmall -Value 'held' -NoNewline
$lockSmall = [IO.File]::Open($lockedSmall, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
try {
    # The RETURN value was already null before this was fixed - Get-FileHash's failure is
    # non-terminating, so $null.Hash is null either way. What changed is the ERROR STREAM: a
    # four-line red record went straight into the maintenance screen. Assert on the stream, or the
    # assertion cannot fail (a mutation run proved exactly that about the first version of it).
    $lockErr = @(Get-CachedFileHash -Path $lockedSmall 2>&1 | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] })
    Assert-Equal 0 $lockErr.Count 'a small file held open writes nothing to the error stream'
    Assert-Equal $null (Get-CachedFileHash -Path $lockedSmall) 'and answers null, exactly like a missing file'
} finally { $lockSmall.Dispose() }

$lockedBig = Join-Path $hashDir 'locked-big.bin'
[IO.File]::WriteAllBytes($lockedBig, [byte[]]::new(2MB))
$lockBig = [IO.File]::Open($lockedBig, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
try {
    $lockErrBig = @(Get-CachedFileHash -Path $lockedBig 2>&1 | Where-Object { $_ -is [System.Management.Automation.ErrorRecord] })
    Assert-Equal 0 $lockErrBig.Count 'a large file held open writes nothing to the error stream either'
    Assert-Equal $null (Get-CachedFileHash -Path $lockedBig) 'and answers null too'
} finally { $lockBig.Dispose() }
$lockCache = Get-Content -LiteralPath $script:HashCachePath -Raw | ConvertFrom-Json
Assert-Equal $false ([bool]($lockCache.PSObject.Properties.Name -match 'locked-big')) 'a failed hash is never remembered, so the next call retries'

# --- Prune and swap safety -------------------------------------------------------------------
# Four ways the maintenance screen could destroy the user's working CLI, each reachable from one
# keypress with no confirmation. Assertions are on the FILES, never on a returned message: a report
# that the right thing happened is exactly what this module exists not to trust.

$safeDir = Join-Path $tmp 'safe-versions'
New-Item -ItemType Directory -Force -Path $safeDir | Out-Null
foreach ($v in '2.1.260', '2.1.261', '2.1.262') { Set-Content -LiteralPath (Join-Path $safeDir $v) -Value "build $v" -NoNewline }
Set-Content -LiteralPath (Join-Path $safeDir 'notes.txt') -Value 'my notes' -NoNewline
Set-Content -LiteralPath (Join-Path $safeDir '2.1.264.exe.partial') -Value 'half a download' -NoNewline
$safeBin = Join-Path $tmp 'safe-bin.exe'
Set-Content -LiteralPath $safeBin -Value 'build 2.1.262' -NoNewline

# Prune had no name filter at all, and Get-OrderedClaudeBuilds sorts unparseable names LAST - so
# they were the FIRST things deleted: another launcher's in-flight download, and a file of the
# user's own that happened to sit in that directory.
$p = Remove-OldClaudeVersions -VersionsDir $safeDir -Keep 2 -BinPath $safeBin
Assert-Equal $true (Test-Path -LiteralPath (Join-Path $safeDir 'notes.txt')) 'prune never deletes a file that is not a build'
Assert-Equal $true (Test-Path -LiteralPath (Join-Path $safeDir '2.1.264.exe.partial')) 'prune never deletes an in-flight download'
Assert-Equal $false (Test-Path -LiteralPath (Join-Path $safeDir '2.1.260')) 'prune still removes the build it was asked to remove'
Assert-Equal 1 $p.Deleted.Count 'exactly one build was pruned'

# The in-use guard sat INSIDE `if (Test-Path $BinPath)`, so with the binary absent it was skipped
# entirely and the running build was deleted. Absent is not a rare state: it is the window between
# the rename and the copy of a swap, which a second launcher can walk into.
$gapDir = Join-Path $tmp 'gap-versions'
New-Item -ItemType Directory -Force -Path $gapDir | Out-Null
foreach ($v in '2.1.270', '2.1.271') { Set-Content -LiteralPath (Join-Path $gapDir $v) -Value "build $v" -NoNewline }
$g = Remove-OldClaudeVersions -VersionsDir $gapDir -Keep 1 -BinPath (Join-Path $tmp 'not-there.exe')
Assert-Equal 0 $g.Deleted.Count 'with no installed binary to identify, prune deletes nothing'
Assert-Equal $true (Test-Path -LiteralPath (Join-Path $gapDir '2.1.270')) 'the build that might have been the running one survives'

# Same fail-closed rule when the binary exists but matches no build on disk: unidentified is
# unidentified, and a silent no-op guard is how the running build got deleted.
$strayBin = Join-Path $tmp 'stray.exe'
Set-Content -LiteralPath $strayBin -Value 'a build this directory has never seen' -NoNewline
$g2 = Remove-OldClaudeVersions -VersionsDir $gapDir -Keep 1 -BinPath $strayBin
Assert-Equal 0 $g2.Deleted.Count 'an installed binary matching no build on disk also stops the prune'

# The swap renamed the working binary aside and only THEN copied. Any copy failure - an antivirus or
# a search indexer holding the just-downloaded build is the everyday one - left the user with no
# claude at all, and a claude.exe.old nobody told them about.
$swapDir = Join-Path $tmp 'swap-versions'
New-Item -ItemType Directory -Force -Path $swapDir | Out-Null
$swapSrc = Join-Path $swapDir '2.1.280'
Set-Content -LiteralPath $swapSrc -Value 'build 280' -NoNewline
$swapBin = Join-Path $tmp 'swap-bin.exe'
Set-Content -LiteralPath $swapBin -Value 'build 200' -NoNewline
$held = [IO.File]::Open($swapSrc, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
try { $sr = Repair-ClaudeBinaryByRename -BinPath $swapBin -VersionsDir $swapDir }
finally { $held.Dispose() }
Assert-Equal $false $sr.Ok 'a swap that cannot read the new build reports failure'
Assert-Equal $true (Test-Path -LiteralPath $swapBin) 'a failed swap leaves the working binary in place'
Assert-Equal 'build 200' (Get-Content -LiteralPath $swapBin -Raw) 'and it is the ORIGINAL binary, not a partial copy'
Assert-Equal $true ($sr.Message -match 'swap') 'the failure names the swap'

# BinPath is wherever `claude` resolves on PATH. With an npm or global shim first on PATH and a
# leftover versions directory from a native install tried once, the swap copied native build bytes
# straight over the shim and destroyed it.
$shimBin = Join-Path $tmp 'claude.cmd'
$shimText = '@echo off & node claude.js %*'
Set-Content -LiteralPath $shimBin -Value $shimText -NoNewline
$cr = Repair-ClaudeBinaryByRename -BinPath $shimBin -VersionsDir $swapDir
Assert-Equal $false $cr.Ok 'the swap refuses a binary that is not a native .exe'
Assert-Equal $shimText (Get-Content -LiteralPath $shimBin -Raw) 'an npm shim is left exactly as it was'
Assert-Equal $true ($cr.Message -match 'not a native install') 'and the message names why'
Assert-Equal $false (Test-Path -LiteralPath "$shimBin.old") 'no .old copy of the shim is left behind'

Remove-Item -Recurse -Force $tmp
# Invoke-ClaudeCommandText (Ui.ps1) is still not asserted: beyond try/catch its only logic is
# ConvertTo-StatusText, which Test-Ui.ps1 asserts, and exercising it means shelling out to the real
# binary.
if ($script:Ran -ne 99) { Write-Host "COULD NOT RUN: expected 99 assertions, ran $($script:Ran) - an assertion was skipped (its argument threw)"; exit 2 }
if ($script:Failed) { Write-Host ""; Write-Host "$script:Failed failed"; exit 1 }
Write-Host ""; Write-Host "all passed"
exit 0
